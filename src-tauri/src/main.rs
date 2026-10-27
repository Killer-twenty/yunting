#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! yunting Tauri 版主入口
//! 架构:
//!   ui/                —— 复用旧版原生 JS 前端 + 纯函数模块(parsers/configBuilder)
//!   src-tauri/src/     —— Rust: paths/store/kernel/sysproxy/clashapi/configbuilder
//! 数据: %APPDATA%\yunting（与 Electron 版数据隔离）；不可写时兜底 exe 旁 data/
//! 内核: 打包态 exe 旁 kernel/；开发态项目 kernel/

mod clashapi;
mod configbuilder;
mod kernel;
mod paths;
mod store;
mod sysproxy;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use std::sync::Arc;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use paths::{data_dir, kernel_exe};
use store::{load_nodes, load_settings, load_subs, save_nodes, save_subs, Settings};

/// 内核进程句柄（tokio Mutex: async command 内安全持锁）
struct KernelState(pub Mutex<kernel::KernelManager>);

/// 内核状态快照句柄（Arc<KernelStatus> 内部全是原子量/std Mutex）
/// —— 任何上下文（async 命令 / 窗口事件 / 同步命令）都能无锁读取，
///    严禁对 tokio::sync::Mutex 调 blocking_lock（runtime 内会 panic）
struct KernelStatusHandle(pub Arc<kernel::KernelStatus>);

fn now_hms() -> String {
    // 本地时间 HH:MM:SS（避免引 chrono；GetLocalTime 直接拿）
    #[repr(C)]
    #[derive(Default)]
    struct ST {
        _y: u16, _mo: u16, _dow: u16, _d: u16, h: u16, m: u16, s: u16, _ms: u16,
    }
    extern "system" { fn GetLocalTime(p: *mut ST); }
    unsafe {
        let mut st = ST::default();
        GetLocalTime(&mut st);
        format!("{:02}:{:02}:{:02}", st.h, st.m, st.s)
    }
}

fn log_line(app: &AppHandle, line: impl AsRef<str>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.emit("log", format!("[{}] {}", now_hms(), line.as_ref()));
    }
}

/// 读取内核状态（无锁同步，任何上下文安全）
fn core_snapshot(app: &AppHandle) -> kernel::KernelSnapshot {
    match app.try_state::<KernelStatusHandle>() {
        Some(h) => h.0.snapshot(),
        None => kernel::KernelStatus::new().snapshot(),
    }
}

