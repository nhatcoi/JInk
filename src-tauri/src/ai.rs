// OpenAI-compatible chat completions with streaming, surfaced to the UI via events.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Window};

/// Runtimes we spawned, by name — so Stop can kill exactly what we started.
fn servers() -> &'static Mutex<HashMap<String, Child>> {
    static S: OnceLock<Mutex<HashMap<String, Child>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

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

/// A model a provider can serve. Daemons only need `id`; launchers also need a
/// launch source — a `.gguf` `path` (`-m`) or an HF `repo` (`-hf`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    pub id: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
}

impl LocalModel {
    fn from_id(id: String) -> Self {
        LocalModel {
            id,
            path: None,
            repo: None,
        }
    }
}

/// A detected local runtime. UI flow is generic over `kind`: pick provider →
/// (start if needed) → pick model.
#[derive(Debug, Clone, Serialize)]
pub struct LocalProvider {
    pub name: String,
    /// "daemon" serves every model, pick at request time; "launcher" is spawned
    /// bound to one model, pick before start.
    pub kind: String,
    pub base_url: String,
    pub running: bool,
    /// Can we start it headless? Others must be launched by the user first.
    pub can_start: bool,
    pub models: Vec<LocalModel>,
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

enum Kind {
    Daemon,
    Launcher,
}

impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Kind::Daemon => "daemon",
            Kind::Launcher => "launcher",
        }
    }
}

struct ProviderSpec {
    name: &'static str,
    kind: Kind,
    port: u16,
    can_start: bool,
    scan: fn() -> Vec<LocalModel>,
}

const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        name: "Ollama",
        kind: Kind::Daemon,
        port: 11434,
        can_start: true,
        scan: scan_ollama_installed,
    },
    ProviderSpec {
        name: "LM Studio",
        kind: Kind::Daemon,
        port: 1234,
        can_start: true,
        scan: scan_lmstudio_installed,
    },
    ProviderSpec {
        name: "llama-server",
        kind: Kind::Launcher,
        port: 8080,
        can_start: true,
        scan: scan_llama_models,
    },
    ProviderSpec {
        name: "Jan",
        kind: Kind::Daemon,
        port: 1337,
        can_start: false,
        scan: no_disk,
    },
    ProviderSpec {
        name: "vLLM",
        kind: Kind::Daemon,
        port: 8000,
        can_start: false,
        scan: no_disk,
    },
    ProviderSpec {
        name: "GPT4All",
        kind: Kind::Daemon,
        port: 4891,
        can_start: false,
        scan: no_disk,
    },
];

fn no_disk() -> Vec<LocalModel> {
    vec![]
}

/// Probe each provider's port for a running server, else scan disk for
/// downloaded models. Included if running or has models on disk.
pub async fn detect_local() -> Vec<LocalProvider> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(600))
        .build()
        .unwrap_or_default();

    let probes = PROVIDERS.iter().map(|spec| {
        let client = client.clone();
        let base_url = format!("http://127.0.0.1:{}/v1", spec.port);
        async move {
            let running = probe_running(&client, &base_url).await;
            let is_running = running.is_some();
            let mut models: Vec<LocalModel> = running
                .unwrap_or_default()
                .into_iter()
                .map(LocalModel::from_id)
                .collect();
            if !is_running || models.is_empty() {
                let disk = (spec.scan)();
                if !disk.is_empty() {
                    models = disk;
                }
            }
            if !is_running && models.is_empty() {
                return None;
            }
            Some(LocalProvider {
                name: spec.name.into(),
                kind: spec.kind.as_str().into(),
                base_url,
                running: is_running,
                can_start: spec.can_start,
                models,
            })
        }
    });

    futures_util::future::join_all(probes)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// `Some(ids)` if the server answered (possibly empty), `None` if unreachable.
async fn probe_running(client: &reqwest::Client, base_url: &str) -> Option<Vec<String>> {
    let resp = client.get(format!("{base_url}/models")).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed = resp.json::<ModelsResponse>().await.ok()?;
    Some(
        parsed
            .data
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.id)
            .collect(),
    )
}

fn scan_llama_models() -> Vec<LocalModel> {
    let Some(home) = home_dir() else {
        return vec![];
    };
    let mut out = vec![];

    let mut files = vec![];
    for dir in [
        ".cache/llama.cpp",
        ".llama.cpp/models",
        "models",
        "Downloads",
    ] {
        collect_gguf_paths(&home.join(dir), 0, &mut files);
    }
    files.sort();
    files.dedup();
    for path in files {
        let id = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        out.push(LocalModel {
            id,
            path: Some(path),
            repo: None,
        });
    }

    for repo in scan_hf_gguf_repos(&home) {
        out.push(LocalModel {
            id: repo.clone(),
            path: None,
            repo: Some(repo),
        });
    }
    out
}

/// HF hub cache stores each repo as `models--<org>--<name>` with `/` encoded as
/// `--`. List those holding a `.gguf`, decoded back to `<org>/<name>` for `-hf`.
fn scan_hf_gguf_repos(home: &std::path::Path) -> Vec<String> {
    let hub = home.join(".cache/huggingface/hub");
    let Ok(entries) = std::fs::read_dir(&hub) else {
        return vec![];
    };
    let mut out = vec![];
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("models--") else {
            continue;
        };
        let mut ggufs = vec![];
        collect_gguf_paths(&entry.path(), 0, &mut ggufs);
        if ggufs.is_empty() {
            continue;
        }
        out.push(rest.replace("--", "/"));
    }
    out.sort();
    out.dedup();
    out
}

