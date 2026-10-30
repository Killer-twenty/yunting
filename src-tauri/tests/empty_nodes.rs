#[path = "../src/configbuilder.rs"]
mod configbuilder;
#[path = "../src/paths.rs"]
mod paths;
#[path = "../src/kernel.rs"]
mod kernel;

#[test]
fn empty_nodes_config_builds_and_checks() {
    let cfg = configbuilder::build_config(&[], &None, 20810, 20811, "rule", &[]).expect("empty build");
    assert_eq!(cfg["outbounds"].as_array().unwrap().iter().find(|o| o["tag"] == "proxy").unwrap()["outbounds"].as_array().unwrap().len(), 1);
    let dir = std::env::temp_dir().join("yunting-empty-cfg-test");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("config.json");
    std::fs::write(&p, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();
    let exe = paths::kernel_exe();
    kernel::check_config(&exe, &p).expect("sing-box check must accept empty-node config");
    println!("[t] empty nodes config ✅");
}
