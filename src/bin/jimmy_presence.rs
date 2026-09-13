use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use opencv::{
    core::{Mat, Size, Vector},
    imgproc,
    objdetect::CascadeClassifier,
    prelude::*,
    videoio::{VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst, CAP_ANY},
};
use serde::Serialize;
use std::{env, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize)]
struct Presence {
    present: bool,
    state: String,
    detail: String,
}

type Shared = Arc<RwLock<Presence>>;

#[tokio::main]
async fn main() -> Result<()> {
    let bind = env::var("JIMMY_PRESENCE_BIND").unwrap_or_else(|_| "127.0.0.1:8791".into());
    let camera_index = env::var("JIMMY_CAMERA_DEVICE")
        .unwrap_or_else(|_| "0".into())
        .parse::<i32>()
        .context("invalid JIMMY_CAMERA_DEVICE")?;
    let interval_ms = env::var("JIMMY_PRESENCE_INTERVAL_MS")
        .unwrap_or_else(|_| "1000".into())
        .parse::<u64>()
        .context("invalid JIMMY_PRESENCE_INTERVAL_MS")?
        .max(250);
    let away_after = env::var("JIMMY_PRESENCE_AWAY_AFTER_SECONDS")
        .unwrap_or_else(|_| "30".into())
        .parse::<u64>()
        .context("invalid JIMMY_PRESENCE_AWAY_AFTER_SECONDS")?;
    let cascade_path = env::var("JIMMY_FACE_CASCADE").unwrap_or_else(|_| {
        "/usr/share/opencv4/haarcascades/haarcascade_frontalface_default.xml".into()
    });

    let state: Shared = Arc::new(RwLock::new(Presence {
        present: false,
        state: "unknown".into(),
        detail: "starting local camera detector".into(),
    }));

    let detector_state = Arc::clone(&state);
    tokio::task::spawn_blocking(move || detect_loop(detector_state, camera_index, interval_ms, away_after, cascade_path));

    let app = Router::new().route("/health", get(health)).route("/v1/state", get(state_api)).with_state(state);
    axum::serve(tokio::net::TcpListener::bind(bind).await?, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true, "service": "jimmy-presence", "mode": "local-presence-only"}))
}

async fn state_api(State(state): State<Shared>) -> Json<Presence> {
    Json(state.read().await.clone())
}

fn detect_loop(state: Shared, camera_index: i32, interval_ms: u64, away_after: u64, cascade_path: String) {
    let mut detector = match CascadeClassifier::new(&cascade_path) {
        Ok(d) => d,
        Err(e) => {
            set_state_blocking(&state, false, "camera_unavailable", format!("could not load face cascade: {e}"));
            return;
        }
    };
    let mut camera = match VideoCapture::new(camera_index, CAP_ANY) {
        Ok(c) => c,
        Err(e) => {
            set_state_blocking(&state, false, "camera_unavailable", format!("camera could not be opened: {e}"));
            return;
        }
    };
    if !camera.is_opened().unwrap_or(false) {
        set_state_blocking(&state, false, "camera_unavailable", "camera could not be opened".into());
        return;
    }

    let mut consecutive_present = 0u32;
    let mut last_seen = std::time::Instant::now() - Duration::from_secs(away_after + 1);
    loop {
        let mut frame = Mat::default();
        let detected = camera.read(&mut frame).ok().filter(|ok| *ok).is_some() && !frame.empty();
        let mut face_seen = false;
        if detected {
            let mut gray = Mat::default();
            if imgproc::cvt_color(&frame, &mut gray, imgproc::COLOR_BGR2GRAY, 0).is_ok() {
                let mut faces = Vector::<opencv::core::Rect>::new();
                if detector.detect_multi_scale(&gray, &mut faces, 1.1, 5, 0, Size::new(80, 80), Size::default()).is_ok() {
                    face_seen = !faces.is_empty();
                }
            }
        }

        if face_seen {
            consecutive_present = consecutive_present.saturating_add(1);
            last_seen = std::time::Instant::now();
        } else {
            consecutive_present = 0;
        }

        if consecutive_present >= 2 {
            set_state_blocking(&state, true, "present", "person detected locally".into());
        } else if last_seen.elapsed() >= Duration::from_secs(away_after) {
            set_state_blocking(&state, false, "away", "no face detected for the configured absence period".into());
        }
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
}

fn set_state_blocking(state: &Shared, present: bool, kind: &str, detail: String) {
    if let Ok(mut current) = state.try_write() {
        current.present = present;
        current.state = kind.into();
        current.detail = detail;
    }
}
