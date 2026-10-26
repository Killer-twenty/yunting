#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 云亭 Tauri 版主入口
//! 架构:
//!   ui/                —— 复用旧版原生 JS 前端 + 纯函数模块(parsers/configBuilder)
//!   src-tauri/src/     —— Rust: paths/store/kernel/sysproxy/clashapi/configbuilder
//! 数据: %APPDATA%\yt-tool（与 Electron 版数据隔离）；不可写时兜底 exe 旁 data/
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

use paths::{data_dir, kernel_exe};
use store::{load_nodes, load_settings, load_subs, save_nodes, save_subs, Settings};

/// 内核进程句柄（tokio Mutex: async command 内安全持锁）
struct KernelState(pub Mutex<kernel::KernelManager>);

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

fn broadcast_state(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let state = app.state::<KernelState>();
        let core = state.0.blocking_lock().get_state();
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
    save_nodes(merged.clone());
    log_line(&app, format!("导入 {added} 个节点（识别 {parsed_count}，去重后新增 {added}）"));
    Ok(json!({ "added": added, "total": merged.len() }))
}

#[tauri::command]
fn nodes_remove(tag: String) -> usize {
    let nodes = load_nodes();
    let next: Vec<_> = nodes
        .into_iter()
        .filter(|n| n.get("tag").and_then(|v| v.as_str()) != Some(tag.as_str()))
        .collect();
    let n = next.len();
    save_nodes(next);
    n
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
    save_subs(subs);
    // 替换该订阅的节点
    let mut existing: Vec<Value> = load_nodes()
        .into_iter()
        .filter(|n| n.get("subId").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    for mut n in nodes {
        n["subId"] = json!(id);
        existing.push(n);
    }
    save_nodes(dedupe(existing));
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
    save_subs(subs);
    let mut existing: Vec<Value> = load_nodes()
        .into_iter()
        .filter(|n| n.get("subId").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    for mut n in nodes {
        n["subId"] = json!(id);
        existing.push(n);
    }
    save_nodes(dedupe(existing));
    log_line(&app, format!("订阅 {id} 更新成功：{node_count} 节点"));
    Ok(json!({ "ok": true }))
}

#[tauri::command]
fn subs_remove(id: String) -> usize {
    let subs: Vec<Value> = load_subs()
        .into_iter()
        .filter(|s| s.get("id").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    let n = subs.len();
    save_subs(subs);
    let nodes: Vec<Value> = load_nodes()
        .into_iter()
        .filter(|nd| nd.get("subId").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    save_nodes(nodes);
    n
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

    // 端口占用预检
    if port_busy(mixed_port) {
        return Err(format!("端口 {mixed_port} 被占用（可能已有内核或其他代理在跑），请在设置中改端口"));
    }
    if port_busy(api_port) {
        return Err(format!("端口 {api_port}（clash api）被占用，请在设置中改端口"));
    }

    // Rust 侧生成 config（configbuilder.rs 纯函数）
    let cfg = configbuilder::build_config(&nodes, &selected_tag, mixed_port, api_port, mode)?;
    let exe = kernel_exe();
    if !exe.exists() {
        return Err(format!("内核不存在: {}", exe.display()));
    }
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap())
        .map_err(|e| e.to_string())?;
    log_line(&app, format!("[core] config 写入 {}", cfg_path.display()));

    // check
    kernel::check_config(&exe, &cfg_path)?;
    log_line(&app, "[core] sing-box check 通过");

    // spawn（日志经 mpsc 通道 → Tauri event 转发给前端）
    {
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
        km.start(&exe, &cfg_path, &dir).await?;
    }
    log_line(&app, format!("[core] 内核已启动（mixed :{mixed_port} / clash api :{api_port} / 模式 {}）",
        if mode == "global" { "全局代理" } else { "规则分流" }));

    // clash api 就绪等待（spawn_blocking，不阻塞 async runtime）
    let ready = tokio::task::spawn_blocking(move || kernel::wait_api_ready(api_port, std::time::Duration::from_secs(10)))
        .await
        .map_err(|e| e.to_string())?;

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
            let state = app.state::<KernelState>();
            let km = state.0.blocking_lock();
            Ok(json!(km.get_state()))
        }
        Err(e) => {
            // 启动失败 → 清理（停内核）
            log_line(&app, format!("[core] clash api 未就绪: {e}"));
            let state = app.state::<KernelState>();
            let km = state.0.lock().await;
            let _ = km.stop();
            drop(km);
            Err(format!("内核启动失败: {e}"))
        }
    }
}

#[tauri::command]
async fn core_disconnect(app: AppHandle) -> Result<Value, String> {
    if let Err(e) = sysproxy::disable_restore() {
        log_line(&app, format!("系统代理恢复失败: {e}"));
    }
    {
        let state = app.state::<KernelState>();
        let km = state.0.lock().await;
        km.stop().await.map_err(|e| e.to_string())?;
    }
    log_line(&app, "已断开，系统代理已恢复");
    broadcast_state(&app);
    let state = app.state::<KernelState>();
    let km = state.0.blocking_lock();
    Ok(json!(km.get_state()))
}

#[tauri::command]
fn core_state(app: AppHandle) -> Value {
    let state = app.state::<KernelState>();
    let km = state.0.blocking_lock();
    json!({
        "core": km.get_state(),
        "proxy": sysproxy::status(),
    })
}

#[tauri::command]
async fn core_delay(app: AppHandle, tag: String) -> Result<Option<u32>, String> {
    let api_port = load_settings().clash_api_port;
    let d = clashapi::test_delay(&tag, api_port).await;
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
    store::save_settings(&s);
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
#[tauri::command]
async fn fetch_text(url: String) -> Result<String, String> {
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
    r.text().await.map_err(|e| e.to_string())
}

// ============================================================
// 入口
// ============================================================
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 启动残留清理（孤儿 sing-box + 系统代理残留）
    let orphan = kernel::cleanup_orphans();
    let settings = load_settings();
    let cleaned_stale = sysproxy::cleanup_stale(settings.mixed_port);
    let boot_note = (orphan, cleaned_stale);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // 单实例锁: 二开时聚焦已有窗口（替代旧版 requestSingleInstanceLock）
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .manage(KernelState(Mutex::new(kernel::KernelManager::new())))
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
            if matches!(event, tauri::WindowEvent::Destroyed { .. }) {
                let app = window.app_handle().clone();
                let _ = sysproxy::disable_restore();
                if let Some(state) = app.try_state::<KernelState>() {
                    let km = state.0.blocking_lock();
                    let _ = km.stop_blocking();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app, event| {
            // 首窗创建后把启动清理结果打给日志面板
            if let tauri::RunEvent::Ready = event {
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
                if let Some(state) = app.try_state::<KernelState>() {
                    let km = state.0.blocking_lock();
                    let _ = km.stop_blocking();
                }
            }
        });
}

fn main() {
    run();
}
