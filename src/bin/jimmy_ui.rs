use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::{Duration, Instant}};
use tokio::sync::RwLock;

const INDEX: &str = include_str!("../../ui/index.html");
type Shared = Arc<RwLock<UiState>>;

#[derive(Debug, Default)]
struct UiState { listening: bool, last_wake: Option<Instant> }

#[derive(Debug, Deserialize)]
struct WakeRequest { phrase: Option<String> }

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Presence { present: bool, state: String, detail: String }

#[derive(Debug, Serialize)]
struct UiResponse {
    listening: bool,
    active: bool,
    phrase: Option<String>,
    presence: Presence,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bind: SocketAddr = std::env::var("JIMMY_UI_BIND").unwrap_or_else(|_| "127.0.0.1:8788".into()).parse()?;
    let state: Shared = Arc::new(RwLock::new(UiState::default()));
    let app = Router::new()
        .route("/", get(index))
        .route("/api/state", get(state_api))
        .route("/api/wake", post(wake))
        .route("/api/listening", post(listening))
        .with_state(state);
    println!("Jimmy UI listening on http://{bind}");
    axum::serve(tokio::net::TcpListener::bind(bind).await?, app).await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    ([ ("content-type", "text/html; charset=utf-8") ], INDEX).into_response()
}

async fn state_api(State(state): State<Shared>) -> Json<UiResponse> {
    let s = state.read().await;
    let active = s.last_wake.map(|t| t.elapsed() < Duration::from_secs(6)).unwrap_or(false);
    let presence = local_presence().await;
    Json(UiResponse { listening: s.listening, active, phrase: active.then(|| "Hey Jimmy".into()), presence })
}

async fn wake(State(state): State<Shared>, Json(req): Json<WakeRequest>) -> Json<UiResponse> {
    state.write().await.last_wake = Some(Instant::now());
    let presence = local_presence().await;
    Json(UiResponse { listening: true, active: true, phrase: Some(req.phrase.unwrap_or_else(|| "Hey Jimmy".into())), presence })
}

async fn listening(State(state): State<Shared>) -> impl IntoResponse {
    state.write().await.listening = true;
    (StatusCode::NO_CONTENT, "").into_response()
}

async fn local_presence() -> Presence {
    match Client::new().get("http://127.0.0.1:8791/v1/state").timeout(Duration::from_millis(300)).send().await {
        Ok(r) if r.status().is_success() => r.json::<Presence>().await.unwrap_or_else(|_| unavailable("invalid presence response")),
        Ok(_) => unavailable("presence service unavailable"),
        Err(_) => unavailable("presence service unavailable"),
    }
}

fn unavailable(detail: &str) -> Presence {
    Presence { present: false, state: "camera_unavailable".into(), detail: detail.into() }
}
