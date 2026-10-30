#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! yunting Tauri 版主入口
//! 架构:
//!   ui/                —— 复用旧版原生 JS 前端 + 纯函数模块(parsers/configBuilder)
//!   src-tauri/src/     —— Rust: paths/store/kernel/proxy/clashapi/configbuilder
//! 数据: %APPDATA%\yunting（与 Electron 版数据隔离）；不可写时兜底 exe 旁 data/
//! 内核: 打包态 exe 旁 kernel/；开发态项目 kernel/

mod clashapi;
mod configbuilder;
mod kernel;
mod paths;
mod proxy_util;
mod store;

use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

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
        _y: u16,
        _mo: u16,
        _dow: u16,
        _d: u16,
        h: u16,
        m: u16,
        s: u16,
        _ms: u16,
    }
    extern "system" {
        fn GetLocalTime(p: *mut ST);
    }
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
        let proxy = proxy_util::status_json();
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
    log_line(
        &app,
        format!("导入 {added} 个节点（识别 {parsed_count}，去重后新增 {added}）"),
    );
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

/// 修改节点显示名称（tag）。新名称会同步进 raw 链接的 #fragment，
/// 保证下次订阅更新去重时仍按 type|server|port 命中同一节点。
#[tauri::command]
fn nodes_rename(tag: String, new_tag: String) -> Result<Value, String> {
    let new_tag = new_tag.trim().to_string();
    if new_tag.is_empty() {
        return Err("名称不能为空".into());
    }
    if new_tag.chars().count() > 64 {
        return Err("名称过长（最多 64 字符）".into());
    }
    let nodes = load_nodes();
    if nodes
        .iter()
        .any(|n| n.get("tag").and_then(|v| v.as_str()) != Some(tag.as_str())
            && n.get("tag").and_then(|v| v.as_str()) == Some(new_tag.as_str()))
    {
        return Err(format!("名称「{new_tag}」已被其它节点占用"));
    }
    let mut found = false;
    let next: Vec<Value> = nodes
        .into_iter()
        .map(|mut n| {
            if n.get("tag").and_then(|v| v.as_str()) == Some(tag.as_str()) {
                n["tag"] = json!(new_tag);
                // raw 分享链接保持原样（含原 #name）——改名只动外显，
                // 否则复制出的链接与订阅原始链接不一致，可能连不回对应节点。
                // 无 raw 的节点（clash yaml）用 origTag 记录原始名供反推链接用。
                if n.get("origTag").is_none() {
                    n["origTag"] = json!(tag);
                }
                found = true;
            }
            n
        })
        .collect();
    if !found {
        return Err("节点不存在".into());
    }
    save_nodes(next)?;
    Ok(json!({ "ok": true, "tag": new_tag }))
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
async fn subs_add(
    app: AppHandle,
    url: String,
    name: Option<String>,
    nodes: Vec<Value>,
) -> Result<Value, String> {
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
    log_line(
        &app,
        format!(
            "更新订阅 {} ...",
            sub.get("name").and_then(|v| v.as_str()).unwrap_or("")
        ),
    );
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
    // 幂等保护：内核已在运行且未指定切换节点 → 直接返回状态
    // （软件启动时内核已自启动，避免"一键连接"把健康的内核无谓重启）
    if selected_tag.is_none() {
        let prev = core_snapshot(&app);
        if prev.running {
            // 自启动路径不联动系统代理；用户点「一键连接」时按设置补上
            if load_settings().auto_system_proxy {
                let port = load_settings().mixed_port;
                match proxy_util::enable(&format!("127.0.0.1:{port}")) {
                    Ok(()) => log_line(&app, format!("系统代理已开启 → 127.0.0.1:{port}")),
                    Err(e) => log_line(&app, format!("[系统代理] 自动开启失败: {e}")),
                }
            }
            broadcast_state(&app);
            return Ok(json!(prev));
        }
    }
    let nodes = load_nodes();
    // 无节点不再拦截启动：configbuilder 生成纯直连 config，内核正常运行；
    // 导入节点后重载即可走代理
    let settings = load_settings();
    let mode = if settings.route_mode == "global" {
        "global"
    } else {
        "rule"
    };
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
    let cfg = configbuilder::build_config(
        &nodes,
        &selected_tag,
        mixed_port,
        api_port,
        mode,
        &settings.direct_list,
    )?;
    let exe = kernel_exe();
    if !exe.exists() {
        return Err(format!("内核不存在: {}", exe.display()));
    }
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cfg_path = dir.join("config.json");
    let cfg_bytes =
        serde_json::to_vec_pretty(&cfg).map_err(|e| format!("config 序列化失败: {e}"))?;
    let cfg_json =
        serde_json::to_string(&cfg).map_err(|e| format!("最终 config 序列化失败: {e}"))?;
    diag(&format!("Final Config: {cfg_json}"));
    // 完整 config 只进诊断日志文件；GUI 日志面板仅开发版显示（分发版不刷屏、不泄露节点细节）
    if cfg!(debug_assertions) {
        log_line(&app, format!("[core] Final Config: {cfg_json}"));
    }
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
    log_line(
        &app,
        format!(
            "[core] 内核已启动（mixed :{mixed_port} / clash api :{api_port} / 模式 {}）",
            if mode == "global" {
                "全局代理"
            } else {
                "规则分流"
            }
        ),
    );

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
            let mixed_ready = tokio::task::spawn_blocking(move || {
                kernel::wait_port_ready(mixed_port, std::time::Duration::from_secs(5))
            })
            .await
            .map_err(|e| format!("mixed 端口探测线程失败: {e}"))?;
            mixed_ready?;
            log_line(&app, format!("[core] mixed 代理端口 {mixed_port} 就绪"));
            // 自动系统代理 —— 失败不阻塞连接
            let auto = load_settings().auto_system_proxy;
            if auto {
                match proxy_util::enable(&format!("127.0.0.1:{mixed_port}")) {
                    Ok(()) => log_line(&app, format!("系统代理已开启 → 127.0.0.1:{mixed_port}")),
                    Err(e) => log_line(&app, format!("[系统代理] 自动开启失败: {e}")),
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
            let _ = proxy_util::disable();
            broadcast_state(&app);
            Err(format!("内核启动失败: {e}"))
        }
    }
}

/// 内核启动（自启动与连接共用）：写 config → check → spawn → 就绪等待。
/// 复用 core_connect 的完整流程但跳过系统代理开关联动。
async fn start_kernel(app: &AppHandle) -> Result<(), String> {
    let nodes = load_nodes();
    // 无节点不再拦截启动：configbuilder 生成纯直连 config，内核正常运行；
    // 导入节点后重载即可走代理
    let settings = load_settings();
    let mode = if settings.route_mode == "global" {
        "global"
    } else {
        "rule"
    };
    let mixed_port = settings.mixed_port;
    let api_port = settings.clash_api_port;

    // 自启动路径同样做残留自愈
    {
        let prev = core_snapshot(app);
        if prev.running || prev.pid.unwrap_or(0) > 0 {
            let state = app.state::<KernelState>();
            let km = state.0.lock().await;
            let _ = km.stop().await;
        }
    }
    if port_busy(mixed_port) {
        return Err(format!(
            "端口 {mixed_port} 被占用（可能已有其他代理/内核在跑），内核暂不启动；可在设置里换端口后点「一键连接」重试"
        ));
    }
    if port_busy(api_port) {
        return Err(format!(
            "端口 {api_port}（clash api）被占用，内核暂不启动；可在设置里换端口后点「一键连接」重试"
        ));
    }

    let cfg = configbuilder::build_config(
        &nodes,
        &None,
        mixed_port,
        api_port,
        mode,
        &settings.direct_list,
    )?;
    let exe = kernel_exe();
    if !exe.exists() {
        return Err(format!("内核不存在: {}", exe.display()));
    }
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cfg_path = dir.join("config.json");
    let cfg_bytes =
        serde_json::to_vec_pretty(&cfg).map_err(|e| format!("config 序列化失败: {e}"))?;
    let tmp_path = dir.join("config.json.tmp");
    std::fs::write(&tmp_path, &cfg_bytes).map_err(|e| format!("config 写入失败: {e}"))?;
    let _ = std::fs::remove_file(&cfg_path);
    std::fs::rename(&tmp_path, &cfg_path).map_err(|e| format!("config 替换失败: {e}"))?;
    log_line(app, format!("[core] config 写入 {}", cfg_path.display()));

    kernel::check_config(&exe, &cfg_path)?;
    log_line(app, "[core] sing-box check 通过");

    let start_res = {
        let state = app.state::<KernelState>();
        let km = state.0.lock().await;
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
        log_line(app, format!("[core] 内核启动失败: {e}"));
        broadcast_state(app);
        return Err(e);
    }
    log_line(
        app,
        format!(
            "[core] 内核已启动（mixed :{mixed_port} / clash api :{api_port} / 模式 {}）",
            if mode == "global" {
                "全局代理"
            } else {
                "规则分流"
            }
        ),
    );

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
            log_line(app, "[core] clash api 就绪");
            let mixed_ready = tokio::task::spawn_blocking(move || {
                kernel::wait_port_ready(mixed_port, std::time::Duration::from_secs(5))
            })
            .await
            .map_err(|e| format!("mixed 端口探测线程失败: {e}"))?;
            mixed_ready?;
            log_line(app, format!("[core] mixed 代理端口 {mixed_port} 就绪"));
            broadcast_state(app);
            Ok(())
        }
        Err(e) => {
            log_line(app, format!("[core] clash api 未就绪: {e}"));
            {
                let state = app.state::<KernelState>();
                let km = state.0.lock().await;
                let _ = km.stop().await;
            }
            broadcast_state(app);
            Err(format!("内核启动失败: {e}"))
        }
    }
}

