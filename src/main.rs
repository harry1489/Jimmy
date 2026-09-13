use anyhow::{bail, Context, Result};
use axum::{extract::State, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::{get, post}, Json, Router};
use chrono::Utc;
use clap::Parser;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{collections::HashMap, net::SocketAddr, process::Command, sync::Arc};
use tokio::sync::RwLock;
use tracing::{error, info};
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
    #[serde(default = "default_bind")]
    bind: String,
    #[serde(default)]
    shared_secret: String,
    #[serde(default = "default_name")]
    name: String,
    #[serde(default = "default_confirm")]
    require_confirmation_for: Vec<String>,
    #[serde(default)]
    allowed_apps: Vec<String>,
}
fn default_bind() -> String { "127.0.0.1:8787".into() }
fn default_name() -> String { "Jimmy".into() }
fn default_confirm() -> Vec<String> { vec!["close_app".into(), "type_text".into(), "mouse_click".into(), "shutdown".into(), "reboot".into()] }

#[derive(Debug)]
struct Jimmy {
    config: Config,
    pending: HashMap<String, PendingAction>,
}

#[derive(Debug, Clone, Serialize)]
struct PendingAction {
    id: String,
    action: String,
    created_at: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActionRequest {
    action: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    source: String,
}

#[derive(Debug, Serialize)]
struct ActionResponse {
    ok: bool,
    status: String,
    message: String,
    request_id: String,
    confirmation_required: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();
    let raw = tokio::fs::read_to_string(&args.config).await
        .with_context(|| format!("cannot read {}", args.config))?;
    let config: Config = toml::from_str(&raw).context("invalid Jimmy config")?;
    if config.shared_secret.len() < 32 {
        bail!("shared_secret must be at least 32 characters");
    }
    let bind = if config.bind.is_empty() { args.bind } else { config.bind.clone() };
    let state: AppState = Arc::new(RwLock::new(Jimmy { config, pending: HashMap::new() }));

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/action", post(action))
        .route("/v1/confirm/:id", post(confirm))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!("Jimmy listening on {}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "name": "Jimmy"}))
}

async fn status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = authenticate(&state, &headers, b"status") .await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok": false, "error": e.to_string()}))).into_response();
    }
    let hypr = run("hyprctl", &["activewindow", "-j"]).unwrap_or_else(|_| "{}".into());
    let uptime = run("uptime", &["-p"]).unwrap_or_default();
    Json(serde_json::json!({"ok": true, "hyprland": serde_json::from_str::<serde_json::Value>(&hypr).unwrap_or(serde_json::json!({})), "uptime": uptime})).into_response()
}

async fn action(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<ActionRequest>) -> impl IntoResponse {
    let payload = serde_json::to_vec(&req).unwrap_or_default();
    if let Err(e) = authenticate(&state, &headers, &payload).await {
        return (StatusCode::UNAUTHORIZED, Json(ActionResponse { ok:false, status:"denied".into(), message:e.to_string(), request_id:Uuid::new_v4().to_string(), confirmation_required:false })).into_response();
    }
    let request_id = Uuid::new_v4().to_string();
    let mut jimmy = state.write().await;
    let confirmation = jimmy.config.require_confirmation_for.iter().any(|x| x == &req.action);
    if confirmation {
        let pending = PendingAction { id: request_id.clone(), action:req.action.clone(), created_at:Utc::now().to_rfc3339(), reason:req.reason.clone() };
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

async fn confirm(State(state): State<AppState>, headers: HeaderMap, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    if let Err(e) = authenticate(&state, &headers, id.as_bytes()).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"ok":false,"error":e.to_string()}))).into_response();
    }
    let pending = { state.write().await.pending.remove(&id) };
    let Some(pending) = pending else { return (StatusCode::NOT_FOUND, Json(serde_json::json!({"ok":false,"error":"confirmation not found or expired"}))).into_response(); };
    let args = serde_json::json!({"id": pending.id});
    let cfg = { state.read().await.config.clone() };
    match execute(&cfg, &pending.action, &args).await {
        Ok(message) => Json(serde_json::json!({"ok":true,"status":"completed","message":message,"request_id":id})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"ok":false,"status":"failed","error":e.to_string()}))).into_response(),
    }
}

async fn authenticate(state: &AppState, headers: &HeaderMap, body: &[u8]) -> Result<()> {
    let signature = headers.get("x-jimmy-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    let secret = state.read().await.config.shared_secret.clone();
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());
    if signature.len() != expected.len() || !constant_time_eq(signature.as_bytes(), expected.as_bytes()) { bail!("invalid signature"); }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b).fold(0u8, |acc, (x,y)| acc | (x ^ y)) == 0
}

async fn execute(cfg: &Config, action: &str, args: &serde_json::Value) -> Result<String> {
    match action {
        "lock" => { run("loginctl", &["lock-session"])?; Ok("session locked".into()) }
        "open_url" => {
            let url = args.get("url").and_then(|v|v.as_str()).context("url required")?;
            if !(url.starts_with("https://") || url.starts_with("http://")) { bail!("only http/https URLs are allowed"); }
            run("xdg-open", &[url])?; Ok(format!("opened {}", url))
        }
        "open_app" => {
            let app = args.get("app").and_then(|v|v.as_str()).context("app required")?;
            if !cfg.allowed_apps.iter().any(|x| x == app) { bail!("application is not allowlisted"); }
            run(app, &[])?; Ok(format!("started {}", app))
        }
        "close_app" => bail!("close_app confirmation must be executed through a future constrained app-specific implementation"),
        "get_windows" => Ok(run("hyprctl", &["clients", "-j"])?),
        "focus_window" => {
            let address = args.get("address").and_then(|v|v.as_str()).context("Hyprland window address required")?;
            if !address.starts_with("0x") || address.len() > 32 { bail!("invalid Hyprland address"); }
            run("hyprctl", &["dispatch", "focuswindow", &format!("address:{}", address)])?; Ok("window focused".into())
        }
        "workspace" => {
            let workspace = args.get("workspace").and_then(|v|v.as_i64()).context("workspace required")?;
            if !(1..=99).contains(&workspace) { bail!("workspace must be 1..99"); }
            run("hyprctl", &["dispatch", "workspace", &workspace.to_string()])?; Ok(format!("moved to workspace {}", workspace))
        }
        "screenshot" => {
            let path = args.get("path").and_then(|v|v.as_str()).unwrap_or("/tmp/jimmy-screenshot.png");
            if !path.starts_with("/tmp/") { bail!("screenshots are restricted to /tmp in v0.1"); }
            run("grim", &[path])?; Ok(path.into())
        }
        "shutdown" => { run("loginctl", &["poweroff"])?; Ok("shutdown requested".into()) }
        "reboot" => { run("loginctl", &["reboot"])?; Ok("reboot requested".into()) }
        "type_text" | "mouse_click" => bail!("input control is intentionally disabled in v0.1 until ydotool/uinput permissions are explicitly configured"),
        _ => bail!("unknown action: {}", action),
    }
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program).args(args).output().with_context(|| format!("failed to execute {}", program))?;
    if !out.status.success() { bail!("{} failed: {}", program, String::from_utf8_lossy(&out.stderr).trim()); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
