// data/ 目录持久化（JSON, 顶层 version=1）—— 对齐旧版 store.js 行为
// files: nodes.json / subs.json / settings.json
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::paths::data_dir;

pub const DATA_VERSION: u64 = 1;

fn file_path(name: &str) -> PathBuf {
    data_dir().join(match name {
        "nodes" => "nodes.json",
        "subs" => "subs.json",
        "settings" => "settings.json",
        _ => "unknown.json",
    })
}

fn load(name: &str) -> Vec<Value> {
    let p = file_path(name);
    if !p.exists() {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(obj) => obj
            .get("data")
            .and_then(|d| d.as_array())
            .map(|a| a.clone())
            .unwrap_or_default(),
        Err(_) => Vec::new(), // 损坏容错回默认（与旧版一致）
    }
}

fn save(name: &str, data: &[Value]) {
    let _ = std::fs::create_dir_all(data_dir());
    let payload = serde_json::json!({ "version": DATA_VERSION, "data": data });
    let p = file_path(name);
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&payload).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, &p); // 原子写,避免半截文件
    }
}

// ---------- nodes ----------
pub fn load_nodes() -> Vec<Value> {
    load("nodes")
}
pub fn save_nodes(nodes: Vec<Value>) {
    save("nodes", &nodes);
}

// ---------- subs ----------
pub fn load_subs() -> Vec<Value> {
    load("subs")
}
pub fn save_subs(subs: Vec<Value>) {
    save("subs", &subs);
}

// ---------- settings ----------
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "d_mixed_port")]
    pub mixed_port: u16,
    #[serde(default = "d_api_port")]
    pub clash_api_port: u16,
    #[serde(default = "d_auto_proxy")]
    pub auto_system_proxy: bool,
    #[serde(default = "d_route_mode")]
    pub route_mode: String,
    #[serde(default)]
    pub direct_list: Vec<String>,
}

fn d_mixed_port() -> u16 { 7890 }
fn d_api_port() -> u16 { 9090 }
fn d_auto_proxy() -> bool { true }
fn d_route_mode() -> String { "rule".into() }

impl Default for Settings {
    fn default() -> Self {
        Self {
            mixed_port: 7890,
            clash_api_port: 9090,
            auto_system_proxy: true,
            route_mode: "rule".into(),
            direct_list: Vec::new(),
        }
    }
}

pub fn load_settings() -> Settings {
    let p = file_path("settings");
    if !p.exists() {
        return Settings::default();
    }
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Settings::default();
    };
    let Ok(obj) = serde_json::from_str::<Value>(&text) else {
        return Settings::default();
    };
    // 兼容旧版字段名（snake_case 不存在——旧版就是 camelCase 带默认合并）
    let data = obj.get("data").cloned().unwrap_or_else(|| obj.clone());
    if let Ok(s) = serde_json::from_value::<Settings>(data) {
        s
    } else {
        Settings::default()
    }
}

pub fn save_settings(s: &Settings) {
    let _ = std::fs::create_dir_all(data_dir());
    let payload = serde_json::json!({ "version": DATA_VERSION, "data": s });
    let p = file_path("settings");
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&payload).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}