/// 软件启动即拉起内核（不联动系统代理；失败仅记日志不弹窗）
async fn autostart_kernel(app: AppHandle) {
    match start_kernel(&app).await {
        Ok(()) => {
            log_line(&app, "[core] 内核已随软件自启动，点「一键连接」开启系统代理");
        }
        Err(e) => {
            log_line(&app, format!("[core] 内核自启动跳过: {e}（导入节点或调整端口后点「一键连接」即可）"));
        }
    }
}

#[tauri::command]
async fn core_disconnect(app: AppHandle) -> Result<Value, String> {
    // 「断开」= 仅撤销系统代理。内核生命周期跟随软件：
    // 启动时自启（RunEvent::Ready → autostart_kernel），退出时停止（Destroyed / ExitRequested）。
    if proxy_util::enabled_by_us() {
        if let Err(e) = proxy_util::disable() {
            log_line(&app, format!("系统代理恢复失败: {e}"));
        }
    }
    log_line(&app, "已断开系统代理（内核保持运行，退出软件时自动停止）");
    broadcast_state(&app);
    Ok(json!(core_snapshot(&app)))
}

/// 重启内核（节点列表变更后重载配置；与 core_connect 共用启动流程）
#[tauri::command]
async fn core_restart(app: AppHandle, selected_tag: Option<String>) -> Result<Value, String> {
    {
        let state = app.state::<KernelState>();
        let km = state.0.lock().await;
        let _ = km.stop().await;
    }
    core_connect(app, selected_tag).await
}

