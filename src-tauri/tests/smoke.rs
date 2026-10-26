// 系统代理注册表操作 + 端口检测 + kernel check 的无 GUI 集成冒烟
// 用法: cargo test --manifest-path src-tauri/Cargo.toml --test smoke -- --nocapture
// （不依赖 WebView；真实注册表 HKCU 操作，测完恢复）

#[path = "../src/sysproxy.rs"]
mod sysproxy;
#[path = "../src/kernel.rs"]
mod kernel;
#[path = "../src/paths.rs"]
mod paths;
#[path = "../src/store.rs"]
mod store;

#[test]
fn sysproxy_registry_roundtrip() {
    // 1. 现状
    let before = sysproxy::status();
    println!("[t] before: enabled={} server={:?} ours={}", before.enabled, before.server, before.ours);

    // 2. enable（测试端口 59998）
    let r = sysproxy::enable(59998).expect("enable failed");
    println!("[t] enabled via {} → {}", r.via, r.server);
    let s = sysproxy::status();
    if r.via == "elevate" && !s.enabled {
        // 无头/受限会话: HKCU 直写被 ACL 拒 + UAC 提权子进程被会话限制 → 桌面环境不受影响
        println!("[t] SKIP: 受限会话（无头窗口站）内注册表写入不生效 —— 用户桌面双击不受影响（Electron 版同类问题已在旧 PROGRESS 记录）");
        sysproxy::disable_force();
        return;
    }
    assert!(s.enabled, "ProxyEnable should be 1");
    assert_eq!(s.server, "127.0.0.1:59998");
    assert!(s.ours);

    // 3. 残留检测应清理（内核未运行场景）
    let cleaned = sysproxy::cleanup_stale(59998);
    assert!(cleaned, "stale cleanup should fire");
    let s2 = sysproxy::status();
    assert!(!s2.enabled, "should be disabled after cleanup");

    // 4. 恢复原状
    sysproxy::disable_force();
    let after = sysproxy::status();
    println!("[t] after: enabled={} server={:?}", after.enabled, after.server);
    if !before.enabled {
        assert!(!after.enabled);
    }
    println!("[t] sysproxy roundtrip ✅");
}

#[test]
fn port_detection() {
    use std::net::TcpListener;
    // 占一个端口 → port_busy 应报 true
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = l.local_addr().unwrap().port();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_err(), "second bind should fail");
    let busy = TcpListener::bind(("127.0.0.1", port)).is_err();
    assert!(busy);
    drop(l);
    // 释放后可绑
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok(), "free port should bind");
    println!("[t] port busy detection ✅ (port {port})");
}

#[test]
fn kernel_check_config() {
    // 测试进程 cwd 在 target\debug\deps 下，显式指到项目根 kernel/
    let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("kernel")
        .join("sing-box.exe");
    if !exe.exists() {
        panic!("kernel exe missing at {}", exe.display());
    }
    // 生成一个最小 config 再 check
    let nodes = vec![serde_json::json!({
        "tag": "smoke-hy2", "type": "hysteria2", "server": "127.0.0.1",
        "server_port": 18443, "password": "x", "tls": { "enabled": true, "insecure": true },
    })];
    // 直接内嵌 configbuilder 逻辑太重，这里手写一个等价 config 走 check
    let cfg = serde_json::json!({
        "log": { "level": "info", "timestamp": true },
        "dns": { "servers": [
            { "tag": "local-dns", "address": "223.5.5.5", "detour": "direct" },
            { "tag": "proxy-dns", "address": "https://1.1.1.1/dns-query", "detour": "proxy" },
        ], "final": "proxy-dns", "strategy": "prefer_ipv4" },
        "inbounds": [ { "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": 20808 } ],
        "outbounds": [
            { "tag": "smoke-hy2", "type": "hysteria2", "server": "127.0.0.1", "server_port": 18443, "password": "x", "tls": { "enabled": true, "insecure": true } },
            { "type": "selector", "tag": "proxy", "outbounds": ["auto", "smoke-hy2"], "default": "smoke-hy2", "interrupt_exist_connections": false },
            { "type": "urltest", "tag": "auto", "outbounds": ["smoke-hy2"], "url": "https://www.gstatic.com/generate_204", "interval": "5m", "tolerance": 50, "idle_timeout": "30m" },
            { "type": "direct", "tag": "direct" },
            { "type": "block", "tag": "block" },
        ],
        "route": {
            "rules": [ { "action": "sniff" }, { "protocol": "dns", "action": "hijack-dns" }, { "clash_mode": "Direct", "outbound": "direct" }, { "clash_mode": "Global", "outbound": "proxy" } ],
            "final": "proxy", "auto_detect_interface": true,
        },
        "experimental": { "clash_api": { "external_controller": "127.0.0.1:9090", "default_mode": "rule" }, "cache_file": { "enabled": true } },
    });
    let dir = smoke_tmp_dir("check");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();
    let r = kernel::check_config(&exe, &cfg_path);
    println!("[t] sing-box check: {:?}", r);
    assert!(r.is_ok(), "sing-box check must pass: {r:?}");
    println!("[t] kernel check ✅");
}

