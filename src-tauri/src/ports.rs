//! 本地端口占用检测（不依赖 tauri，集成测试可直接 include）
//!
//! 设计约定（与 Clash for Windows 对齐）：
//!   - 端口只取设置里的值，**绝不自动顺延**；
//!   - 被占用 → 启动失败 + 明确报错（谁占着 / 去哪儿改），而不是偷偷换端口；
//!   - 占用者是"尽力而为"的信息，查不到不影响判定。

/// 端口是否已被占用。
/// SO_REUSEADDR 语义: Windows 上 TcpListener::bind 对 TIME_WAIT 连接通常能成功，
/// 但对真正被 listen 的端口会失败 —— 正是我们要的判定。
pub fn port_busy(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// 查出占用指定端口的进程（netstat 找 LISTENING 的 PID → tasklist 换进程名）。
/// 纯诊断用途：任何一步失败都退化成 PID / None。
pub fn port_owner(port: u16) -> Option<String> {
    let out = std::process::Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let suffix = format!(":{port}");
    let pid = text
        .lines()
        .filter(|l| l.contains("LISTENING"))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            if cols.len() < 5 || !cols[1].ends_with(&suffix) {
                return None;
            }
            Some(cols[4].to_string())
        })
        .next()?;
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout);
    let name = t
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_string();
    // tasklist 无匹配时会输出本地化的"信息: 没有运行的任务..."，直接丢弃
    if name.is_empty()
        || name.contains("信息")
        || name.contains("INFO")
        || name.contains("没有运行")
        || name.contains("No tasks")
    {
        return Some(format!("PID {pid}"));
    }
    Some(format!("{name}，PID {pid}"))
}

/// 端口冲突检查（纯函数，不写日志）。返回可直接展示给用户的错误文案。
pub fn port_conflict(mixed_port: u16, api_port: u16) -> Option<String> {
    let owner = |p: u16| match port_owner(p) {
        Some(o) => format!("（占用者：{o}）"),
        None => String::new(),
    };
    if mixed_port == api_port {
        return Some(format!(
            "本地端口与 API 端口不能相同（都是 {mixed_port}），请到 设置 → 端口 换成两个不同端口"
        ));
    }
    if port_busy(mixed_port) {
        return Some(format!(
            "本地端口 {mixed_port} 已被其它程序占用{}，请关闭该程序，或到 设置 → 端口 换成空闲端口",
            owner(mixed_port)
        ));
    }
    if port_busy(api_port) {
        return Some(format!(
            "API 端口 {api_port} 已被其它程序占用{}，请关闭该程序，或到 设置 → 端口 换成空闲端口",
            owner(api_port)
        ));
    }
    None
}
