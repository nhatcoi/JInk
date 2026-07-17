// OpenAI-compatible chat completions with streaming, surfaced to the UI via events.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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

#[derive(Debug, Clone, Serialize)]
pub struct LocalProvider {
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub running: bool,
}

#[derive(Deserialize)]
struct ModelsResponse {
    // Ollama with no models pulled returns `"data": null` rather than `[]`.
    data: Option<Vec<ModelEntry>>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// Well-known local OpenAI-compatible servers, by default host:port.
const LOCAL_CANDIDATES: &[(&str, u16)] = &[
    ("Ollama", 11434),
    ("LM Studio", 1234),
    ("llama.cpp server", 8080),
    ("text-generation-webui", 5000),
    ("vLLM", 8000),
];

/// Probe well-known local ports for a running OpenAI-compatible `/v1/models`
/// endpoint (Ollama, LM Studio, llama.cpp server, etc.) and list their models.
/// Unreachable ports are skipped silently — most of them won't be running.
/// For runtimes not currently running, also check disk for models already
/// pulled/downloaded, so the UI can offer "Start" instead of "install this".
pub async fn detect_local() -> Vec<LocalProvider> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(600))
        .build()
        .unwrap_or_default();

    let probes = LOCAL_CANDIDATES.iter().map(|(name, port)| {
        let client = client.clone();
        let base_url = format!("http://127.0.0.1:{port}/v1");
        let name = name.to_string();
        async move {
            let resp = client.get(format!("{base_url}/models")).send().await.ok()?;
            let parsed = resp.json::<ModelsResponse>().await.ok()?;
            let models = parsed.data.unwrap_or_default();
            if models.is_empty() {
                return None;
            }
            Some(LocalProvider {
                name,
                base_url,
                models: models.into_iter().map(|m| m.id).collect(),
                running: true,
            })
        }
    });

    let mut found: Vec<LocalProvider> = futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect();

    if !found.iter().any(|p| p.name == "Ollama") {
        let models = scan_ollama_installed();
        if !models.is_empty() {
            found.push(LocalProvider {
                name: "Ollama".into(),
                base_url: "http://127.0.0.1:11434/v1".into(),
                models,
                running: false,
            });
        }
    }
    if !found.iter().any(|p| p.name == "LM Studio") {
        let models = scan_lmstudio_installed();
        if !models.is_empty() {
            found.push(LocalProvider {
                name: "LM Studio".into(),
                base_url: "http://127.0.0.1:1234/v1".into(),
                models,
                running: false,
            });
        }
    }

    found
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Models pulled via `ollama pull` but not necessarily loaded — Ollama keeps
/// one manifest file per tag under the library folder regardless of whether
/// the server process is running.
fn scan_ollama_installed() -> Vec<String> {
    let Some(home) = home_dir() else { return vec![] };
    let base = home.join(".ollama/models/manifests/registry.ollama.ai/library");
    let Ok(models) = std::fs::read_dir(&base) else {
        return vec![];
    };
    models
        .flatten()
        .flat_map(|model| {
            let model_name = model.file_name().to_string_lossy().to_string();
            std::fs::read_dir(model.path())
                .into_iter()
                .flatten()
                .flatten()
                .map(move |tag| format!("{model_name}:{}", tag.file_name().to_string_lossy()))
        })
        .collect()
}

/// LM Studio downloads GGUF files under `<publisher>/<repo>/*.gguf`, in
/// either its legacy cache dir or the newer `~/.lmstudio` location.
fn scan_lmstudio_installed() -> Vec<String> {
    let Some(home) = home_dir() else { return vec![] };
    let mut out = vec![];
    for base in [
        home.join(".cache/lm-studio/models"),
        home.join(".lmstudio/models"),
    ] {
        let Ok(publishers) = std::fs::read_dir(&base) else {
            continue;
        };
        for publisher in publishers.flatten() {
            let Ok(repos) = std::fs::read_dir(publisher.path()) else {
                continue;
            };
            for repo in repos.flatten() {
                let has_gguf = std::fs::read_dir(repo.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|f| f.file_name().to_string_lossy().ends_with(".gguf"));
                if has_gguf {
                    out.push(format!(
                        "{}/{}",
                        publisher.file_name().to_string_lossy(),
                        repo.file_name().to_string_lossy()
                    ));
                }
            }
        }
    }
    out
}

/// Launch a not-yet-running local runtime and wait for its API to answer.
/// Only Ollama and LM Studio (via its `lms` CLI) have a reliable headless
/// start command — anything else must be started by the user.
pub async fn start_local(name: &str) -> Result<String, String> {
    let (cmd, args): (&str, &[&str]) = match name {
        "Ollama" => ("ollama", &["serve"]),
        "LM Studio" => ("lms", &["server", "start"]),
        _ => return Err(format!("{name} has no auto-start — launch it manually.")),
    };

    std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't launch `{cmd}`: {e}"))?;

    let port = LOCAL_CANDIDATES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| *p)
        .ok_or_else(|| format!("Unknown runtime {name}"))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap_or_default();
    let url = format!("http://127.0.0.1:{port}/v1/models");

    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                return Ok(format!("{name} is up."));
            }
        }
    }
    Err(format!("{name} didn't come up in time — check it manually."))
}
