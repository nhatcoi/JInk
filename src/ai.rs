// OpenAI-compatible chat completions with streaming.
use base64::Engine;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    #[serde(default)]
    pub search_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum AiStreamEvent {
    Token(String),
    Done,
    Error(String),
}

pub async fn stream_chat(
    config: AiConfig,
    messages: Vec<ChatMessage>,
    images: Vec<String>,
    sender: UnboundedSender<AiStreamEvent>,
) {
    if let Err(e) = run(config, messages, images, &sender).await {
        let _ = sender.send(AiStreamEvent::Error(e.to_string()));
    }
}

async fn run(
    config: AiConfig,
    messages: Vec<ChatMessage>,
    images: Vec<String>,
    sender: &UnboundedSender<AiStreamEvent>,
) -> anyhow::Result<()> {
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let mut convo: Vec<Value> = messages
        .into_iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();

    if !images.is_empty() {
        attach_images(&mut convo, &images);
    }

    if !config.search_key.is_empty() {
        resolve_tools(&client, &url, &config, &mut convo).await?;
    }

    let resp = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .json(&json!({
            "model": config.model,
            "messages": convo,
            "stream": true,
            "temperature": 0.3,
        }))
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

        while let Some(nl) = buffer.find('\n') {
            let line = buffer[..nl].trim().to_string();
            buffer.drain(..=nl);

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                let _ = sender.send(AiStreamEvent::Done);
                return Ok(());
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(token) = json["choices"][0]["delta"]["content"].as_str() {
                    if !token.is_empty() {
                        let _ = sender.send(AiStreamEvent::Token(token.to_string()));
                    }
                }
            }
        }
    }

    let _ = sender.send(AiStreamEvent::Done);
    Ok(())
}

fn attach_images(convo: &mut [Value], paths: &[String]) {
    let Some(msg) = convo.iter_mut().rev().find(|m| m["role"] == "user") else {
        return;
    };
    let text = msg["content"].as_str().unwrap_or_default().to_string();
    let mut parts = vec![json!({ "type": "text", "text": text })];
    for path in paths {
        if let Some(url) = image_data_url(path) {
            parts.push(json!({ "type": "image_url", "image_url": { "url": url } }));
        }
    }
    msg["content"] = Value::Array(parts);
}

fn image_data_url(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mime = match path.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

fn web_search_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web for current facts, definitions, or context missing from the input.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" }
                },
                "required": ["query"]
            }
        }
    })
}

async fn resolve_tools(
    client: &reqwest::Client,
    url: &str,
    config: &AiConfig,
    convo: &mut Vec<Value>,
) -> anyhow::Result<()> {
    for _ in 0..3 {
        let resp = client
            .post(url)
            .bearer_auth(&config.api_key)
            .json(&json!({
                "model": config.model,
                "messages": &convo,
                "stream": false,
                "temperature": 0.3,
                "tools": [web_search_tool()],
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(());
        }
        let json: Value = resp.json().await?;
        let msg = &json["choices"][0]["message"];
        let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            return Ok(());
        }
        convo.push(msg.clone());
        for call in calls {
            let id = call["id"].as_str().unwrap_or_default();
            let query = call["function"]["arguments"]
                .as_str()
                .and_then(|a| serde_json::from_str::<Value>(a).ok())
                .and_then(|v| v["query"].as_str().map(str::to_string))
                .unwrap_or_default();
            let result = web_search(&config.search_key, &query)
                .await
                .unwrap_or_else(|e| format!("Search failed: {e}"));
            convo.push(json!({ "role": "tool", "tool_call_id": id, "content": result }));
        }
    }
    Ok(())
}

async fn web_search(api_key: &str, query: &str) -> anyhow::Result<String> {
    let resp = reqwest::Client::new()
        .post("https://api.tavily.com/search")
        .json(&json!({
            "api_key": api_key,
            "query": query,
            "max_results": 5,
            "include_answer": true,
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let json: Value = resp.json().await?;
    let mut out = String::new();
    if let Some(answer) = json["answer"].as_str() {
        out.push_str(answer);
        out.push('\n');
    }
    for r in json["results"].as_array().into_iter().flatten() {
        let title = r["title"].as_str().unwrap_or_default();
        let content = r["content"].as_str().unwrap_or_default();
        let url = r["url"].as_str().unwrap_or_default();
        out.push_str(&format!("- {title}: {content} ({url})\n"));
    }
    Ok(out)
}

pub fn enhance_prompt(text: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".into(),
            content: "You are a writing assistant. Improve the user's text: fix grammar, spelling and clarity while preserving meaning, tone and language. Return ONLY the improved text, no quotes, no explanation.".into(),
        },
        ChatMessage {
            role: "user".into(),
            content: text.into(),
        },
    ]
}

pub fn translate_prompt(text: &str, from: &str, to: &str) -> Vec<ChatMessage> {
    let instruction = if from == "auto" {
        format!("Detect the source language and translate it to {to}.")
    } else {
        format!("Translate from {from} to {to}.")
    };
    vec![
        ChatMessage {
            role: "system".into(),
            content: format!("{instruction} Return ONLY the translation, no quotes, no explanation."),
        },
        ChatMessage {
            role: "user".into(),
            content: text.into(),
        },
    ]
}

pub fn explain_prompt(text: &str, lang: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".into(),
            content: format!("You are a concise explainer. Explain the meaning of the word, sentence, or concept the user sends. Answer in {lang} if you are fluent in it; otherwise answer in English. Be brief: 1-3 short sentences, no preamble, no quotes."),
        },
        ChatMessage {
            role: "user".into(),
            content: if text.is_empty() { "Explain this.".into() } else { text.into() },
        },
    ]
}

pub fn strip_think_tags(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find("<think>") {
        if let Some(end) = out[start..].find("</think>") {
            out.replace_range(start..start + end + 8, "");
        } else {
            out.truncate(start);
            break;
        }
    }
    out.trim_start().to_string()
}

