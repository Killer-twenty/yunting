// clash api 客户端（测延迟等）—— Rust 重写（tokio reqwest）
use std::time::Duration;

/// GET JSON。（节点切换等扩展用）
#[allow(dead_code)]
pub async fn get_json(port: u16, path: &str) -> Result<serde_json::Value, String> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("clash api {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// 测某节点延迟（走 clash api /proxies/{tag}/delay）。返回毫秒或 None（超时）。
pub async fn test_delay(tag: &str, port: u16) -> Option<u32> {
    let url = format!(
        "http://127.0.0.1:{port}/proxies/{}/delay?timeout=5000&url={}",
        urlencode(tag),
        urlencode("https://www.gstatic.com/generate_204")
    );
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(6)).build() {
        Ok(c) => c,
        Err(_) => return None,
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(v) => v.get("delay").and_then(|d| d.as_u64()).map(|d| d as u32),
            Err(_) => None,
        },
        _ => None,
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
