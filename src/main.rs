use anyhow::{bail, Context, Result};
use axum::{extract::{Path, State}, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::{get, post}, Json, Router};
use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{collections::HashMap, process::Command, sync::Arc};
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
type AppState = Arc<RwLock<Jimmy>>;

#[derive(Parser, Debug)]
#[command(name = "jimmy")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
    #[arg(long, default_value = "/etc/jimmy/config.toml")]
    config: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default = "default_bind")] bind: String,
    #[serde(default)] shared_secret: String,
    #[serde(default = "default_name")] name: String,
    #[serde(default = "default_confirm")] require_confirmation_for: Vec<String>,
    #[serde(default)] allowed_apps: Vec<String>,
    #[serde(default = "default_ollama_url")] ollama_url: String,
    #[serde(default = "default_ollama_model")] ollama_model: String,
    #[serde(default = "default_system_prompt")] ollama_system_prompt: String,
    #[serde(default = "default_voice_url")] voice_url: String,
    #[serde(default = "default_voice_status_url")] voice_status_url: String,
    #[serde(default = "default_voice_engine")] voice_engine: String,
    #[serde(default = "default_fixed_voice")] fixed_voice: String,
    #[serde(default)] loaded_voices: Vec<String>,
    #[serde(default = "default_voice_port")] voice_port: u16,
    #[serde(default = "default_speech_rate")] speech_rate: f32,
    #[serde(default = "default_voice_mode")] voice_mode: String,
    #[serde(default = "default_stt_ready")] stt_ready: bool,
    #[serde(default)] piper: bool,
    #[serde(default = "default_confirmation_ttl")] confirmation_ttl_seconds: i64,
}

fn default_bind() -> String { "127.0.0.1:8787".into() }
fn default_name() -> String { "Jimmy".into() }
fn default_confirm() -> Vec<String> { vec!["close_app".into(), "type_text".into(), "mouse_click".into(), "shutdown".into(), "reboot".into()] }
fn default_ollama_url() -> String { "http://192.168.10.181:11434".into() }
fn default_ollama_model() -> String { "llama3.2".into() }
fn default_system_prompt() -> String { "You are Jimmy, a helpful desktop AI companion. Never claim a desktop action happened unless Jimmy's action API completed it. Ask for confirmation for sensitive actions. Be concise, friendly, and practical.".into() }
fn default_voice_url() -> String { "http://127.0.0.1:5006".into() }
fn default_voice_status_url() -> String { "http://127.0.0.1:5006/health".into() }
fn default_voice_engine() -> String { "whisper.cpp tiny.en".into() }
fn default_fixed_voice() -> String { "en_US-lessac-medium.onnx".into() }
fn default_voice_port() -> u16 { 5006 }
fn default_speech_rate() -> f32 { 0.9 }
fn default_voice_mode() -> String { "fixed voice".into() }
fn default_stt_ready() -> bool { true }
fn default_confirmation_ttl() -> i64 { 120 }

#[derive(Debug)]
struct Jimmy { config: Config, pending: HashMap<String, PendingAction>, http: Client }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingAction { id: String, action: String, args: serde_json::Value, created_at: String, reason: Option<String>, source: String }