fn get_proxy_status() -> serde_json::Value {
    proxy_util::status_json()
}

#[tauri::command]
fn core_state(app: AppHandle) -> Value {
    json!({
        "core": core_snapshot(&app),
        "proxy": get_proxy_status(),
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
        format!(
            "延迟 {tag}: {}",
            match d {
                None => "超时".to_string(),
                Some(v) => format!("{v}ms"),
            }
        ),
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
        // 内核未运行时提示（手动开系统代理会指向死端口），但尊重用户选择不拦截
        if !core_snapshot(&app).running {
            log_line(&app, "提示: 内核未运行，系统代理将指向未监听的端口");
        }
        proxy_util::enable(&format!("127.0.0.1:{port}"))?;
        log_line(&app, format!("系统代理已开启 → 127.0.0.1:{port}"));
    } else {
        proxy_util::disable()?;
        log_line(&app, "系统代理已关闭");
    }

    broadcast_state(&app);
    Ok(proxy_util::status_json())
}

#[tauri::command]
fn sysproxy_get() -> Value {
    proxy_util::status_json()
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
    if !["ask", "quit", "tray"].contains(&s.close_behavior.as_str()) {
        return Err(format!("未知关闭行为: {}", s.close_behavior));
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

/// 启动诊断日志（data_dir()/main-diag.log，与业务数据同收束到 exe 旁 data/）
/// —— 桌面排障用（GUI 子系统无 stderr）
/// 轮转: 超 256KB 时把旧日志转 .old，防无限增长
pub fn diag(msg: &str) {
    use std::io::Write;
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let log = dir.join("main-diag.log");
    if log
        .metadata()
        .map(|m| m.len() > 256 * 1024)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&log, dir.join("main-diag.log.old"));
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
///
/// 当前停用：Windows 侧 WebView2 默认跟随用户级 Internet Settings，
/// 启动时强制 pin 代理会在内核未运行时把 WebView 流量指向死端口。
/// 保留实现备查，需要时在 main() 里取消注释调用。
#[allow(dead_code)]
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
    diag(&format!(
        "launch: WebView2 proxy configured at 127.0.0.1:{port}"
    ));
}

// ============================================================
// 托盘
// ============================================================
/// 构建系统托盘：显示/退出 两个菜单项（点击图标 = 显示窗口）
fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().unwrap())
        .tooltip("yunting")
        .menu(&menu)
        .show_menu_on_left_click(false) // 左键=显示窗口，右键=菜单（Windows 惯例）
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
            "quit" => {
                app.exit(0); // 走 ExitRequested 清理链（撤代理 + 杀内核）
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(win) = tray.app_handle().get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// 前端托盘菜单「退出」调用（等价于 app.exit，但给日志面板留一条记录）
#[tauri::command]
fn app_quit(app: AppHandle) {
    log_line(&app, "托盘退出：关闭内核并清理系统代理…");
    app.exit(0);
}

/// 恢复显示主窗口（托盘菜单/对话框按钮用）
#[tauri::command]
fn window_show(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 隐藏主窗口到托盘（关闭行为对话框选"最小化"时用）
#[tauri::command]
fn window_hide(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
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
    let cleaned_stale = proxy_util::cleanup_stale(load_settings().mixed_port);
    diag(&format!("run: stale proxy cleanup done ({cleaned_stale})"));
    let boot_note = (orphan, cleaned_stale);

    // 内核 manager 与状态句柄分离: manager 需 async 锁，状态可无锁同步读
    let (km, status) = kernel::KernelManager::new();
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .manage(KernelState(Mutex::new(km)))
        .manage(KernelStatusHandle(status))
        .setup(|app| {
            diag("run: setup（构建托盘）");
            init_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            nodes_get,
            nodes_add,
            nodes_remove,
            nodes_rename,
            subs_get,
            subs_add,
            subs_update,
            subs_remove,
            core_connect,
            core_disconnect,
            core_restart,
            core_state,
            core_delay,
            sysproxy_set,
            sysproxy_get,
            settings_get,
            settings_save,
            diag_paths,
            fetch_text,
            app_quit,
            window_show,
            window_hide,
        ])
        .on_window_event(|window, event| {
            // 点 × 的拦截：
            //   quit → 放行（走 Destroyed 清理链）
            //   tray → 隐藏到托盘
            //   ask  → 阻止关闭 + 发 "close-asked" 事件，前端弹自定义对话框
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                match load_settings().close_behavior.as_str() {
                    "tray" => {
                        api.prevent_close();
                        let _ = window.hide();
                        diag("close: hidden to tray (close_behavior=tray)");
                    }
                    "ask" => {
                        api.prevent_close();
                        let _ = window.emit("close-asked", ());
                        diag("close: ask dialog emitted");
                    }
                    _ => {} // quit：放行
                }
                return;
            }
            // 窗口销毁时同步清理底层资源（注册表恢复 + 停内核）
            // 注意: 这里绝不能 blocking_lock tokio Mutex —— 不在 runtime 上下文但拿不到锁会死锁，
            //      且若恰好处于 runtime 线程会 panic。用 stop_blocking 前的无锁状态判断。
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let app = window.app_handle().clone();
                if proxy_util::enabled_by_us() {
                    let _ = proxy_util::disable();
                }
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
        // 首窗创建后把启动清理结果打给日志面板，并自启动内核
        if let tauri::RunEvent::Ready = event {
            diag("run: RunEvent::Ready（事件循环已就绪，窗口应已创建）");
            if let Some(win) = app.get_webview_window("main") {
                let (orphan, stale) = boot_note;
                if orphan > 0 {
                    let _ = win.emit(
                        "log",
                        format!("[{}] 启动时清理了 {orphan} 个残留 sing-box 进程", now_hms()),
                    );
                }
                if stale {
                    let _ = win.emit(
                        "log",
                        format!(
                            "[{}] 检测到系统代理残留（上次异常退出），已自动清除",
                            now_hms()
                        ),
                    );
                }
            }
            // 内核随软件启动（tokio runtime 内 spawn，不阻塞事件循环）
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                autostart_kernel(app2).await;
            });
        }
        if let tauri::RunEvent::ExitRequested { .. } = event {
            // 系统代理只在「本应用开启」时撤销，不动其他程序设置的代理
            if proxy_util::enabled_by_us() {
                let _ = proxy_util::disable();
            }
            // 退出钩子在主线程，绝不能 blocking_lock tokio Mutex → 按 pid 直接杀进程树
            if let Some(pid) = core_snapshot(app).pid {
                if pid > 0 {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                        .creation_flags(0x08000000)
                        .status();
                    diag(&format!("run: exit killed kernel pid={pid}"));
                }
            }
        }
    });
}

fn main() {
    diag("launch: main entered");
    // 兜底 panic 钩子 —— panic 消息也写进诊断日志
    std::panic::set_hook(Box::new(move |info| {
        diag(&format!("PANIC: {info}"));
    }));
    diag("launch: calling run()");
    run();
    diag("launch: run() returned");
}
