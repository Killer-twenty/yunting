// Rust configbuilder 与旧版 JS configBuilder 产物一致性对比测试
// 基准: scripts/out-js-config.json（由 ui/lib/configBuilder.js 用 fixture-profile.json 生成）
#[path = "../src/paths.rs"]
mod paths;
#[path = "../src/configbuilder.rs"]
mod configbuilder;

use serde_json::Value;

#[allow(unused_imports)]
use paths as _paths;

#[test]
fn matches_js_reference_output() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let js_out = std::fs::read_to_string(root.join("scripts").join("out-js-config.json"))
        .expect("run node config-gen first");
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("scripts").join("fixture-profile.json")).unwrap(),
    )
    .unwrap();

    let nodes = fixture["nodes"].as_array().unwrap().clone();
    let selected = fixture["selectedTag"].as_str().map(|s| s.to_string());
    let mixed_port = fixture["mixedPort"].as_u64().unwrap() as u16;
    let api_port = fixture["clashApiPort"].as_u64().unwrap() as u16;
    let mode = fixture["mode"].as_str().unwrap_or("rule").to_string();

    let rs_cfg = configbuilder::build_config(&nodes, &selected, mixed_port, api_port, &mode)
        .expect("rust build failed");
    let js_cfg: Value = serde_json::from_str(&js_out).unwrap();

    // 逐顶层段对比（rule_set 的 path 因 CARGO_MANIFEST_DIR 绝对路径差异允许不同前缀）
    assert_eq!(rs_cfg["inbounds"], js_cfg["inbounds"], "inbounds mismatch");
    assert_eq!(rs_cfg["outbounds"], js_cfg["outbounds"], "outbounds mismatch");
    assert_eq!(rs_cfg["dns"], js_cfg["dns"], "dns mismatch");
    assert_eq!(rs_cfg["experimental"], js_cfg["experimental"], "experimental mismatch");
    let rs_route = rs_cfg["route"].clone();
    let js_route = js_cfg["route"].clone();
    assert_eq!(rs_route["rules"], js_route["rules"], "route.rules mismatch");
    assert_eq!(rs_route["final"], js_route["final"], "route.final mismatch");
    assert_eq!(rs_route["auto_detect_interface"], js_route["auto_detect_interface"]);
    // rule_set tag/type 一致（path 可能前缀不同）
    let rs_sets = rs_route["rule_set"].as_array().unwrap();
    let js_sets = js_route["rule_set"].as_array().unwrap();
    assert_eq!(rs_sets.len(), js_sets.len());
    for (r, j) in rs_sets.iter().zip(js_sets.iter()) {
        assert_eq!(r["tag"], j["tag"], "rule_set tag mismatch");
        assert_eq!(r["type"], j["type"], "rule_set type mismatch");
        if r["type"] == "local" {
            assert_eq!(r["format"], j["format"]);
            // path 文件名一致
            let rp = r["path"].as_str().unwrap();
            let jp = j["path"].as_str().unwrap();
            assert!(rp.ends_with("cn.srs"), "rust path: {rp}");
            assert!(jp.ends_with("cn.srs"), "js path: {jp}");
        }
    }
    println!("[t] rust configbuilder output == js configBuilder output ✅");
}
