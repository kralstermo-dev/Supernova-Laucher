use std::sync::Arc;

use futures::AsyncReadExt;
use gpui::{
    SharedString,
    http_client::{self, AsyncBody, HttpClient, Method},
};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum MclogsResult {
    Success { url: SharedString },
    Failure { message: SharedString },
}

#[derive(Debug, Deserialize)]
struct MclogsResponse {
    success: bool,
    url: Option<String>,
    error: Option<String>,
}

/// Uploads `content` (already scrubbed of secrets by the caller) to mclo.gs,
/// returning either the shareable URL or a human-readable failure reason.
/// Never panics on a network/parse failure — always resolves to a `MclogsResult`
/// so the caller can show it directly as a notification.
pub async fn upload_log(client: Arc<dyn HttpClient>, content: String) -> MclogsResult {
    match try_upload_log(client, content).await {
        Ok(result) => result,
        Err(err) => MclogsResult::Failure { message: format!("{err}").into() },
    }
}

async fn try_upload_log(client: Arc<dyn HttpClient>, content: String) -> anyhow::Result<MclogsResult> {
    let body = format!("content={}", urlencoding::encode(&content));

    let request = http_client::Request::builder()
        .method(Method::POST)
        .uri("https://api.mclo.gs/1/log")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(AsyncBody::from(body))?;

    let mut response = client.send(request).await?;

    let mut bytes = Vec::new();
    response.body_mut().read_to_end(&mut bytes).await?;

    let parsed: MclogsResponse = serde_json::from_slice(&bytes)?;

    if parsed.success {
        Ok(MclogsResult::Success { url: parsed.url.unwrap_or_default().into() })
    } else {
        Ok(MclogsResult::Failure { message: parsed.error.unwrap_or_else(|| "Unknown error".to_string()).into() })
    }
}
