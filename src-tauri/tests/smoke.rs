// 端口检测 + kernel check 的无 GUI 集成冒烟
// 用法: cargo test --manifest-path src-tauri/Cargo.toml --test smoke -- --nocapture
// （不依赖 WebView）

#[path = "../src/configbuilder.rs"]
mod configbuilder;
#[path = "../src/kernel.rs"]
mod kernel;
#[path = "../src/paths.rs"]
mod paths;
#[path = "../src/store.rs"]
mod store;

#[test]
fn port_detection() {
    use std::net::TcpListener;
    // 占一个端口 → port_busy 应报 true
    let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = l.local_addr().unwrap().port();
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_err(),
        "second bind should fail"
    );
    let busy = TcpListener::bind(("127.0.0.1", port)).is_err();
    assert!(busy);
    drop(l);
    // 释放后可绑
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "free port should bind"
    );
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

#[test]
fn configbuilder_rejects_out_of_range_port() {
    // 超 uint16 范围的节点端口应被拒绝（sing-box 解码 config 时直接 FATAL）
    let bad = serde_json::json!({ "tag": "BAD", "type": "ss", "server": "1.2.3.4", "server_port": 70000, "method": "aes-256-gcm", "password": "x" });
    assert!(
        configbuilder::node_to_outbound(&bad).is_none(),
        "port 70000 must be rejected"
    );
    let good = serde_json::json!({ "tag": "OK", "type": "ss", "server": "1.2.3.4", "server_port": 65535, "method": "aes-256-gcm", "password": "x" });
    assert!(
        configbuilder::node_to_outbound(&good).is_some(),
        "port 65535 must be accepted"
    );
    let zero = serde_json::json!({ "tag": "Z", "type": "ss", "server": "1.2.3.4", "server_port": 0, "method": "aes-256-gcm", "password": "x" });
    assert!(
        configbuilder::node_to_outbound(&zero).is_none(),
        "port 0 must be rejected"
    );
    println!("[t] configbuilder port range ✅");
}

#[test]
fn configbuilder_preserves_reality_strings() {
    let node = serde_json::json!({
        "tag": "REALITY",
        "type": "vless",
        "server": "1.2.3.4",
        "server_port": 443,
        "uuid": "uuid",
        "flow": "xtls-rprx-vision",
        "tls": {
            "enabled": true,
            "server_name": "www.cloudflare.com",
            "reality": {
                "enabled": true,
                "public_key": "DJk-alzV7TbEcm_txr3TytBs5Tz2m6LID8dHAyTk81c",
                "short_id": "08480e7f"
            }
        }
    });
    let outbound = configbuilder::node_to_outbound(&node).expect("valid Reality node");
    assert_eq!(outbound["tls"]["reality"]["short_id"], "08480e7f");
    assert_eq!(
        outbound["tls"]["reality"]["public_key"],
        "DJk-alzV7TbEcm_txr3TytBs5Tz2m6LID8dHAyTk81c"
    );
    assert!(outbound["tls"]["reality"]["short_id"].is_string());
    assert!(outbound["tls"]["reality"]["public_key"].is_string());
}

#[test]
fn data_dir_fallback_when_appdata_unwritable() {
    // 受限会话（DSH 无头）下 %APPDATA%\yunting 目录存在但拒写（os error 5）
    // → 解析应回退 exe 旁 data/，否则所有保存静默失败（用户报导入红框）
    //
    // 纯函数验证: 把 APPDATA 模拟成不可用路径，确保 resolve_data_dir_with 回退到 exe 旁。
    let bogus = smoke_tmp_dir("appdata-bogus");
    let _ = std::fs::remove_dir_all(&bogus);
    std::fs::create_dir_all(&bogus).unwrap();
    // yunting 是个文件 → create_dir_all 失败 → 必须回退
    std::fs::write(bogus.join("yunting"), b"file not dir").unwrap();

    // 调用纯逻辑版本，显式注入不可用的 APPDATA
    let exe_dir = paths::exe_dir();
    let d = paths::resolve_data_dir_with(
        Some(String::new()),
        Some(bogus.to_string_lossy().to_string()),
        &exe_dir,
    );
    let unusable = bogus.join("yunting");
    assert_ne!(
        d, unusable,
        "must never resolve to the unusable APPDATA\\yunting"
    );
    let probe = d.join(".wprobe");
    assert!(
        std::fs::write(&probe, b"x").is_ok(),
        "resolved data dir must be writable: {}",
        d.display()
    );
    let _ = std::fs::remove_file(&probe);
    let _ = std::fs::remove_dir_all(&bogus);
    println!("[t] data_dir fallback ✅ → {}", d.display());
}