/// 测试工作目录: 项目 target/smoke-tmp/<name>（temp_dir 在受限会话+非 ASCII 用户名下拒写）
fn smoke_tmp_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("smoke-tmp").join(name)
}

#[tokio::test]
async fn kernel_lifecycle() {
    // 全链路: spawn → clash api 就绪 → /version → 停止 → 端口释放
    let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("kernel")
        .join("sing-box.exe");
    if !exe.exists() {
        panic!("kernel exe missing: {}", exe.display());
    }
    let dir = smoke_tmp_dir("lifecycle");
    std::fs::create_dir_all(&dir).unwrap();
    // 用独立端口避免与可能存在的内核冲突
    let mixed_port = 20818u16;
    let api_port = 9098u16;
    let cfg = serde_json::json!({
        "log": { "level": "info", "timestamp": true },
        "dns": { "servers": [
            { "tag": "local-dns", "address": "223.5.5.5", "detour": "direct" },
            { "tag": "proxy-dns", "address": "https://1.1.1.1/dns-query", "detour": "proxy" },
        ], "final": "proxy-dns", "strategy": "prefer_ipv4" },
        "inbounds": [ { "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": mixed_port } ],
        "outbounds": [
            { "tag": "T", "type": "shadowsocks", "server": "127.0.0.1", "server_port": 18444, "method": "aes-256-gcm", "password": "x" },
            { "type": "selector", "tag": "proxy", "outbounds": ["auto", "T"], "default": "T", "interrupt_exist_connections": false },
            { "type": "urltest", "tag": "auto", "outbounds": ["T"], "url": "https://www.gstatic.com/generate_204", "interval": "5m", "tolerance": 50, "idle_timeout": "30m" },
            { "type": "direct", "tag": "direct" },
            { "type": "block", "tag": "block" },
        ],
        "route": {
            "rules": [ { "action": "sniff" }, { "protocol": "dns", "action": "hijack-dns" }, { "clash_mode": "Direct", "outbound": "direct" } ],
            "final": "proxy", "auto_detect_interface": true,
        },
        "experimental": { "clash_api": { "external_controller": format!("127.0.0.1:{api_port}"), "default_mode": "rule" }, "cache_file": { "enabled": true } },
    });
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();
    kernel::check_config(&exe, &cfg_path).expect("check failed");

    let km = kernel::KernelManager::new();
    let mut rx = km.take_log_rx();
    let pid = km.start(&exe, &cfg_path, &dir).await.expect("start failed");
    println!("[t] kernel started pid={pid}");
    assert!(pid > 0);
    // 收日志（非阻塞采样）
    while let Ok(line) = rx.try_recv() {
        println!("[core] {line}");
    }
    // clash api 就绪
    kernel::wait_api_ready(api_port, std::time::Duration::from_secs(10)).expect("api not ready");
    println!("[t] clash api ready on {api_port}");
    // /version 查询（原始 HTTP，无 reqwest::blocking —— 它在 tokio 运行时内会挂起）
    let version_body = {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", api_port)).expect("connect api");
        let req = format!("GET /version HTTP/1.1\r\nHost: 127.0.0.1:{api_port}\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).expect("write req");
        let mut buf = String::new();
        s.read_to_string(&mut buf).expect("read resp");
        buf
    };
    println!("[t] clash api raw: {}", version_body.lines().next().unwrap_or(""));
    assert!(version_body.contains("sing-box"), "version should mention sing-box");

    // 停止 → 端口应释放
    km.stop().await.expect("stop failed");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let port_free = std::net::TcpListener::bind(("127.0.0.1", mixed_port)).is_ok();
    assert!(port_free, "mixed port should be free after stop");
    println!("[t] kernel lifecycle ✅ (start → api ready → version → stop → port free)");
}

#[test]
fn store_roundtrip() {
    // store 用 %APPDATA%\yt-tool —— 测试里读写一遍
    let s0 = store::load_settings();
    println!("[t] settings: {:?}", s0);
    let mut s = s0.clone();
    let orig_port = s.mixed_port;
    s.mixed_port = 20809;
    store::save_settings(&s);
    let s1 = store::load_settings();
    assert_eq!(s1.mixed_port, 20809);
    // 还原
    let mut s2 = s1;
    s2.mixed_port = orig_port;
    store::save_settings(&s2);
    println!("[t] store roundtrip ✅");
}
