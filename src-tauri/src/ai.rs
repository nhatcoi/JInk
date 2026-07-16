// OpenAI-compatible chat completions with streaming, surfaced to the UI via events.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Window};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    temperature: f32,
}

/// Stream a chat completion. Emits `ai-token` per delta, then `ai-done` or `ai-error`.
/// `request_id` lets the frontend correlate a stream with the widget that started it.
pub async fn stream_chat(
    window: Window,
    config: AiConfig,
    messages: Vec<ChatMessage>,
    request_id: String,
) {
    if let Err(e) = run(&window, config, messages, &request_id).await {
        let _ = window.emit("ai-error", (request_id, e.to_string()));
    }
}

async fn run(
    window: &Window,
    config: AiConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let body = ChatRequest {
        model: config.model,
        messages,
        stream: true,
        temperature: 0.3,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {}: {}", status, text);
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // SSE frames are separated by blank lines; process complete lines only.
        while let Some(nl) = buffer.find('\n') {
            let line = buffer[..nl].trim().to_string();
            buffer.drain(..=nl);

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                window.emit("ai-done", request_id)?;
                return Ok(());
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(token) = json["choices"][0]["delta"]["content"].as_str() {
                    if !token.is_empty() {
                        window.emit("ai-token", (request_id, token))?;
                    }
                }
            }
        }
    }

    window.emit("ai-done", request_id)?;
    Ok(())
}
