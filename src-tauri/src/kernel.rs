// sing-box 内核进程管理（Rust 重写，tokio::process::Command）
// 职责: config check → spawn → 日志通道 → 优雅停止（杀进程树）→ 孤儿清理
// 日志经 mpsc 通道交给调用方（main.rs 转发到 WebView 事件）
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::paths::kernel_exe;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernelState {
    pub running: bool,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub phase: String,
}

pub struct KernelManager {
    child: Mutex<Option<Child>>,
    pid: Mutex<Option<u32>>,
    running: AtomicBool,
    exit_code: Mutex<Option<i32>>,
    log_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
}

impl KernelManager {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            pid: Mutex::new(None),
            running: AtomicBool::new(false),
            exit_code: Mutex::new(None),
            log_tx: Mutex::new(None),
        }
    }

    pub fn get_state(&self) -> KernelState {
        KernelState {
            running: self.running.load(Ordering::SeqCst),
            pid: *self.pid.lock().unwrap(),
            exit_code: *self.exit_code.lock().unwrap(),
            phase: if self.running.load(Ordering::SeqCst) { "running".into() } else { "stopped".into() },
        }
    }

    /// 注册日志接收端（main.rs 拿 rx 转发到 WebView）。重复调用覆盖旧通道。
    pub fn take_log_rx(&self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.log_tx.lock().unwrap() = Some(tx);
        rx
    }

    fn log(&self, line: String) {
        if let Some(tx) = self.log_tx.lock().unwrap().as_ref() {
            let _ = tx.send(line);
        }
    }

    /// 启动内核。日志走通道；状态本函数返回即视为 running（就绪等待由调用方做）。
    pub async fn start(&self, exe: &Path, cfg: &Path, work_dir: &Path) -> Result<u32, String> {
        {
            let g = self.child.lock().unwrap();
            if g.is_some() {
                return Err("内核已在运行".into());
            }
        }
        if !exe.exists() {
            return Err(format!("内核不存在: {}", exe.display()));
        }
        let mut cmd = Command::new(exe);
        cmd.args(["run", "-c", &cfg.to_string_lossy(), "-D", &work_dir.to_string_lossy()])
            .current_dir(work_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags_hide();
        let mut child = cmd.spawn().map_err(|e| format!("内核启动失败: {e}"))?;
        let pid = child.id().unwrap_or(0);
        *self.pid.lock().unwrap() = Some(pid);
        self.log(format!("内核已启动 pid={pid}"));

        // stdout / stderr 逐行转发到日志通道
        if let Some(out) = child.stdout.take() {
            let log = self.log_sender();
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(out);
                let mut line = String::new();
                use tokio::io::AsyncBufReadExt;
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let l = line.trim_end().to_string();
                            if !l.is_empty() {
                                let _ = log.send(l);
                            }
                        }
                    }
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            let log = self.log_sender();
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(err);
                let mut line = String::new();
                use tokio::io::AsyncBufReadExt;
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let l = line.trim_end().to_string();
                            if !l.is_empty() {
                                let _ = log.send(l);
                            }
                        }
                    }
                }
            });
        }

        self.running.store(true, Ordering::SeqCst);
        *self.exit_code.lock().unwrap() = None;
        *self.child.lock().unwrap() = Some(child);
        Ok(pid)
    }

    fn log_sender(&self) -> mpsc::UnboundedSender<String> {
        if let Some(tx) = self.log_tx.lock().unwrap().as_ref() {
            tx.clone()
        } else {
            // 无订阅者: 丢弃日志
            let (tx, _rx) = mpsc::unbounded_channel();
            tx
        }
    }

    /// 停止（同步版，供退出钩子调用）: 直接 taskkill /T /F 杀进程树
    pub fn stop_blocking(&self) -> Result<(), String> {
        let pid = {
            let mut g = self.child.lock().unwrap();
            match g.take() {
                Some(child) => child.id().unwrap_or(0),
                None => {
                    self.running.store(false, Ordering::SeqCst);
                    *self.pid.lock().unwrap() = None;
                    return Ok(());
                }
            }
        };
        if pid > 0 {
            self.log(format!("退出清理: taskkill /T /F pid={pid}"));
            kill_process_tree(pid);
        }
        self.running.store(false, Ordering::SeqCst);
        *self.pid.lock().unwrap() = None;
        Ok(())
    }

    /// 停止: kill 主进程 → 3s 等待 → taskkill /T /F 杀进程树兜底
    /// （guard 不跨 await：同步块内 take child，drop guard 后再异步等待）
    pub async fn stop(&self) -> Result<(), String> {
        let mut child = {
            let mut child_g = self.child.lock().unwrap();
            match child_g.take() {
                Some(c) => c,
                None => {
                    self.running.store(false, Ordering::SeqCst);
                    *self.pid.lock().unwrap() = None;
                    return Ok(());
                }
            }
        };
        let pid = child.id().unwrap_or(0);
        let _ = child.kill().await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.log(format!("内核退出 code={}", status.code().unwrap_or(-1)));
                    break;
                }
                Ok(None) => {
                    if tokio::time::Instant::now() > deadline {
                        if pid > 0 {
                            self.log(format!("3s 未退出，taskkill /T /F pid={pid}"));
                            kill_process_tree(pid);
                        }
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(_) => break,
            }
        }
        let _ = child.wait().await;
        self.running.store(false, Ordering::SeqCst);
        *self.pid.lock().unwrap() = None;
        Ok(())
    }
}

trait HideWin {
    fn creation_flags_hide(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl HideWin for Command {
    fn creation_flags_hide(&mut self) -> &mut Self {
        self.creation_flags(0x08000000); // CREATE_NO_WINDOW
        self
    }
}

#[cfg(not(windows))]
impl HideWin for Command {
    fn creation_flags_hide(&mut self) -> &mut Self {
        self
    }
}

/// taskkill /T /F（杀进程树）
fn kill_process_tree(pid: u32) {
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let _ = cmd.status();
}

/// config 校验（sing-box check -c）
pub fn check_config(exe: &Path, cfg: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["check", "-c", &cfg.to_string_lossy()]).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().map_err(|e| format!("check 执行失败: {e}"))?;
    if !out.status.success() {
        return Err(format!("config 校验失败: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// clash api 就绪等待（阻塞式循环 GET /version；放 spawn_blocking 跑）
pub fn wait_api_ready(port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    let url = format!("http://127.0.0.1:{port}/version");
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    loop {
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        if std::time::Instant::now() > deadline {
            return Err("clash api 就绪超时".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// 启动时清理孤儿 sing-box（按可执行文件路径匹配）。返回清理数量。
pub fn cleanup_orphans() -> u32 {
    let my_kernel = kernel_exe();
    let my_kernel_str = my_kernel.to_string_lossy().to_lowercase();
    // wmic 在 Win11 24H2 已移除 → PowerShell CIM
    let mut cmd = std::process::Command::new("powershell");
    cmd.args([
        "-NoProfile", "-Command",
        "Get-CimInstance Win32_Process -Filter \"Name='sing-box.exe'\" | ForEach-Object { \"$($_.ProcessId)|$($_.CommandLine)\" }",
    ])
    .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let Ok(out) = cmd.output() else { return 0 };
    if !out.status.success() {
        return 0;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut killed = 0;
    for line in text.lines() {
        let Some((pid_str, cmdline)) = line.split_once('|') else { continue };
        let Ok(pid) = pid_str.trim().parse::<u32>() else { continue };
        if cmdline.to_lowercase().contains(&my_kernel_str) {
            kill_process_tree(pid);
            killed += 1;
        }
    }
    killed
}