fn broadcast_state(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let core = core_snapshot(app);
        let proxy = sysproxy::status();
        let _ = win.emit("state", json!({ "core": core, "proxy": proxy }));
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 端口是否被占用。
/// SO_REUSEADDR 语义: Windows 上 TcpListener::bind 对 TIME_WAIT 连接通常能成功，
/// 但对真正被 listen 的端口会失败 —— 正是我们要的判定。
fn port_busy(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

// ============================================================
// 节点
// ============================================================
#[tauri::command]
fn nodes_get() -> Vec<Value> {
    load_nodes()
}

/// 前端把解析好的节点数组传过来（parsers.js 在 WebView 内执行）
#[tauri::command]
fn nodes_add(app: AppHandle, nodes: Vec<Value>) -> Result<Value, String> {
    if nodes.is_empty() {
        return Err("没有可导入的节点".into());
    }
    let existing = load_nodes();
    let parsed_count = nodes.len();
    let mut merged = existing.clone();
    for n in nodes {
        merged.push(n);
    }
    let merged = dedupe(merged);
    let added = merged.len() - existing.len();
    save_nodes(merged.clone())?;
    log_line(&app, format!("导入 {added} 个节点（识别 {parsed_count}，去重后新增 {added}）"));
    Ok(json!({ "added": added, "total": merged.len() }))
}

#[tauri::command]
fn nodes_remove(tag: String) -> Result<usize, String> {
    let nodes = load_nodes();
    let next: Vec<_> = nodes
        .into_iter()
        .filter(|n| n.get("tag").and_then(|v| v.as_str()) != Some(tag.as_str()))
        .collect();
    let n = next.len();
    save_nodes(next)?;
    Ok(n)
}

/// 与旧版 parsers.js dedupeNodes 一致：raw 相同或 type|server|port 相同视为同一节点
fn dedupe(nodes: Vec<Value>) -> Vec<Value> {
    let mut seen_raw = std::collections::HashSet::new();
    let mut seen_key = std::collections::HashSet::new();
    let mut out = Vec::new();
    for n in nodes {
        let raw = n.get("raw").and_then(|v| v.as_str()).map(|s| s.to_string());
        if let Some(r) = &raw {
            if seen_raw.contains(r) {
                continue;
            }
        }
        let key = format!(
            "{}|{}|{}",
            n.get("type").and_then(|v| v.as_str()).unwrap_or(""),
            n.get("server").and_then(|v| v.as_str()).unwrap_or(""),
            n.get("server_port").and_then(|v| v.as_u64()).unwrap_or(0)
        );
        if seen_key.contains(&key) {
            continue;
        }
        if let Some(r) = raw {
            seen_raw.insert(r);
        }
        seen_key.insert(key);
        out.push(n);
    }
    out
}

// ============================================================
// 订阅
// ============================================================
#[tauri::command]
fn subs_get() -> Vec<Value> {
    load_subs()
}

/// 前端解析订阅文本后传节点过来；url 由前端传入。一次往返完成订阅添加。
#[tauri::command]
async fn subs_add(app: AppHandle, url: String, name: Option<String>, nodes: Vec<Value>) -> Result<Value, String> {
    let node_count = nodes.len();
    if nodes.is_empty() {
        return Err("订阅内容解析不出节点".into());
    }
    log_line(&app, format!("添加订阅 {url}"));
    let host = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("sub")
        .to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let sub = json!({
        "id": id,
        "name": name.unwrap_or(host),
        "url": url,
        "addedAt": now_millis(),
        "updatedAt": now_millis(),
        "nodeCount": node_count,
    });
    let mut subs = load_subs();
    subs.push(sub);
    save_subs(subs)?;
    // 替换该订阅的节点
    let mut existing: Vec<Value> = load_nodes()
        .into_iter()
        .filter(|n| n.get("subId").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    for mut n in nodes {
        n["subId"] = json!(id);
        existing.push(n);
    }
    save_nodes(dedupe(existing))?;
    log_line(&app, format!("订阅添加成功：{node_count} 节点"));
    Ok(json!({ "ok": true, "id": id }))
}

#[tauri::command]
async fn subs_update(app: AppHandle, id: String, nodes: Vec<Value>) -> Result<Value, String> {
    let node_count = nodes.len();
    if nodes.is_empty() {
        return Err("订阅更新后解析不出节点".into());
    }
    let mut subs = load_subs();
    let sub = subs
        .iter_mut()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        .ok_or("订阅不存在")?;
    log_line(&app, format!("更新订阅 {} ...", sub.get("name").and_then(|v| v.as_str()).unwrap_or("")));
    sub["updatedAt"] = json!(now_millis());
    sub["nodeCount"] = json!(node_count);
    save_subs(subs)?;
    let mut existing: Vec<Value> = load_nodes()
        .into_iter()
        .filter(|n| n.get("subId").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    for mut n in nodes {
        n["subId"] = json!(id);
        existing.push(n);
    }
    save_nodes(dedupe(existing))?;
    log_line(&app, format!("订阅 {id} 更新成功：{node_count} 节点"));
    Ok(json!({ "ok": true }))
}

#[tauri::command]
fn subs_remove(id: String) -> Result<usize, String> {
    let subs: Vec<Value> = load_subs()
        .into_iter()
        .filter(|s| s.get("id").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    let n = subs.len();
    save_subs(subs)?;
    let nodes: Vec<Value> = load_nodes()
        .into_iter()
        .filter(|nd| nd.get("subId").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    save_nodes(nodes)?;
    Ok(n)
}

// ============================================================
// 连接
// ============================================================
#[tauri::command]
async fn core_connect(app: AppHandle, selected_tag: Option<String>) -> Result<Value, String> {
    let nodes = load_nodes();
    if nodes.is_empty() {
        return Err("没有可用节点，请先导入".into());
    }
    let settings = load_settings();
    let mode = if settings.route_mode == "global" { "global" } else { "rule" };
    let mixed_port = settings.mixed_port;
    let api_port = settings.clash_api_port;

    // 连接前自愈: 若记录里内核仍在跑（上次异常/panic 残留），先停掉再重连，
    // 否则端口预检会把"自己上次的残留"当成占用源，用户换端口也无效。
    {
        let prev = core_snapshot(&app);
        if prev.running || prev.pid.unwrap_or(0) > 0 {
            log_line(&app, "[core] 检测到上次残留内核，先清理");
            let state = app.state::<KernelState>();
            let km = state.0.lock().await;
            let _ = km.stop().await;
        }
    }

    // 端口占用预检（区分"被自己占用"与"被其他程序占用"）
    if port_busy(mixed_port) {
        return Err(format!(
            "端口 {mixed_port} 被占用（可能已有其他代理/内核在跑）。可在设置里换端口，或先关闭占用该端口的程序"
        ));
    }
    if port_busy(api_port) {
        return Err(format!(
            "端口 {api_port}（clash api）被占用。可在设置里换端口，或先关闭占用该端口的程序"
        ));
    }

    // Rust 侧生成 config（configbuilder.rs 纯函数）
    let cfg = configbuilder::build_config(&nodes, &selected_tag, mixed_port, api_port, mode, &settings.direct_list)?;
    let exe = kernel_exe();
    if !exe.exists() {
        return Err(format!("内核不存在: {}", exe.display()));
    }
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cfg_path = dir.join("config.json");
    let cfg_bytes = serde_json::to_vec_pretty(&cfg).map_err(|e| format!("config 序列化失败: {e}"))?;
    let cfg_json = serde_json::to_string(&cfg).map_err(|e| format!("最终 config 序列化失败: {e}"))?;
    diag(&format!("Final Config: {cfg_json}"));
    log_line(&app, format!("[core] Final Config: {cfg_json}"));
    // 原子写（tmp → 删旧 → rename），避免崩溃留半截 config
    let tmp_path = dir.join("config.json.tmp");
    std::fs::write(&tmp_path, &cfg_bytes).map_err(|e| format!("config 写入失败: {e}"))?;
    let _ = std::fs::remove_file(&cfg_path);
    std::fs::rename(&tmp_path, &cfg_path).map_err(|e| format!("config 替换失败: {e}"))?;
    log_line(&app, format!("[core] config 写入 {}", cfg_path.display()));

    // check（失败时不产生进程，无需清理）
    kernel::check_config(&exe, &cfg_path)?;
    log_line(&app, "[core] sing-box check 通过");

    // spawn（日志经 mpsc 通道 → Tauri event 转发给前端）
    let start_res = {
        let state = app.state::<KernelState>();
        let km = state.0.lock().await;
        // 每次启动重建日志通道，订阅转发到 WebView
        let mut rx = km.take_log_rx();
        let app2 = app.clone();
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if let Some(win) = app2.get_webview_window("main") {
                    let _ = win.emit("log", format!("[{}] [core] {}", now_hms(), line));
                }
            }
        });
        km.start(&exe, &cfg_path, &dir).await
    };
    if let Err(e) = start_res {
        log_line(&app, format!("[core] 内核启动失败: {e}"));
        broadcast_state(&app);
        return Err(e);
    }
    log_line(&app, format!("[core] 内核已启动（mixed :{mixed_port} / clash api :{api_port} / 模式 {}）",
        if mode == "global" { "全局代理" } else { "规则分流" }));

    // clash api 就绪等待（spawn_blocking，不阻塞 async runtime）
    let ready = match tokio::task::spawn_blocking(move || {
        kernel::wait_api_ready(api_port, std::time::Duration::from_secs(10))
    })
    .await
    {
        Ok(r) => r,
        Err(e) => Err(format!("就绪探测线程失败: {e}")),
    };

    match ready {
        Ok(()) => {
            log_line(&app, "[core] clash api 就绪");
            // 自动系统代理 —— 失败不阻塞连接
            let auto = load_settings().auto_system_proxy;
            if auto {
                match sysproxy::enable(mixed_port) {
                    Ok(r) => log_line(&app, format!("系统代理已开启 → {}（via {}）", r.server, r.via)),
                    Err(e) => {
                        log_line(&app, format!("[系统代理] 自动开启失败: {e}"));
                        log_line(&app, format!("[系统代理] 请手动设置：Windows 设置 → 网络和 Internet → 代理 → 手动设置代理 → 127.0.0.1:{mixed_port}"));
                    }
                }
            }
            broadcast_state(&app);
            Ok(json!(core_snapshot(&app)))
        }
        Err(e) => {
            // 启动失败 → 清理（停内核 + 撤销可能已写入的系统代理）
            log_line(&app, format!("[core] clash api 未就绪: {e}"));
            {
                let state = app.state::<KernelState>();
                let km = state.0.lock().await;
                // 停止内核（内核都没起来时 stop 内部直接置位返回）
                let _ = km.stop().await;
            }
            let _ = sysproxy::disable_restore();
            broadcast_state(&app);
            Err(format!("内核启动失败: {e}"))
        }
    }
}

#[tauri::command]
async fn core_disconnect(app: AppHandle) -> Result<Value, String> {
    // 先停内核（释放端口），再恢复系统代理 —— 顺序反了会让流量短暂指向已关闭的端口
    {
        let state = app.state::<KernelState>();
        let km = state.0.lock().await;
        km.stop().await.map_err(|e| e.to_string())?;
    }
    if let Err(e) = sysproxy::disable_restore() {
        log_line(&app, format!("系统代理恢复失败: {e}"));
    }
    log_line(&app, "已断开，系统代理已恢复");
    broadcast_state(&app);
    Ok(json!(core_snapshot(&app)))
}

#[tauri::command]
fn core_state(app: AppHandle) -> Value {
    json!({
        "core": core_snapshot(&app),
        "proxy": sysproxy::status(),
    })
}

#[tauri::command]
async fn core_delay(app: AppHandle, tag: String) -> Result<Option<u32>, String> {
    let api_port = load_settings().clash_api_port;
    let d = match clashapi::test_delay(&tag, api_port).await {
        Ok(delay) => delay,
        Err(e) => {
            log_line(&app, format!("延迟 {tag} 测试失败: {e}"));
            if e.contains("404") {
                return Err("节点列表已更新，请断开重连后生效".to_string());
            }
            return Ok(None);
        }
    };
    log_line(
        &app,
        format!("延迟 {tag}: {}", match d { None => "超时".to_string(), Some(v) => format!("{v}ms") }),
    );
    Ok(d)
}

// ============================================================
// 系统代理
// ============================================================
#[tauri::command]
fn sysproxy_set(app: AppHandle, on: bool) -> Result<Value, String> {
    let port = load_settings().mixed_port;
    if on {
        let r = sysproxy::enable(port)?;
        log_line(&app, format!("系统代理已开启 → {}（via {}）", r.server, r.via));
    } else {
        sysproxy::disable_restore().map_err(|e| e.to_string())?;
        log_line(&app, "系统代理已关闭");
    }
    broadcast_state(&app);
    Ok(json!(sysproxy::status()))
}

#[tauri::command]
fn sysproxy_get() -> Value {
    json!(sysproxy::status())
}

// ============================================================
// 设置
// ============================================================
#[tauri::command]
fn settings_get() -> Settings {
    load_settings()
}

#[tauri::command]
fn settings_save(s: Settings) -> Result<Settings, String> {
    if !["rule", "global"].contains(&s.route_mode.as_str()) {
        return Err(format!("未知分流模式: {}", s.route_mode));
    }
    if s.mixed_port == 0 || s.clash_api_port == 0 {
        return Err("端口不能为 0".into());
    }
    if s.mixed_port == s.clash_api_port {
        return Err("本地端口与 API 端口不能相同".into());
    }
    store::save_settings(&s)?;
    Ok(s)
}

// ============================================================
// 诊断
// ============================================================
#[tauri::command]
fn diag_paths() -> Value {
    json!({
        "dataDir": data_dir().to_string_lossy(),
        "kernelExe": kernel_exe().to_string_lossy(),
        "kernelExists": kernel_exe().exists(),
    })
}

/// 前端 bridge 用来拉订阅文本（带 UA + 超时；解析仍在前端 parsers.js 做）
/// 上限 10MB：畸形/恶意订阅响应不会撑爆内存
#[tauri::command]
async fn fetch_text(url: String) -> Result<String, String> {
    const MAX_BYTES: usize = 10 * 1024 * 1024;
    let r = reqwest::Client::builder()
        .user_agent("clash-verge/1.7.7")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("订阅拉取失败: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("订阅拉取失败 HTTP {}", r.status()));
    }
    let mut bytes = Vec::new();
    let mut r = r;
    while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("订阅内容过大（>10MB）".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|e| format!("订阅内容编码异常: {e}"))
}

/// 启动诊断日志（exe 旁 data/main-diag.log）—— 桌面排障用（GUI 子系统无 stderr）
/// 轮转: 超 256KB 时把旧日志转 .old，防无限增长
pub fn diag(msg: &str) {
    use std::io::Write;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let _ = std::fs::create_dir_all(exe_dir.join("data"));
    let log = exe_dir.join("data").join("main-diag.log");
    if log.metadata().map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&log, exe_dir.join("data").join("main-diag.log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// Pin WebView2 to the same local proxy as the sing-box mixed inbound.
/// WebView2 creates its browser process before the user can enable the
/// system proxy, so relying only on the Windows Internet Settings registry
/// leaves the WebView on the proxy snapshot from application startup.
fn configure_webview_proxy() {
    let port = load_settings().mixed_port;
    let args = format!(
        "--proxy-server=http://127.0.0.1:{port} \
         --proxy-bypass-list=ipc.localhost;localhost;127.0.0.1;[::1]"
    );
    let current = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
    if current.contains("--proxy-server=") {
        diag("launch: keeping existing WEBVIEW2 proxy arguments");
        return;
    }
    let combined = if current.trim().is_empty() {
        args
    } else {
        format!("{current} {args}")
    };
    std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", &combined);
    diag(&format!("launch: WebView2 proxy configured at 127.0.0.1:{port}"));
}

// ============================================================
// 入口
// ============================================================
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    diag("run: entry (startup cleanup begins)");
    // 启动残留清理（孤儿 sing-box + 系统代理残留）
    let orphan = kernel::cleanup_orphans();
    diag(&format!("run: orphan cleanup done ({orphan})"));
    let settings = load_settings();
    let cleaned_stale = sysproxy::cleanup_stale(settings.mixed_port);
    diag(&format!("run: stale proxy cleanup done ({cleaned_stale})"));
    let boot_note = (orphan, cleaned_stale);

    // 内核 manager 与状态句柄分离: manager 需 async 锁，状态可无锁同步读
    let (km, status) = kernel::KernelManager::new();
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // 单实例锁: 二开时聚焦已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .manage(KernelState(Mutex::new(km)))
        .manage(KernelStatusHandle(status))
        .invoke_handler(tauri::generate_handler![
            nodes_get, nodes_add, nodes_remove,
            subs_get, subs_add, subs_update, subs_remove,
            core_connect, core_disconnect, core_state, core_delay,
            sysproxy_set, sysproxy_get,
            settings_get, settings_save,
            diag_paths, fetch_text,
        ])
        .on_window_event(|window, event| {
            // 窗口销毁时同步清理底层资源（注册表恢复 + 停内核）
            // 注意: 这里绝不能 blocking_lock tokio Mutex —— 不在 runtime 上下文但拿不到锁会死锁，
            //      且若恰好处于 runtime 线程会 panic。用 stop_blocking 前的无锁状态判断。
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let app = window.app_handle().clone();
                let _ = sysproxy::disable_restore();
                let running = core_snapshot(&app).running;
                if running {
                    // 走 KernelState 的 std 侧清理: spawn 一个阻塞任务杀进程树
                    if let Some(state) = app.try_state::<KernelState>() {
                        if let Ok(km) = state.0.try_lock() {
                            let _ = km.stop_blocking();
                        } else {
                            // 锁被占（正在 connect/disconnect）→ 直接按 pid 杀进程树兜底
                            if let Some(pid) = core_snapshot(&app).pid {
                                if pid > 0 {
                                    let _ = std::process::Command::new("taskkill")
                                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                                        .creation_flags(0x08000000)
                                        .status();
                                }
                            }
                        }
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    diag("run: tauri app built ok（窗口已创建，进入事件循环）");
    built.run(move |app, event| {
        // 首窗创建后把启动清理结果打给日志面板
        if let tauri::RunEvent::Ready = event {
            diag("run: RunEvent::Ready（事件循环已就绪，窗口应已创建）");
            if let Some(win) = app.get_webview_window("main") {
                let (orphan, stale) = boot_note;
                if orphan > 0 {
                    let _ = win.emit("log", format!("[{}] 启动时清理了 {orphan} 个残留 sing-box 进程", now_hms()));
                }
                if stale {
                    let _ = win.emit("log", format!("[{}] 检测到系统代理残留（上次异常退出），已自动清除", now_hms()));
                }
            }
        }
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let _ = sysproxy::disable_restore();
            // 退出钩子在主线程，绝不能 blocking_lock tokio Mutex → 按 pid 直接杀进程树
            if let Some(pid) = core_snapshot(app).pid {
                if pid > 0 {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                        .creation_flags(0x08000000)
                        .status();
                }
            }
        }
    });
}

fn main() {
    configure_webview_proxy();
    // WebView2 用户数据目录重定向到 exe 旁，便携版数据不落系统盘。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let wv = dir.join("data").join("webview");
            if std::fs::create_dir_all(&wv).is_ok() {
                std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &wv);
            }
        }
    }
    diag("launch: main entered");
    // 兜底 panic 钩子 —— panic 消息也写进诊断日志
    std::panic::set_hook(Box::new(move |info| {
        diag(&format!("PANIC: {info}"));
    }));
    diag("launch: calling run()");
    run();
    diag("launch: run() returned");
}