#[derive(Debug, Deserialize, Serialize)]
struct ActionRequest { action: String, #[serde(default)] args: serde_json::Value, #[serde(default)] reason: Option<String>, #[serde(default)] source: String }

#[derive(Debug, Serialize)]
struct ActionResponse { ok: bool, status: String, message: String, request_id: String, confirmation_required: bool }

#[derive(Debug, Deserialize)]
struct ChatRequest { messages: Vec<ChatMessage>, #[serde(default)] temperature: Option<f32> }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage { role: String, content: String }

#[derive(Debug, Serialize)]
struct OllamaChatRequest { model: String, messages: Vec<ChatMessage>, stream: bool, options: OllamaOptions }

#[derive(Debug, Serialize)]
struct OllamaOptions { #[serde(skip_serializing_if = "Option::is_none")] temperature: Option<f32> }

#[derive(Debug, Deserialize)]
struct OllamaChatResponse { message: ChatMessage }

#[derive(Debug, Serialize)]
struct VoiceConfigResponse { engine: String, fixed_voice: String, loaded_voices: Vec<String>, piper: bool, port: u16, speech_rate: f32, status: String, stt_ready: bool, voice_mode: String }

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();
    let raw = tokio::fs::read_to_string(&args.config).await.with_context(|| format!("cannot read {}", args.config))?;
    let config: Config = toml::from_str(&raw).context("invalid Jimmy config")?;
    if config.shared_secret.len() < 32 { bail!("shared_secret must be at least 32 characters"); }
    let bind = if config.bind.is_empty() { args.bind } else { config.bind.clone() };
    let http = Client::builder().connect_timeout(std::time::Duration::from_secs(3)).timeout(std::time::Duration::from_secs(120)).build()?;
    let state: AppState = Arc::new(RwLock::new(Jimmy { config, pending: HashMap::new(), http }));
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/action", post(action))
        .route("/v1/confirm/:id", post(confirm))
        .route("/v1/ai/chat", post(ai_chat))
        .route("/v1/voice/config", get(voice_config))
        .route("/v1/voice/status", get(voice_status))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("Jimmy listening on {}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse { Json(serde_json::json!({"ok": true, "name": "Jimmy", "version": "0.2.0", "ai_model": "llama3.2"})) }

async fn status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = authenticate(&state, &headers, b"status").await { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok": false, "error": e.to_string()}))).into_response(); }
    let hypr = run("hyprctl", &["activewindow", "-j"]).unwrap_or_else(|_| "{}".into());
    let uptime = run("uptime", &["-p"]).unwrap_or_default();
    let cfg = state.read().await.config.clone();
    Json(serde_json::json!({"ok": true, "name": cfg.name, "ai": {"provider":"ollama","url":cfg.ollama_url,"model":cfg.ollama_model}, "voice": voice_config_value(&cfg), "hyprland": serde_json::from_str::<serde_json::Value>(&hypr).unwrap_or(serde_json::json!({})), "uptime": uptime})).into_response()
}

async fn action(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<ActionRequest>) -> impl IntoResponse {
    let payload = serde_json::to_vec(&req).unwrap_or_default();
    if let Err(e) = authenticate(&state, &headers, &payload).await { return (StatusCode::UNAUTHORIZED, Json(ActionResponse { ok:false, status:"denied".into(), message:e.to_string(), request_id:Uuid::new_v4().to_string(), confirmation_required:false })).into_response(); }
    let request_id = Uuid::new_v4().to_string();
    let mut jimmy = state.write().await;
    let confirmation = jimmy.config.require_confirmation_for.iter().any(|x| x == &req.action);
    if confirmation {
        let pending = PendingAction { id: request_id.clone(), action:req.action.clone(), args:req.args.clone(), created_at:Utc::now().to_rfc3339(), reason:req.reason.clone(), source:req.source.clone() };
        jimmy.pending.insert(request_id.clone(), pending);
        info!(request_id=%request_id, source=%req.source, action=%req.action, "confirmation required");
        return Json(ActionResponse { ok:true, status:"pending_confirmation".into(), message:format!("{} requires your confirmation", req.action), request_id, confirmation_required:true }).into_response();
    }
    let result = execute(&jimmy.config, &req.action, &req.args).await;
    match result {
        Ok(msg) => Json(ActionResponse { ok:true, status:"completed".into(), message:msg, request_id, confirmation_required:false }).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(ActionResponse { ok:false, status:"failed".into(), message:e.to_string(), request_id, confirmation_required:false })).into_response(),
    }
}

async fn confirm(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> impl IntoResponse {
    if let Err(e) = authenticate(&state, &headers, id.as_bytes()).await { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok":false,"error":e.to_string()}))).into_response(); }
    let pending = { state.write().await.pending.remove(&id) };
    let Some(pending) = pending else { return (StatusCode::NOT_FOUND, Json(serde_json::json!({"ok":false,"error":"confirmation not found or expired"}))).into_response(); };
    let cfg = { state.read().await.config.clone() };
    let created = DateTime::parse_from_rfc3339(&pending.created_at).ok().map(|d| d.with_timezone(&Utc));
    if created.map(|t| Utc::now() - t > Duration::seconds(cfg.confirmation_ttl_seconds)).unwrap_or(true) { return (StatusCode::GONE, Json(serde_json::json!({"ok":false,"error":"confirmation expired"}))).into_response(); }
    match execute(&cfg, &pending.action, &pending.args).await {
        Ok(message) => Json(serde_json::json!({"ok":true,"status":"completed","message":message,"request_id":id})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok":false,"status":"failed","error":e.to_string()}))).into_response(),
    }
}

async fn ai_chat(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<ChatRequest>) -> impl IntoResponse {
    let payload = serde_json::to_vec(&req).unwrap_or_default();
    if let Err(e) = authenticate(&state, &headers, &payload).await { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok":false,"error":e.to_string()}))).into_response(); }
    if req.messages.is_empty() { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok":false,"error":"messages cannot be empty"}))).into_response(); }
    let (cfg, client) = { let s = state.read().await; (s.config.clone(), s.http.clone()) };
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    messages.push(ChatMessage { role: "system".into(), content: cfg.ollama_system_prompt.clone() });
    messages.extend(req.messages);
    let body = OllamaChatRequest { model: cfg.ollama_model.clone(), messages, stream:false, options:OllamaOptions { temperature:req.temperature } };
    let url = format!("{}/api/chat", cfg.ollama_url.trim_end_matches('/'));
    match client.post(url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<OllamaChatResponse>().await {
            Ok(answer) => Json(serde_json::json!({"ok":true,"provider":"ollama","model":cfg.ollama_model,"message":answer.message})).into_response(),
            Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"ok":false,"error":format!("invalid Ollama response: {}", e)}))).into_response(),
        },
        Ok(resp) => { let status = resp.status(); let text = resp.text().await.unwrap_or_default(); (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"ok":false,"error":format!("Ollama returned {}: {}", status, text)}))).into_response() }
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"ok":false,"error":format!("cannot reach Ollama: {}", e)}))).into_response(),
    }
}

