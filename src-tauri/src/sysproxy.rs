// Windows 系统代理 —— 注册表 + WM_SETTINGCHANGE 广播（Rust 重写，windows crate 原生 API）
// HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings
// 写入策略: windows-rs 直写 → 失败时 UAC 提权兜底（对齐旧版三层策略的语义）
use std::sync::Mutex;

use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, NO_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_SZ, REG_VALUE_TYPE, REG_DWORD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};

const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub enabled: bool,
    pub server: String,
    pub ours: bool,
}

// 记住开代理前的原值，退出恢复用；None = 原本没开
static SAVED: Mutex<Option<SavedProxy>> = Mutex::new(None);

#[derive(Clone)]
struct SavedProxy {
    enabled: bool,
    server: String,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(v: &[u8]) -> String {
    let u16s: Vec<u16> = v
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
    String::from_utf16_lossy(&u16s[..end])
}

/// 打开 HKCU Internet Settings key（不存在则创建；RegCreateKeyW 以 KEY_ALL_ACCESS 打开）
fn open_key(_access: u32) -> Result<HKEY, String> {
    let subkey = to_wide(SUBKEY);
    let mut hkey = HKEY(std::ptr::null_mut());
    let res = unsafe {
        RegCreateKeyW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            &mut hkey,
        )
    };
    if res != NO_ERROR {
        return Err(format!("RegCreateKeyW error {res:?}"));
    }
    Ok(hkey)
}

/// 直写注册表 SZ 值
fn reg_set_sz(name: &str, value: &str) -> Result<(), String> {
    let hkey = open_key(KEY_WRITE.0)?;
    let name_w = to_wide(name);
    let data = to_wide(value);
    let bytes: Vec<u8> = data.iter().flat_map(|u| u.to_le_bytes()).collect();
    let res = unsafe {
        RegSetValueExW(hkey, PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(bytes.as_slice()))
    };
    unsafe { let _ = RegCloseKey(hkey); }
    if res != NO_ERROR {
        return Err(format!("RegSetValueExW({name}) error {res:?}"));
    }
    Ok(())
}

/// 直写注册表 DWORD 值
fn reg_set_dword(name: &str, value: u32) -> Result<(), String> {
    let hkey = open_key(KEY_WRITE.0)?;
    let name_w = to_wide(name);
    let res = unsafe {
        RegSetValueExW(hkey, PCWSTR(name_w.as_ptr()), 0, REG_DWORD, Some(value.to_le_bytes().as_slice()))
    };
    unsafe { let _ = RegCloseKey(hkey); }
    if res != NO_ERROR {
        return Err(format!("RegSetValueExW({name}) error {res:?}"));
    }
    Ok(())
}

/// 读注册表原始值
fn reg_query_raw(name: &str) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>, String> {
    let hkey = open_key(KEY_READ.0)?;
    let name_w = to_wide(name);
    let mut buf = vec![0u8; 2048];
    let mut len = buf.len() as u32;
    let mut vtype = REG_VALUE_TYPE(0);
    let res = unsafe {
        RegQueryValueExW(hkey, PCWSTR(name_w.as_ptr()), None, Some(&mut vtype), Some(buf.as_mut_ptr()), Some(&mut len))
    };
    unsafe { let _ = RegCloseKey(hkey); }
    match res {
        NO_ERROR => {
            buf.truncate(len as usize);
            Ok(Some((vtype, buf)))
        }
        ERROR_FILE_NOT_FOUND => Ok(None),
        e => Err(format!("RegQueryValueExW({name}) error {e:?}")),
    }
}

fn read_proxy_server() -> String {
    match reg_query_raw("ProxyServer") {
        Ok(Some((_, data))) => from_wide(&data),
        _ => String::new(),
    }
}

fn read_proxy_enable() -> bool {
    match reg_query_raw("ProxyEnable") {
        Ok(Some((_, data))) => data.first().copied().unwrap_or(0) == 1,
        _ => false,
    }
}

