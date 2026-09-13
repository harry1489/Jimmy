use anyhow::{Context, Result};
use opencv::{
    core::{Mat, Size, Vector},
    imgproc,
    objdetect::CascadeClassifier,
    prelude::*,
    videoio::{self, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst, CAP_ANY},
};
use serde::Serialize;
use std::{env, thread, time::Duration};

#[derive(Debug, Serialize)]
struct Presence {
    present: bool,
    state: &'static str,
    detail: String,
}

fn emit(state: &'static str, detail: impl Into<String>) {
    let p = Presence {
        present: state == "present",
        state,
        detail: detail.into(),
    };
    println!("{}", serde_json::to_string(&p).expect("presence JSON"));
}

fn main() -> Result<()> {
    let camera = env::var("JIMMY_CAMERA_DEVICE")
        .unwrap_or_else(|_| "0".into())
        .parse::<i32>()
        .context("invalid JIMMY_CAMERA_DEVICE")?;
    let samples = env::var("JIMMY_PRESENCE_SAMPLES")
        .unwrap_or_else(|_| "3".into())
        .parse::<usize>()
        .context("invalid JIMMY_PRESENCE_SAMPLES")?
        .max(1);
    let interval_ms = env::var("JIMMY_PRESENCE_INTERVAL_MS")
        .unwrap_or_else(|_| "250".into())
        .parse::<u64>()
        .context("invalid JIMMY_PRESENCE_INTERVAL_MS")?
        .max(50);
    let cascade_path = env::var("JIMMY_FACE_CASCADE").unwrap_or_else(|_| {
        "/usr/share/opencv4/haarcascades/haarcascade_frontalface_default.xml".into()
    });

    let mut detector = match CascadeClassifier::new(&cascade_path) {
        Ok(d) => d,
        Err(e) => {
            emit("camera_unavailable", format!("could not load face cascade: {e}"));
            return Ok(());
        }
    };

    let mut camera = match VideoCapture::new(camera, CAP_ANY) {
        Ok(c) => c,
        Err(e) => {
            emit("camera_unavailable", format!("camera could not be opened: {e}"));
            return Ok(());
        }
    };

    if !camera.is_opened()? {
        emit("camera_unavailable", "camera could not be opened");
        return Ok(());
    }

    let mut seen = 0usize;
    for _ in 0..samples {
        let mut frame = Mat::default();
        if camera.read(&mut frame)? && !frame.empty() {
            let mut gray = Mat::default();
            imgproc::cvt_color(&frame, &mut gray, imgproc::COLOR_BGR2GRAY, 0)?;
            let mut faces = Vector::<opencv::core::Rect>::new();
            detector.detect_multi_scale(
                &gray,
                &mut faces,
                1.1,
                5,
                0,
                Size::new(80, 80),
                Size::default(),
            )?;
            if !faces.is_empty() {
                seen += 1;
            }
        }
        thread::sleep(Duration::from_millis(interval_ms));
    }

    if seen >= (samples + 1) / 2 {
        emit("present", "person detected locally");
    } else {
        emit("away", "no face detected in the sample window");
    }

    Ok(())
}
