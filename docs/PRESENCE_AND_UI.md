# Jimmy local presence + pink wake UI

## Face detection

Jimmy's presence detector is now Rust, not Python.

It uses the Rust `opencv` bindings with OpenCV's `objdetect` module and the standard Haar cascade:

`haarcascade_frontalface_default.xml`

The detector answers one question only: **does a person appear to be in front of the camera?**

It does not:

- identify Harry
- create face embeddings
- perform face recognition
- send camera frames to Ollama
- send camera frames to Jonathan
- save camera frames
- use presence as authentication

The detector runs on the gaming PC and exposes only a small local state API:

`GET http://127.0.0.1:8791/v1/state`

Possible states are `present`, `away`, `unknown`, and `camera_unavailable`.

The default absence hysteresis is 30 seconds so one missed frame does not immediately mark Harry as away.

## Pink Y2K UI

The Rust UI server runs on:

`http://127.0.0.1:8788`

It serves the local pink/glossy Y2K interface in `ui/index.html`.

The UI shows:

- Jimmy's pink listening orb
- a visible `Hey Jimmy` listening state
- a wake animation when the wake event arrives
- local presence state
- voice/vision/privacy status
- an explicit note that presence is not authentication

## Hey Jimmy event

The current voice service already owns the microphone/wake-word side. Because its exact wake-event API is not part of Jimmy's documented contract yet, Jimmy does not invent an endpoint on that service.

Instead, once the voice service detects `Hey Jimmy`, it should POST:

```json
{"phrase":"Hey Jimmy"}
```

to:

`POST http://127.0.0.1:8788/api/wake`

That causes the pink orb to glow and the UI to display `Hi! I heard “Hey Jimmy”` for several seconds.

If the voice service lives on another machine, the UI endpoint must be deliberately exposed through a private/authenticated LAN path before using it remotely. Do not expose the UI wake endpoint to the public Internet.

## OpenRC

Install the binaries:

```sh
sudo install -Dm755 target/release/jimmy_presence /usr/local/bin/jimmy_presence
sudo install -Dm755 target/release/jimmy_ui /usr/local/bin/jimmy_ui
sudo install -Dm755 openrc/jimmy-presence /etc/init.d/jimmy-presence
sudo install -Dm755 openrc/jimmy-ui /etc/init.d/jimmy-ui
```

Then:

```sh
sudo rc-update add jimmy-presence default
sudo rc-update add jimmy-ui default
sudo rc-service jimmy-presence start
sudo rc-service jimmy-ui start
```

For camera access, the service user needs permission to read the V4L2 camera device, normally through the `video` group.

## Build requirements

The Rust OpenCV binding requires a supported OpenCV 4.x/5.x installation and Clang/libclang for binding generation. The exact Gentoo USE flags/package split can vary, so install the OpenCV and Clang development packages appropriate for the current Gentoo tree before running `cargo build --release`.
