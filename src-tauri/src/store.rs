// data/ 目录持久化（JSON, 顶层 version=1）—— 对齐旧版 store.js 行为
// files: nodes.json / subs.json / settings.json
// 可靠性: 原子写(tmp → rename) + 保存失败返回 Err 供命令层提示; 损坏文件备份 .corrupt 后回默认
// 并发: 所有 load/save 串行化（单实例 + Tauri 命令 async 并发也可能交错读写）
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::paths::data_dir;

pub const DATA_VERSION: u64 = 1;

static STORE_LOCK: Mutex<()> = Mutex::new(());

fn file_path(name: &str) -> PathBuf {
    data_dir().join(match name {
        "nodes" => "nodes.json",
        "subs" => "subs.json",
        "settings" => "settings.json",
        _ => "unknown.json",
    })
}

fn load(name: &str) -> Vec<Value> {
    let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
            .cloned()
            .unwrap_or_default(),
        Err(_) => {
            // 损坏容错: 备份现场，回默认
            let _ = std::fs::rename(&p, p.with_extension("json.corrupt"));
            Vec::new()
        }
    }
}

fn save(name: &str, data: &[Value]) -> Result<(), String> {
    let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(data_dir()).map_err(|e| format!("数据目录创建失败: {e}"))?;
    let payload = serde_json::json!({ "version": DATA_VERSION, "data": data });
    let p = file_path(name);
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&payload).unwrap())
        .map_err(|e| format!("写入 {name}.json 失败: {e}"))?;
    // 原子替换: rename 前先删目标（Windows rename 对已存在文件可能失败）
    let _ = std::fs::remove_file(&p);
    std::fs::rename(&tmp, &p).map_err(|e| format!("替换 {name}.json 失败: {e}"))?;
    Ok(())
}

// ---------- nodes ----------
pub fn load_nodes() -> Vec<Value> {
    load("nodes")
}
pub fn save_nodes(nodes: Vec<Value>) -> Result<(), String> {
    save("nodes", &nodes)
}

// ---------- subs ----------
pub fn load_subs() -> Vec<Value> {
    load("subs")
}
pub fn save_subs(subs: Vec<Value>) -> Result<(), String> {
    save("subs", &subs)
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
    /// 点击窗口 × 的行为: "ask"(每次询问, 默认) | "tray"(最小化到托盘) | "quit"(直接退出)
    #[serde(default = "d_close_behavior")]
    pub close_behavior: String,
}

fn d_mixed_port() -> u16 {
    7890
}
fn d_api_port() -> u16 {
    9090
}
fn d_auto_proxy() -> bool {
    true
}
fn d_route_mode() -> String {
    "rule".into()
}
fn d_close_behavior() -> String {
    "ask".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mixed_port: 7890,
            clash_api_port: 9090,
            auto_system_proxy: true,
            route_mode: "rule".into(),
            direct_list: Vec::new(),
            close_behavior: "ask".into(),
        }
    }
}

pub fn load_settings() -> Settings {
    let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let p = file_path("settings");
    if !p.exists() {
        return Settings::default();
    }
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Settings::default();
    };
    let Ok(obj) = serde_json::from_str::<Value>(&text) else {
        let _ = std::fs::rename(&p, p.with_extension("json.corrupt"));
        return Settings::default();
    };
    // 兼容旧版字段名（snake_case 不存在——旧版就是 camelCase 带默认合并）
    let data = obj.get("data").cloned().unwrap_or_else(|| obj.clone());
    serde_json::from_value::<Settings>(data).unwrap_or_default()
}

pub fn save_settings(s: &Settings) -> Result<(), String> {
    let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(data_dir()).map_err(|e| format!("数据目录创建失败: {e}"))?;
    let payload = serde_json::json!({ "version": DATA_VERSION, "data": s });
    let p = file_path("settings");
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&payload).unwrap())
        .map_err(|e| format!("写入 settings.json 失败: {e}"))?;
    let _ = std::fs::remove_file(&p);
    std::fs::rename(&tmp, &p).map_err(|e| format!("替换 settings.json 失败: {e}"))?;
    Ok(())
}