async fn voice_config(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = authenticate(&state, &headers, b"voice-config").await { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok":false,"error":e.to_string()}))).into_response(); }
    let cfg = state.read().await.config.clone();
    Json(serde_json::json!({"ok":true,"status":"ok","voice":voice_config_value(&cfg)})).into_response()
}

async fn voice_status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = authenticate(&state, &headers, b"voice-status").await { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok":false,"error":e.to_string()}))).into_response(); }
    let (cfg, client) = { let s = state.read().await; (s.config.clone(), s.http.clone()) };
    match client.get(&cfg.voice_status_url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(value) => Json(serde_json::json!({"ok":true,"configured":voice_config_value(&cfg),"service":value})).into_response(),
            Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"ok":false,"error":format!("invalid voice service response: {}", e)}))).into_response(),
        },
        Ok(resp) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"ok":false,"error":format!("voice service returned {}", resp.status())}))).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"ok":false,"error":format!("cannot reach voice service: {}", e)}))).into_response(),
    }
}

fn voice_config_value(cfg: &Config) -> VoiceConfigResponse { VoiceConfigResponse { engine:cfg.voice_engine.clone(), fixed_voice:cfg.fixed_voice.clone(), loaded_voices:cfg.loaded_voices.clone(), piper:cfg.piper, port:cfg.voice_port, speech_rate:cfg.speech_rate, status:"ok".into(), stt_ready:cfg.stt_ready, voice_mode:cfg.voice_mode.clone() } }

async fn authenticate(state: &AppState, headers: &HeaderMap, body: &[u8]) -> Result<()> {
    let signature = headers.get("x-jimmy-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    let secret = state.read().await.config.shared_secret.clone();
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());
    if signature.len() != expected.len() || !constant_time_eq(signature.as_bytes(), expected.as_bytes()) { bail!("invalid signature"); }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool { if a.len() != b.len() { return false; } a.iter().zip(b).fold(0u8, |acc, (x,y)| acc | (x ^ y)) == 0 }

async fn execute(cfg: &Config, action: &str, args: &serde_json::Value) -> Result<String> {
    match action {
        "lock" => { run("loginctl", &["lock-session"])?; Ok("session locked".into()) }
        "open_url" => { let url = args.get("url").and_then(|v|v.as_str()).context("url required")?; if !(url.starts_with("https://") || url.starts_with("http://")) { bail!("only http/https URLs are allowed"); } run("xdg-open", &[url])?; Ok(format!("opened {}", url)) }
        "open_app" => { let app = args.get("app").and_then(|v|v.as_str()).context("app required")?; if !cfg.allowed_apps.iter().any(|x| x == app) { bail!("application is not allowlisted"); } run(app, &[])?; Ok(format!("started {}", app)) }
        "close_app" => bail!("close_app is not implemented yet; no arbitrary process killing is allowed"),
        "get_windows" => Ok(run("hyprctl", &["clients", "-j"])?),
        "focus_window" => { let address = args.get("address").and_then(|v|v.as_str()).context("Hyprland window address required")?; if !address.starts_with("0x") || address.len() > 32 { bail!("invalid Hyprland address"); } run("hyprctl", &["dispatch", "focuswindow", &format!("address:{}", address)])?; Ok("window focused".into()) }
        "workspace" => { let workspace = args.get("workspace").and_then(|v|v.as_i64()).context("workspace required")?; if !(1..=99).contains(&workspace) { bail!("workspace must be 1..99"); } run("hyprctl", &["dispatch", "workspace", &workspace.to_string()])?; Ok(format!("moved to workspace {}", workspace)) }
        "screenshot" => { let path = args.get("path").and_then(|v|v.as_str()).unwrap_or("/tmp/jimmy-screenshot.png"); if !path.starts_with("/tmp/") { bail!("screenshots are restricted to /tmp"); } run("grim", &[path])?; Ok(path.into()) }
        "shutdown" => { run("loginctl", &["poweroff"])?; Ok("shutdown requested".into()) }
        "reboot" => { run("loginctl", &["reboot"])?; Ok("reboot requested".into()) }
        "type_text" | "mouse_click" => bail!("input control is intentionally disabled until ydotool/uinput permissions are explicitly configured"),
        _ => bail!("unknown action: {}", action),
    }
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program).args(args).output().with_context(|| format!("failed to execute {}", program))?;
    if !out.status.success() { bail!("{} failed: {}", program, String::from_utf8_lossy(&out.stderr).trim()); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
