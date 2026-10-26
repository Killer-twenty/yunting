// sing-box config 生成 —— Rust 实现（对齐旧版 configBuilder.js 产物结构）
// 输入: 节点列表(与旧版节点对象一致) / 选中 tag / 端口 / 模式
// 输出: 完整 sing-box 1.11 格式 config（与 data/smoke-config.json 同构）
use serde_json::{json, Value};

use crate::paths::kernel_dir;

fn rule_set_file(name: &str) -> std::path::PathBuf {
    kernel_dir().join(format!("{name}.srs"))
}

pub fn build_config(
    nodes: &[Value],
    selected_tag: &Option<String>,
    mixed_port: u16,
    api_port: u16,
    mode: &str,
) -> Result<Value, String> {
    if nodes.is_empty() {
        return Err("profile.nodes 不能为空".into());
    }
    let mut outbounds: Vec<Value> = Vec::new();
    let mut proxy_tags: Vec<String> = Vec::new();
    for node in nodes {
        if let Some(ob) = node_to_outbound(node) {
            proxy_tags.push(ob["tag"].as_str().unwrap_or("").to_string());
            outbounds.push(ob);
        }
    }
    if proxy_tags.is_empty() {
        return Err("没有可用的出站节点".into());
    }
    let selected = match selected_tag {
        Some(t) if proxy_tags.contains(t) => t.clone(),
        _ => proxy_tags[0].clone(),
    };

    let mut selector_outbounds: Vec<String> = vec!["auto".to_string()];
    selector_outbounds.extend(proxy_tags.iter().cloned());
    outbounds.push(json!({
        "type": "selector",
        "tag": "proxy",
        "outbounds": selector_outbounds,
        "default": selected,
        "interrupt_exist_connections": false,
    }));
    outbounds.push(json!({
        "type": "urltest",
        "tag": "auto",
        "outbounds": proxy_tags,
        "url": "https://www.gstatic.com/generate_204",
        "interval": "5m",
        "tolerance": 50,
        "idle_timeout": "30m",
    }));
    outbounds.push(json!({ "type": "direct", "tag": "direct", "domain_resolver": "local-dns" }));
    outbounds.push(json!({ "type": "block", "tag": "block" }));

    let config = json!({
        "log": { "level": "info", "timestamp": true },
        "dns": build_dns(mode),
        "inbounds": [
            { "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": mixed_port },
        ],
        "outbounds": outbounds,
        "route": build_route(mode),
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{api_port}"),
                "default_mode": if mode == "global" { "global" } else { "rule" },
            },
            "cache_file": { "enabled": true },
        },
    });
    Ok(config)
}

fn build_dns(mode: &str) -> Value {
    let servers = json!([
        { "tag": "local-dns", "type": "udp", "server": "223.5.5.5" },
        { "tag": "proxy-dns", "type": "https", "server": "1.1.1.1", "domain_resolver": "local-dns" },
    ]);
    if mode == "global" {
        return json!({
            "servers": servers,
            "final": "proxy-dns",
            "strategy": "prefer_ipv4",
        });
    }
    json!({
        "servers": servers,
        "rules": [
            { "rule_set": ["geosite-cn"], "server": "local-dns" },
            { "clash_mode": "Global", "server": "proxy-dns" },
        ],
        "final": "proxy-dns",
        "strategy": "prefer_ipv4",
    })
}

fn build_route(mode: &str) -> Value {
    if mode == "global" {
        return json!({
            "rules": [
                { "action": "sniff" },
                { "protocol": "dns", "action": "hijack-dns" },
                { "clash_mode": "Direct", "outbound": "direct" },
            ],
            "final": "proxy",
            "auto_detect_interface": true,
        });
    }
    json!({
        "rules": [
            { "action": "sniff" },
            { "protocol": "dns", "action": "hijack-dns" },
            { "clash_mode": "Direct", "outbound": "direct" },
            { "clash_mode": "Global", "outbound": "proxy" },
            { "rule_set": ["geosite-cn"], "outbound": "direct" },
            { "rule_set": ["geoip-cn"], "outbound": "direct" },
        ],
        "rule_set": [local_rule_set("geosite-cn"), local_rule_set("geoip-cn")],
        "final": "proxy",
        "auto_detect_interface": true,
    })
}

fn local_rule_set(name: &str) -> Value {
    let file = rule_set_file(name);
    if file.exists() {
        return json!({ "type": "local", "tag": name, "format": "binary", "path": file.to_string_lossy() });
    }
    let (dir, file) = if name == "geosite-cn" { ("geosite", "cn.srs") } else { ("geoip", "cn.srs") };
    json!({
        "type": "remote",
        "tag": name,
        "format": "binary",
        "url": format!("https://gh-proxy.com/https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/sing/geo/{dir}/{file}"),
        "download_detour": "direct",
    })
}

/// 内部节点 type → sing-box outbound type
fn outbound_type(t: &str) -> Option<&'static str> {
    match t {
        "ss" => Some("shadowsocks"),
        "vless" => Some("vless"),
        "vmess" => Some("vmess"),
        "trojan" => Some("trojan"),
        "hysteria2" => Some("hysteria2"),
        _ => None,
    }
}

/// 节点 → sing-box outbound（对齐旧版 nodeToOutbound）
pub fn node_to_outbound(node: &Value) -> Option<Value> {
    let server = node.get("server")?.as_str()?;
    let port = node.get("server_port")?.as_u64()?;
    let ntype = node.get("type")?.as_str()?;
    let ob_type = outbound_type(ntype)?;
    let tag = node
        .get("tag")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{server}:{port}"));
    let mut ob = json!({
        "tag": tag,
        "type": ob_type,
        "server": server,
        "server_port": port,
    });
    match ntype {
        "vless" => {
            ob["uuid"] = node.get("uuid").cloned().unwrap_or(Value::Null);
            ob["flow"] = json!(node.get("flow").and_then(|v| v.as_str()).unwrap_or(""));
            if let Some(v) = node.get("tls") { ob["tls"] = v.clone(); }
            if let Some(v) = node.get("transport") { ob["transport"] = v.clone(); }
        }
        "vmess" => {
            ob["uuid"] = node.get("uuid").cloned().unwrap_or(Value::Null);
            ob["security"] = json!(node.get("security").and_then(|v| v.as_str()).unwrap_or("auto"));
            if let Some(v) = node.get("alter_id") { ob["alter_id"] = v.clone(); }
            if let Some(v) = node.get("tls") { ob["tls"] = v.clone(); }
            if let Some(v) = node.get("transport") { ob["transport"] = v.clone(); }
        }
        "trojan" => {
            ob["password"] = node.get("password").cloned().unwrap_or(Value::Null);
            if let Some(v) = node.get("tls") { ob["tls"] = v.clone(); }
            if let Some(v) = node.get("transport") { ob["transport"] = v.clone(); }
        }
        "ss" => {
            ob["method"] = node.get("method").cloned().unwrap_or(Value::Null);
            ob["password"] = node.get("password").cloned().unwrap_or(Value::Null);
            if let Some(v) = node.get("plugin") { ob["plugin"] = v.clone(); }
            if let Some(v) = node.get("plugin_opts") { ob["plugin_opts"] = v.clone(); }
        }
        "hysteria2" => {
            ob["password"] = node.get("password").cloned().unwrap_or(Value::Null);
            if let Some(v) = node.get("obfs") { ob["obfs"] = v.clone(); }
            ob["tls"] = node.get("tls").cloned().unwrap_or_else(|| json!({ "enabled": true }));
        }
        _ => return None,
    }
    Some(ob)
}