#[test]
fn kernel_status_readable_from_any_context() {
    // 回归: broadcast_state/core_state 曾在 async runtime 内对 tokio Mutex 调 blocking_lock
    // → panic "Cannot block the current thread from within a runtime"，导致连接报错/断开卡死。
    // 状态句柄必须能在 runtime 上下文与非 runtime 上下文都无锁读取。
    let (_km, status) = kernel::KernelManager::new();
    // 非 runtime 上下文
    let s = status.snapshot();
    assert!(!s.running);
    assert_eq!(s.pid, None);
    assert_eq!(s.phase, "stopped");
    // runtime 上下文（这里若内部用 tokio Mutex 且 blocking_lock 就会 panic）
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let s2 = status.snapshot();
        assert!(!s2.running);
        println!("[t] status snapshot in runtime ctx: {:?}", s2);
    });
    println!("[t] kernel status readable from any context ✅");
}

/// 测试工作目录: 项目 target/smoke-tmp/<name>（temp_dir 在受限会话+非 ASCII 用户名下拒写）
fn smoke_tmp_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("smoke-tmp")
        .join(name)
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

    let (km, kstatus) = kernel::KernelManager::new();
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
        let req = format!(
            "GET /version HTTP/1.1\r\nHost: 127.0.0.1:{api_port}\r\nConnection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes()).expect("write req");
        let mut buf = String::new();
        s.read_to_string(&mut buf).expect("read resp");
        buf
    };
    println!(
        "[t] clash api raw: {}",
        version_body.lines().next().unwrap_or("")
    );
    assert!(
        version_body.contains("sing-box"),
        "version should mention sing-box"
    );

    // 停止 → 端口应释放
    km.stop().await.expect("stop failed");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let port_free = std::net::TcpListener::bind(("127.0.0.1", mixed_port)).is_ok();
    assert!(port_free, "mixed port should be free after stop");
    println!("[t] kernel lifecycle ✅ (start → api ready → version → stop → port free)");
}

#[test]
fn store_roundtrip_and_corrupt_recovery() {
    // store 默认用 %APPDATA%\yunting；受限会话下该目录可能拒写 → 测试用 YUNTING_DATA_DIR 隔离
    // （不污染真实用户数据；默认路径策略另由真实环境验证）
    // 注意: paths::data_dir() 用 OnceLock 缓存，本测试必须先设置 YUNTING_DATA_DIR 再首次访问
    let iso = smoke_tmp_dir("store");
    let _ = std::fs::remove_dir_all(&iso);
    std::fs::create_dir_all(&iso).unwrap();
    std::env::set_var("YUNTING_DATA_DIR", &iso);
    let d = paths::data_dir();
    assert!(d.exists() && d.is_dir());
    let probe = d.join("diag-probe.txt");
    assert!(
        std::fs::write(&probe, b"x").is_ok(),
        "isolated data dir must be writable"
    );
    let _ = std::fs::remove_file(&probe);
    let s0 = store::load_settings();
    println!("[t] settings: {:?}", s0);
    let mut s = s0.clone();
    let orig_port = s.mixed_port;
    s.mixed_port = 20809;
    store::save_settings(&s).expect("save settings must succeed in isolated dir");
    let s1 = store::load_settings();
    assert_eq!(s1.mixed_port, 20809);
    // 还原
    let mut s2 = s1;
    s2.mixed_port = orig_port;
    store::save_settings(&s2).expect("restore save failed");
    let s3 = store::load_settings();
    assert_eq!(s3.mixed_port, orig_port);

    // 损坏 JSON → 备份 .corrupt 并回默认，不 panic 不丢现场
    std::fs::write(d.join("settings.json"), b"{ not json !!!").unwrap();
    let s4 = store::load_settings();
    assert_eq!(
        s4.mixed_port, 7890,
        "corrupt settings should fall back to default"
    );
    assert!(
        d.join("settings.json.corrupt").exists(),
        "corrupt file should be preserved"
    );
    println!("[t] store roundtrip + corrupt recovery ✅");
}
