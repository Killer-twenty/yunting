// sing-box 内核进程管理（Rust 重写，tokio::process::Command）
// 职责: config check → spawn → 日志通道 → 优雅停止（杀进程树）→ 孤儿清理
// 日志经 mpsc 通道交给调用方（main.rs 转发到 WebView 事件）
//
// 状态设计: running/pid/exit_code 抽成 Arc<KernelStatus>，可无锁同步读取。
// —— 命令层需要在 async 上下文和非 runtime 上下文（窗口事件）都能读状态，
//    绝不能对 tokio::sync::Mutex 调 blocking_lock（会 panic：Cannot block the current thread from within a runtime）
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::paths::kernel_exe;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernelSnapshot {
    pub running: bool,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub phase: String,
    /// 内核实际监听的 mixed 端口（可能因占用顺延，与设置值不同）
    pub mixed_port: Option<u16>,
    /// 内核实际监听的 clash api 端口（同上，顺延时与设置值不同）
    pub api_port: Option<u16>,
}

/// 内核运行时状态（内部全是原子量 / std Mutex，任何线程都可同步读取）
pub struct KernelStatus {
    running: AtomicBool,
    pid: Mutex<Option<u32>>,
    exit_code: Mutex<Option<i32>>,
    mixed_port: Mutex<Option<u16>>,
    api_port: Mutex<Option<u16>>,
}

impl KernelStatus {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            pid: Mutex::new(None),
            exit_code: Mutex::new(None),
            mixed_port: Mutex::new(None),
            api_port: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> KernelSnapshot {
        let running = self.running.load(Ordering::SeqCst);
        KernelSnapshot {
            running,
            pid: *self.pid.lock().unwrap_or_else(|e| e.into_inner()),
            exit_code: *self.exit_code.lock().unwrap_or_else(|e| e.into_inner()),
            phase: if running {
                "running".into()
            } else {
                "stopped".into()
            },
            mixed_port: *self.mixed_port.lock().unwrap_or_else(|e| e.into_inner()),
            api_port: *self.api_port.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }

    fn set_running(&self, running: bool, pid: Option<u32>) {
        self.running.store(running, Ordering::SeqCst);
        *self.pid.lock().unwrap_or_else(|e| e.into_inner()) = pid;
        if !running {
            *self.mixed_port.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.api_port.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn set_mixed_port(&self, port: Option<u16>) {
        *self.mixed_port.lock().unwrap_or_else(|e| e.into_inner()) = port;
    }

    fn set_api_port(&self, port: Option<u16>) {
        *self.api_port.lock().unwrap_or_else(|e| e.into_inner()) = port;
    }

    fn set_exit_code(&self, code: Option<i32>) {
        *self.exit_code.lock().unwrap_or_else(|e| e.into_inner()) = code;
    }
}

pub struct KernelManager {
    child: Mutex<Option<Child>>,
    status: Arc<KernelStatus>,
    log_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
}

impl KernelManager {
    /// 返回 manager 与状态句柄（状态句柄可交给命令层无锁读取）
    pub fn new() -> (Self, Arc<KernelStatus>) {
        let status = Arc::new(KernelStatus::new());
        (
            Self {
                child: Mutex::new(None),
                status: status.clone(),
                log_tx: Mutex::new(None),
            },
            status,
        )
    }

    /// 注册日志接收端（main.rs 拿 rx 转发到 WebView）。重复调用覆盖旧通道。
    pub fn take_log_rx(&self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.log_tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        rx
    }

    fn log(&self, line: String) {
        if let Some(tx) = self
            .log_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = tx.send(line);
        }
    }

    /// 启动内核。日志走通道；状态本函数返回即视为 running（就绪等待由调用方做）。
    /// mixed_port/api_port: 本次启动实际使用的端口（供状态快照/系统代理/延迟测试对齐）
    pub async fn start(
        &self,
        exe: &Path,
        cfg: &Path,
        work_dir: &Path,
        mixed_port: Option<u16>,
        api_port: Option<u16>,
    ) -> Result<u32, String> {
        {
            let g = self.child.lock().unwrap_or_else(|e| e.into_inner());
            if g.is_some() {
                return Err("内核已在运行".into());
            }
        }
        if !exe.exists() {
            return Err(format!("内核不存在: {}", exe.display()));
        }
        let mut cmd = Command::new(exe);
        cmd.args([
            "run",
            "-c",
            &cfg.to_string_lossy(),
            "-D",
            &work_dir.to_string_lossy(),
        ])
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags_hide()
        .kill_on_drop(true); // guard 丢失时自动杀进程，防孤儿
        let mut child = cmd.spawn().map_err(|e| format!("内核启动失败: {e}"))?;
        let pid = child.id().unwrap_or(0);
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

        self.status.set_exit_code(None);
        self.status.set_mixed_port(mixed_port);
        self.status.set_api_port(api_port);
        self.status.set_running(true, Some(pid));
        *self.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        Ok(pid)
    }

    fn log_sender(&self) -> mpsc::UnboundedSender<String> {
        if let Some(tx) = self
            .log_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
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
            let mut g = self.child.lock().unwrap_or_else(|e| e.into_inner());
            match g.take() {
                Some(child) => child.id().unwrap_or(0),
                None => {
                    self.status.set_running(false, None);
                    return Ok(());
                }
            }
        };
        if pid > 0 {
            self.log(format!("退出清理: taskkill /T /F pid={pid}"));
            kill_process_tree(pid);
        }
        self.status.set_running(false, None);
        Ok(())
    }

    /// 停止: kill 主进程 → 3s 等待 → taskkill /T /F 杀进程树兜底
    /// （guard 不跨 await：同步块内 take child，drop guard 后再异步等待）
    pub async fn stop(&self) -> Result<(), String> {
        let mut child = {
            let mut child_g = self.child.lock().unwrap_or_else(|e| e.into_inner());
            match child_g.take() {
                Some(c) => c,
                None => {
                    self.status.set_running(false, None);
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
                    let code = status.code();
                    self.log(format!("内核退出 code={}", code.unwrap_or(-1)));
                    self.status.set_exit_code(code);
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
        self.status.set_running(false, None);
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
    cmd.args(["check", "-c", &cfg.to_string_lossy()])
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().map_err(|e| format!("check 执行失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "config 校验失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// clash api 就绪等待（阻塞式原始 HTTP 探测，无 reqwest::blocking 依赖 —— 它在 tokio 运行时内会挂起）
pub fn wait_api_ready(port: u16, timeout: Duration) -> Result<(), String> {
    use std::io::{Read, Write};
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let req = format!(
                "GET /version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(req.as_bytes()).is_ok() {
                let mut buf = [0u8; 512];
                if let Ok(n) = stream.read(&mut buf) {
                    // 必须是 HTTP 2xx（clash api 就绪），随便一个监听端口不算
                    if String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 2") {
                        return Ok(());
                    }
                }
            }
        }
        if std::time::Instant::now() > deadline {
            return Err("clash api 就绪超时".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// 等待 mixed 代理端口开始监听，确保系统代理不会指向尚未启动的端口。
pub fn wait_port_ready(port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(format!("mixed 代理端口 {port} 就绪超时"));
        }
        std::thread::sleep(Duration::from_millis(100));
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
        let Some((pid_str, cmdline)) = line.split_once('|') else {
            continue;
        };
        let Ok(pid) = pid_str.trim().parse::<u32>() else {
            continue;
        };
        if cmdline.to_lowercase().contains(&my_kernel_str) {
            kill_process_tree(pid);
            killed += 1;
        }
    }
    killed
}