/// Recursively collect full `.gguf` paths under `dir`, capped so a stray huge
/// tree can't stall detection.
fn collect_gguf_paths(dir: &std::path::Path, depth: usize, out: &mut Vec<String>) {
    if depth > 4 || out.len() >= 50 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_gguf_paths(&path, depth + 1, out);
        } else if path.extension().is_some_and(|e| e == "gguf") {
            out.push(path.to_string_lossy().to_string());
        }
    }
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

#[derive(Deserialize)]
struct OllamaManifest {
    layers: Vec<OllamaLayer>,
}

#[derive(Deserialize)]
struct OllamaLayer {
    digest: String,
}

/// Tags with a manifest under the library folder. Skip incomplete pulls: a
/// manifest whose blobs are gone (interrupted pull, GC) isn't usable.
fn scan_ollama_installed() -> Vec<LocalModel> {
    let Some(home) = home_dir() else {
        return vec![];
    };
    let root = home.join(".ollama/models");
    let blobs = root.join("blobs");
    let base = root.join("manifests/registry.ollama.ai/library");
    let Ok(models) = std::fs::read_dir(&base) else {
        return vec![];
    };
    models
        .flatten()
        .flat_map(|model| {
            let model_name = model.file_name().to_string_lossy().to_string();
            let blobs = blobs.clone();
            std::fs::read_dir(model.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(move |tag| ollama_tag_complete(&tag.path(), &blobs))
                .map(move |tag| {
                    LocalModel::from_id(format!(
                        "{model_name}:{}",
                        tag.file_name().to_string_lossy()
                    ))
                })
        })
        .collect()
}

/// Ollama stores each layer under `blobs/` with `:` in the digest replaced by `-`.
fn ollama_tag_complete(manifest: &std::path::Path, blobs: &std::path::Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(manifest) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<OllamaManifest>(&raw) else {
        return false;
    };
    !parsed.layers.is_empty()
        && parsed
            .layers
            .iter()
            .all(|layer| blobs.join(layer.digest.replace(':', "-")).exists())
}

/// LM Studio downloads GGUF files under `<publisher>/<repo>/*.gguf`, in
/// either its legacy cache dir or the newer `~/.lmstudio` location.
fn scan_lmstudio_installed() -> Vec<LocalModel> {
    let Some(home) = home_dir() else {
        return vec![];
    };
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
                    out.push(LocalModel::from_id(format!(
                        "{}/{}",
                        publisher.file_name().to_string_lossy(),
                        repo.file_name().to_string_lossy()
                    )));
                }
            }
        }
    }
    out
}

/// Start a runtime and wait for its API. Daemons ignore `model`; launchers are
/// spawned bound to it (no "start empty, choose later").
pub async fn start_local(name: &str, model: Option<LocalModel>) -> Result<String, String> {
    let (cmd, args, port): (&str, Vec<String>, u16) = match name {
        "Ollama" => ("ollama", vec!["serve".into()], 11434),
        "LM Studio" => ("lms", vec!["server".into(), "start".into()], 1234),
        "llama-server" => {
            let m = model.ok_or("Pick a model to launch llama-server with.")?;
            let mut a = Vec::new();
            if let Some(repo) = m.repo {
                a.push("-hf".into());
                a.push(repo);
            } else if let Some(path) = m.path {
                a.push("-m".into());
                a.push(path);
            } else {
                return Err("Model has no file path or HF repo to launch.".into());
            }
            a.push("--port".into());
            a.push("8080".into());
            ("llama-server", a, 8080)
        }
        _ => return Err(format!("{name} has no auto-start — launch it manually.")),
    };

    // Restart cleanly: kill any instance we previously started (e.g. a launcher
    // switching to a different model).
    kill_tracked(name);
    let child = std::process::Command::new(cmd)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't launch `{cmd}`: {e}"))?;
    servers().lock().unwrap().insert(name.to_string(), child);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap_or_default();
    let url = format!("http://127.0.0.1:{port}/v1/models");

    // Launchers load the whole model before answering — longer runway.
    let attempts = if name == "llama-server" { 60 } else { 20 };
    for _ in 0..attempts {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                return Ok(format!("{name} is up."));
            }
        }
    }
    Err(format!(
        "{name} didn't come up in time — check it manually."
    ))
}

/// Stop a runtime. Kills the process we spawned; for servers with a stop CLI
/// (LM Studio detaches, so its child isn't the server) use that instead.
pub async fn stop_local(name: &str) -> Result<String, String> {
    let tracked = kill_tracked(name);
    if name == "LM Studio" {
        return std::process::Command::new("lms")
            .args(["server", "stop"])
            .status()
            .map(|_| format!("{name} stopped."))
            .map_err(|e| format!("Couldn't stop {name}: {e}"));
    }
    if tracked {
        Ok(format!("{name} stopped."))
    } else {
        Err(format!("{name} wasn't started by JInk — stop it there."))
    }
}

/// Kill a tracked child if we have one. Returns whether it was tracked.
fn kill_tracked(name: &str) -> bool {
    if let Some(mut child) = servers().lock().unwrap().remove(name) {
        let _ = child.kill();
        let _ = child.wait();
        true
    } else {
        false
    }
}
