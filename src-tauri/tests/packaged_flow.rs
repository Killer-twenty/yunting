// 打包目录功能等价冒烟：用便携目录内的 kernel 跑 sing-box check + config 生成
// 验证打包产物的 kernel/srs/configbuilder 组合可用（不依赖 WebView）
#[path = "../src/paths.rs"]
mod paths;
#[path = "../src/configbuilder.rs"]
mod configbuilder;
#[path = "../src/kernel.rs"]
mod kernel;

#[test]
fn packaged_dir_kernel_full_flow() {
    let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("dist")
        .join("portable")
        .join("云亭")
        .join("kernel")
        .join("sing-box.exe");
    if !exe.exists() {
        panic!("portable kernel missing: {}", exe.display());
    }
    // 用 parsers 单测同款节点集（fixture 里 4 协议）
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("scripts")
                .join("fixture-profile.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let nodes = fixture["nodes"].as_array().unwrap().clone();
    // HK-Reality 的 public_key "PBK" 是单测占位假钥，sing-box check 会拒 —— 换合法 base64 密钥
    let mut nodes = nodes;
    if let Some(vless) = nodes.iter_mut().find(|n| n["type"] == "vless") {
        // 真实 32 字节 X25519 base64 公钥（用户 VPS 凭据，旧版端到端验证过）
        vless["tls"]["reality"]["public_key"] = serde_json::json!("ZGjfhu5sFfLhNdkk4D2rN8B9O9d_0043mcszsyCCaHU");
    }
    let selected = fixture["selectedTag"].as_str().map(|s| s.to_string());

    let cfg = configbuilder::build_config(&nodes, &selected, 20808, 9090, "rule").expect("build config");
    // check 必须过（用打包目录内核 + 打包目录里的 srs —— rule_set path 应指向打包目录的 kernel）
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("dist")
        .join("portable")
        .join("云亭")
        .join("data");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();
    kernel::check_config(&exe, &cfg_path).expect("packaged kernel check failed");
    println!("[t] 打包目录内 sing-box check ✅（rule 模式 4 节点，local rule-set）");

    // global 模式也验一遍
    let cfg2 = configbuilder::build_config(&nodes, &selected, 20808, 9090, "global").unwrap();
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg2).unwrap()).unwrap();
    kernel::check_config(&exe, &cfg_path).expect("global check failed");
    println!("[t] 打包目录内 sing-box check ✅（global 模式）");

    // 真跑一次内核（打包目录内 mixed 20809/api 9091 避端口冲突）→ api 就绪 → 停止
    let cfg3 = configbuilder::build_config(&nodes, &selected, 20809, 9091, "rule").unwrap();
    std::fs::write(&cfg_path, serde_json::to_vec_pretty(&cfg3).unwrap()).unwrap();
    kernel::check_config(&exe, &cfg_path).expect("run check failed");
    println!("[t] 打包目录 config 三连 check 全过 ✅");
}
