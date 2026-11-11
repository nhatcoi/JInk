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

/// Speech-to-text. Whisper-style models use `/audio/transcriptions`; any other model (e.g. a
/// Gemini chat model) is sent the audio as an `input_audio` chat message instead, since many
/// gateways serve no dedicated transcription endpoint.
pub async fn transcribe(base_url: &str, api_key: &str, model: &str, wav: Vec<u8>) -> anyhow::Result<String> {
    let lower = model.to_ascii_lowercase();
    if lower.contains("whisper") || lower.contains("transcribe") {
        transcribe_endpoint(base_url, api_key, model, wav).await
    } else {
        transcribe_via_chat(base_url, api_key, model, wav).await
    }
}

async fn transcribe_endpoint(base_url: &str, api_key: &str, model: &str, wav: Vec<u8>) -> anyhow::Result<String> {
    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let part = reqwest::multipart::Part::bytes(wav).file_name("audio.wav").mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "json")
        .part("file", part);
    let resp = reqwest::Client::new().post(&url).bearer_auth(api_key).multipart(form).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {}: {}", status, text);
    }
    let json: Value = resp.json().await?;
    Ok(json["text"].as_str().unwrap_or_default().trim().to_string())
}

async fn transcribe_via_chat(base_url: &str, api_key: &str, model: &str, wav: Vec<u8>) -> anyhow::Result<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let b64 = base64::engine::general_purpose::STANDARD.encode(&wav);
    let body = json!({
        "model": model,
        "temperature": 0,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": "Transcribe this audio exactly as spoken, in the language spoken. Return ONLY the transcript, nothing else. If there is no speech, return an empty string." },
                { "type": "input_audio", "input_audio": { "data": b64, "format": "wav" } }
            ]
        }]
    });
    let resp = reqwest::Client::new().post(&url).bearer_auth(api_key).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {}: {}", status, text);
    }
    let json: Value = resp.json().await?;
    let text = json["choices"][0]["message"]["content"].as_str().unwrap_or_default();
    Ok(strip_think_tags(text).trim().to_string())
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

const MAX_CONTEXT_CHARS: usize = 4000;

fn with_context(instruction: String, context: Option<&str>) -> String {
    match context {
        Some(c) => {
            let c: String = c.chars().take(MAX_CONTEXT_CHARS).collect();
            format!("{instruction}\n\nBackground context (text the user selected in another app). Use it only to understand intent and terminology; never repeat or include it in your answer:\n<context>\n{c}\n</context>")
        }
        None => instruction,
    }
}

pub const LANGS: [&str; 8] = ["en", "vi", "ja", "ko", "zh", "fr", "de", "es"];

pub fn lang_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "vi" => "Vietnamese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        other => other,
    }
}

/// Shared rule telling the model which two languages the user works in.
fn lang_rule(a: &str, b: &str) -> String {
    format!(
        "The user works in two main languages: {} and {}. Detect which one the user's text is written in.",
        lang_name(a),
        lang_name(b)
    )
}

/// Turn a rough idea into a clear, well-structured prompt for an AI assistant.
pub fn enhance_prompt(text: &str, a: &str, b: &str, context: Option<&str>) -> Vec<ChatMessage> {
    let sys = format!(
        "You are a prompt engineer. {} Rewrite the user's rough request as a clear, specific, well-structured prompt for an AI assistant: state the goal, add the needed context and constraints, and the desired output format, but invent no facts and do not answer it. Write the prompt in the SAME language as the user's text. Return ONLY the prompt, no quotes, no explanation.",
        lang_rule(a, b)
    );
    vec![
        ChatMessage { role: "system".into(), content: with_context(sys, context) },
        ChatMessage { role: "user".into(), content: text.into() },
    ]
}

pub fn refine_prompt(text: &str, a: &str, b: &str, context: Option<&str>) -> Vec<ChatMessage> {
    let sys = format!(
        "You are a writing assistant. {} Improve the text: fix grammar, spelling, formatting and clarity while preserving meaning and tone. Reply in the SAME language as the text (never translate). Return ONLY the improved text, no quotes, no explanation.",
        lang_rule(a, b)
    );
    vec![
        ChatMessage { role: "system".into(), content: with_context(sys, context) },
        ChatMessage { role: "user".into(), content: text.into() },
    ]
}

/// Translate between the two main languages: A -> B, B -> A, anything else -> A.
pub fn translate_prompt(text: &str, a: &str, b: &str) -> Vec<ChatMessage> {
    let sys = format!(
        "{} If the text is in {}, translate it to {}. If it is in {}, translate it to {}. If it is in any other language, translate it to {}. Return ONLY the translation, no quotes, no explanation.",
        lang_rule(a, b),
        lang_name(a), lang_name(b),
        lang_name(b), lang_name(a),
        lang_name(a)
    );
    vec![
        ChatMessage { role: "system".into(), content: sys },
        ChatMessage { role: "user".into(), content: text.into() },
    ]
}

pub fn explain_prompt(text: &str, a: &str, b: &str, context: Option<&str>) -> Vec<ChatMessage> {
    let sys = format!(
        "You are a concise explainer. {} Explain the meaning of the word, sentence, or concept the user sends. Answer in the SAME language as the user's text; if it is in neither language, answer in {}. Be brief: 1-3 short sentences, no preamble, no quotes.",
        lang_rule(a, b),
        lang_name(a)
    );
    vec![
        ChatMessage { role: "system".into(), content: with_context(sys, context) },
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