/// 广播 WM_SETTINGCHANGE，让浏览器等立即生效
fn broadcast_setting_change() {
    use windows::Win32::Foundation::LPARAM;
    let param = to_wide("Internet Settings");
    let lparam = LPARAM(param.as_ptr() as isize);
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            None,
            lparam,
            SMTO_ABORTIFHUNG,
            3000,
            Some(&mut result),
        );
    };
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableResult {
    pub server: String,
    pub via: String,
}

/// 开系统代理（127.0.0.1:port）。记录原值用于恢复（仅在首次 enable 时记录，
/// 重复 enable 不清掉最早的原始值，保证退出恢复链不丢）。
pub fn enable(port: u16) -> Result<EnableResult, String> {
    let was_enabled = read_proxy_enable();
    let prev_server = read_proxy_server();
    let first_record = {
        let mut s = SAVED.lock().unwrap();
        let first = s.is_none();
        if first {
            *s = Some(SavedProxy { enabled: was_enabled, server: prev_server });
        }
        first
    };
    let server = format!("127.0.0.1:{port}");
    // 层1: windows-rs 直写（HKCU 本应无需提权）
    let write = (|| -> Result<(), String> {
        reg_set_sz("ProxyServer", &server)?;
        reg_set_dword("ProxyEnable", 1)?;
        Ok(())
    })();
    let via = match write {
        Ok(()) => "winreg".to_string(),
        Err(e) => {
            // 层2: UAC 提权兜底（用户点"是"）
            eprintln!("[sysproxy] winreg direct write failed: {e}; elevating...");
            match elevate_set(&server) {
                Ok(()) => "elevate".to_string(),
                Err(e2) => {
                    if first_record {
                        *SAVED.lock().unwrap() = None;
                    }
                    return Err(format!("系统代理写入失败(已试 winreg/管理员两种方式): {e} / {e2}"));
                }
            }
        }
    };
    broadcast_setting_change();
    Ok(EnableResult { server, via })
}

/// UAC 提权写入兜底（-EncodedCommand 绕引号转义问题；一次提权写两条注册表值）
/// CREATE_NO_WINDOW: 隐藏 powershell 自身窗口（否则每次连接弹黑框）
fn elevate_set(server: &str) -> Result<(), String> {
    use std::process::Command;
    let encoded = {
        let ps_inner = format!(
            "Start-Process cmd -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '/c', 'reg add \"{SUBKEY}\" /v ProxyServer /t REG_SZ /d \"{server}\" /f & reg add \"{SUBKEY}\" /v ProxyEnable /t REG_DWORD /d 1 /f'"
        );
        let utf16: Vec<u8> = ps_inner.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        base64_encode(&utf16)
    };
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    // 注册表 flush 缓冲
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}

/// 标准 base64（无依赖实现）
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 关系统代理。restore=true 且之前记过原值 → 恢复原值而非简单关闭。
/// 恢复失败不再静默吞掉 —— 返回值让调用方打日志提示。
pub fn disable_restore() -> Result<(), String> {
    let saved = SAVED.lock().unwrap().clone();
    let r = match saved {
        Some(s) if s.enabled && !s.server.is_empty() => {
            reg_set_sz("ProxyServer", &s.server)?;
            reg_set_dword("ProxyEnable", 1)
        }
        _ => reg_set_dword("ProxyEnable", 0),
    };
    *SAVED.lock().unwrap() = None;
    broadcast_setting_change();
    r
}

/// 强制关闭（不恢复原值）
pub fn disable_force() {
    let _ = reg_set_dword("ProxyEnable", 0);
    *SAVED.lock().unwrap() = None;
    broadcast_setting_change();
}

pub fn status() -> ProxyStatus {
    let enabled = read_proxy_enable();
    let server = read_proxy_server();
    let ours = enabled && server.starts_with("127.0.0.1:");
    ProxyStatus { enabled, server, ours }
}

/// 启动残留检测: 系统代理指向 port → 自动清掉（启动时内核必未运行）。返回是否执行了清理。
pub fn cleanup_stale(port: u16) -> bool {
    let s = status();
    if !s.enabled {
        return false;
    }
    if s.server == format!("127.0.0.1:{port}") {
        disable_force();
        return true;
    }
    false
}
