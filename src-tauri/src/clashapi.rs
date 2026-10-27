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

/// 测某节点延迟（走 clash api /proxies/{tag}/delay）。
/// 返回毫秒；API 明确返回失败时保留错误原因，避免和真实超时混淆。
pub async fn test_delay(tag: &str, port: u16) -> Result<Option<u32>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建延迟测试客户端失败: {e}"))?;
    // 不同网络对公共探测站点的可达性不同；单一 gstatic 不可达时，
    // sing-box 会返回 503，即使节点本身可用，因此按顺序回退。
    let probe_urls = [
        "https://cp.cloudflare.com/generate_204",
        "https://www.gstatic.com/generate_204",
        "https://www.google.com/generate_204",
    ];
    let mut last_error = String::new();
    for probe in probe_urls {
        let url = format!(
            "http://127.0.0.1:{port}/proxies/{}/delay?timeout=8000&url={}",
            urlencode(tag),
            urlencode(probe)
        );
        let resp = match client.get(&url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                last_error = format!("请求延迟接口失败（探测 {probe}）: {e}");
                continue;
            }
        };
        let status = resp.status();
        let body = match resp.text().await {
            Ok(body) => body,
            Err(e) => {
                last_error = format!("读取延迟响应失败（探测 {probe}）: {e}");
                continue;
            }
        };
        if !status.is_success() {
            last_error = format!("clash api 延迟测试返回 {status}（探测 {probe}）: {body}");
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(e) => {
                last_error = format!("解析延迟响应失败（探测 {probe}）: {e}");
                continue;
            }
        };
        if let Some(delay) = value.get("delay").and_then(|d| d.as_u64()) {
            return Ok(Some(delay as u32));
        }
        last_error = format!("延迟响应缺少 delay 字段（探测 {probe}）: {body}");
    }
    Err(last_error)
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
