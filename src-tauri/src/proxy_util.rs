use serde::Serialize;
use serde_json::{json, Value};

const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
const DEFAULT_BYPASS_LIST: &str =
    "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;\
172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*";

#[derive(Debug, Clone, Serialize)]
pub struct ProxyStatus {
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub server: String,
    pub override_list: String,
}

pub fn enable(server: &str) -> Result<(), String> {
    set_system_proxy(true, server, DEFAULT_BYPASS_LIST)
}

pub fn disable() -> Result<(), String> {
    set_system_proxy(false, "", DEFAULT_BYPASS_LIST)
}

#[cfg(windows)]
pub fn set_system_proxy(enable: bool, server: &str, bypass: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            hkey: isize,
            lpsubkey: *const u16,
            uloptions: u32,
            samdesired: u32,
            phkresult: *mut isize,
        ) -> i32;
        fn RegSetValueExW(
            hkey: isize,
            lpvaluename: *const u16,
            reserved: u32,
            dwtype: u32,
            lpdata: *const u8,
            cbdata: u32,
        ) -> i32;
        fn RegCloseKey(hkey: isize) -> i32;
    }

    const HKEY_CURRENT_USER: isize = 0x80000001u32 as isize;
    const KEY_WRITE: u32 = 0x20006;
    const REG_SZ: u32 = 1;
    const REG_DWORD: u32 = 4;
    const ERROR_SUCCESS: i32 = 0;

    let wide = |value: &str| {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let path_w = wide(INTERNET_SETTINGS);
    let mut hkey = 0isize;

    unsafe {
        let status = RegOpenKeyExW(HKEY_CURRENT_USER, path_w.as_ptr(), 0, KEY_WRITE, &mut hkey);
        if status != ERROR_SUCCESS {
            return Err(format!(
                "[proxy_util.rs:{}] RegOpenKeyExW 失败; root=HKEY_CURRENT_USER; \
                 path={INTERNET_SETTINGS}; access=KEY_WRITE; win32_error={status}",
                line!()
            ));
        }

        let write_sz = |name: &str, value: &str| -> Result<(), String> {
            let name_w = wide(name);
            let value_w = wide(value);
            let status = RegSetValueExW(
                hkey,
                name_w.as_ptr(),
                0,
                REG_SZ,
                value_w.as_ptr() as *const u8,
                (value_w.len() * 2) as u32,
            );
            if status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(format!(
                    "[proxy_util.rs:{}] RegSetValueExW 写入 {name} 失败; \
                     type=REG_SZ; win32_error={status}",
                    line!()
                ))
            }
        };

        let write_dword = |name: &str, value: u32| -> Result<(), String> {
            let name_w = wide(name);
            let status = RegSetValueExW(
                hkey,
                name_w.as_ptr(),
                0,
                REG_DWORD,
                &value as *const u32 as *const u8,
                std::mem::size_of::<u32>() as u32,
            );
            if status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(format!(
                    "[proxy_util.rs:{}] RegSetValueExW 写入 {name} 失败; \
                     type=REG_DWORD; win32_error={status}",
                    line!()
                ))
            }
        };

        let result = if enable {
            write_sz("ProxyServer", server)
                .and_then(|_| write_dword("ProxyEnable", 1))
                .and_then(|_| write_sz("ProxyOverride", bypass))
        } else {
            write_dword("ProxyEnable", 0)
        };

        let close_status = RegCloseKey(hkey);
        if let Err(error) = result {
            return Err(error);
        }
        if close_status != ERROR_SUCCESS {
            return Err(format!(
                "[proxy_util.rs:{}] RegCloseKey 失败; win32_error={close_status}",
                line!()
            ));
        }
    }

    refresh_wininet();
    Ok(())
}

#[cfg(not(windows))]
pub fn set_system_proxy(_enable: bool, _server: &str, _bypass: &str) -> Result<(), String> {
    Err("仅支持 Windows".to_string())
}

