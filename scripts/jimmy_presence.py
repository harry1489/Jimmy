#!/usr/bin/env python3
"""Local, privacy-preserving face presence detector for Jimmy.

This helper intentionally performs PRESENCE detection only. It does not identify
who is in front of the camera, build a face embedding, or save camera frames.
It prints one JSON object and exits so Jimmy can treat the result as ephemeral.

Requires: Python 3 + OpenCV (cv2). The Haar cascade is loaded from OpenCV's
installed data directory when available.
"""

import json
import os
import sys
import time


def emit(state: str, detail: str = "") -> None:
    print(json.dumps({"present": state == "present", "state": state, "detail": detail}, separators=(",", ":")))


def main() -> int:
    try:
        import cv2
    except ImportError:
        emit("camera_unavailable", "python3-opencv/cv2 is not installed")
        return 2

    camera = int(os.environ.get("JIMMY_CAMERA_DEVICE", "0"))
    samples = max(1, int(os.environ.get("JIMMY_PRESENCE_SAMPLES", "3")))
    interval = max(0.05, float(os.environ.get("JIMMY_PRESENCE_INTERVAL", "0.25")))

    cascade_path = os.environ.get(
        "JIMMY_FACE_CASCADE",
        os.path.join(cv2.data.haarcascades, "haarcascade_frontalface_default.xml"),
    )
    if not os.path.exists(cascade_path):
        emit("camera_unavailable", "face detector cascade not found")
        return 2

    detector = cv2.CascadeClassifier(cascade_path)
    if detector.empty():
        emit("camera_unavailable", "could not load face detector")
        return 2

    cap = cv2.VideoCapture(camera)
    if not cap.isOpened():
        emit("camera_unavailable", "camera could not be opened")
        return 2

    seen = 0
    try:
        for _ in range(samples):
            ok, frame = cap.read()
            if ok and frame is not None:
                gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
                faces = detector.detectMultiScale(gray, scaleFactor=1.1, minNeighbors=5, minSize=(80, 80))
                if len(faces) > 0:
                    seen += 1
            time.sleep(interval)
    finally:
        cap.release()

    # No image/frame is written to disk and no biometric identity is calculated.
    if seen >= max(1, (samples + 1) // 2):
        emit("present", "person detected locally")
    else:
        emit("away", "no face detected in the sample window")
    return 0


if __name__ == "__main__":
    sys.exit(main())