#[cfg(windows)]
fn refresh_wininet() {
    const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
    const INTERNET_OPTION_REFRESH: u32 = 37;

    #[link(name = "wininet")]
    extern "system" {
        fn InternetSetOptionW(
            hinternet: *mut std::ffi::c_void,
            option: u32,
            buffer: *mut std::ffi::c_void,
            buffer_length: u32,
        ) -> i32;
    }

    unsafe {
        let null = std::ptr::null_mut::<std::ffi::c_void>();
        let _ = InternetSetOptionW(null, INTERNET_OPTION_SETTINGS_CHANGED, null, 0);
        let _ = InternetSetOptionW(null, INTERNET_OPTION_REFRESH, null, 0);
    }
}

#[cfg(windows)]
pub fn status() -> ProxyStatus {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            hkey: isize,
            lpsubkey: *const u16,
            uloptions: u32,
            samdesired: u32,
            phkresult: *mut isize,
        ) -> i32;
        fn RegQueryValueExW(
            hkey: isize,
            lpvaluename: *const u16,
            reserved: *mut u32,
            lp_type: *mut u32,
            lpdata: *mut u8,
            lpcbdata: *mut u32,
        ) -> i32;
        fn RegCloseKey(hkey: isize) -> i32;
    }

    const HKEY_CURRENT_USER: isize = 0x80000001u32 as isize;
    const KEY_READ: u32 = 0x20019;
    let path_w = OsStr::new(INTERNET_SETTINGS)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let mut hkey = 0isize;

    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, path_w.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return ProxyStatus::default();
        }
        let wide = |value: &str| {
            OsStr::new(value)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let query = |name: &str| -> Option<Vec<u8>> {
            let name_w = wide(name);
            let mut size = 0u32;
            if RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            ) != 0
            {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            if RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                data.as_mut_ptr(),
                &mut size,
            ) != 0
            {
                return None;
            }
            data.truncate(size as usize);
            Some(data)
        };
        let enable = query("ProxyEnable")
            .and_then(|data| {
                let bytes = data.get(..4)?;
                Some(u32::from_ne_bytes(bytes.try_into().ok()?))
            })
            .unwrap_or(0)
            != 0;
        let read_string = |name: &str| {
            query(name)
                .map(|data| {
                    let units = data
                        .chunks_exact(2)
                        .map(|bytes| u16::from_ne_bytes([bytes[0], bytes[1]]))
                        .take_while(|unit| *unit != 0)
                        .collect::<Vec<_>>();
                    String::from_utf16_lossy(&units)
                })
                .unwrap_or_default()
        };
        let server = read_string("ProxyServer");
        let override_list = read_string("ProxyOverride");
        let _ = RegCloseKey(hkey);
        let (host, port) = parse_server(&server).unwrap_or_default();
        return ProxyStatus {
            enable,
            host,
            port,
            server,
            override_list,
        };
    }
}

fn parse_server(server: &str) -> Option<(String, u16)> {
    let (host, port) = server.trim().rsplit_once(':')?;
    Some((host.to_string(), port.parse().ok()?))
}

#[cfg(not(windows))]
pub fn status() -> ProxyStatus {
    ProxyStatus::default()
}

impl Default for ProxyStatus {
    fn default() -> Self {
        Self {
            enable: false,
            host: String::new(),
            port: 0,
            server: String::new(),
            override_list: String::new(),
        }
    }
}

pub fn status_json() -> Value {
    let status = status();
    json!({
        "enable": status.enable,
        "enabled": status.enable,
        "host": status.host,
        "port": status.port,
        "server": status.server,
        "override": status.override_list
    })
}

pub fn cleanup_stale(port: u16) -> bool {
    let expected = format!("127.0.0.1:{port}");
    let status = status();
    status.enable && status.server.eq_ignore_ascii_case(&expected) && disable().is_ok()
}
