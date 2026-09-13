# ♡ Jimmy

Jimmy is a small, permissioned desktop companion for Gentoo Linux + Hyprland. The design goal is **agent control without giving an AI an unrestricted shell**.

Test target: Gentoo Linux x86_64, OpenRC, Hyprland 0.54.x, Wayland.

## Recommended deployment: LXC backend + main Gentoo desktop vision

For your setup, keep the **Jimmy backend services in a Proxmox LXC** and keep the **LLaVA vision model on the main Gentoo gaming PC**. This keeps desktop/vision work close to the camera and GPU while the LXC can host the backend, memory, APIs, and orchestration services.

```text
                         Proxmox host
                              │
                    ┌─────────▼─────────┐
                    │   Jimmy Backend   │
                    │       LXC         │
                    │                   │
                    │ • jimmy_backend   │
                    │ • PostgreSQL      │
                    │ • memory/API      │
                    │ • Jonathan bridge │
                    └─────────┬─────────┘
                              │ LAN
                              │
                    ┌─────────▼─────────┐
                    │ Main Gentoo PC    │
                    │                   │
                    │ • jimmy daemon   │
                    │ • LLaVA/Ollama   │
                    │ • camera/presence│
                    │ • pink Y2K UI    │
                    │ • Hyprland       │
                    └───────────────────┘
```

### Which machine runs what?

**Proxmox LXC:** backend/API services, PostgreSQL/memory, Jonathan integration, and other network services. The LXC does not need the desktop camera or Hyprland session.

**Main Gentoo PC:** Jimmy's desktop daemon, the local Rust presence detector, the pink UI, and Ollama with the **LLaVA vision model**. This is also where desktop actions happen.

The important distinction is that `llama3.2` is the normal text model, while `llava` is the separate vision model. Do not try to send images to the text-only `llama3.2` model.

## Network addresses

The repository currently uses these known LAN services:

- Text Ollama: `http://192.168.10.181:11434`
- Voice service: `http://192.168.10.182:5006`

For the recommended layout, `192.168.10.181:11434` should be the **main Gentoo PC's Ollama service**, because that is where LLaVA should run. The LXC should connect to it over the private LAN when it needs vision.

Do **not** replace every `127.0.0.1` with a LAN IP. Desktop-local services such as Jimmy, the presence detector, and the UI should remain localhost unless you deliberately add authentication and firewall rules.

## v0.3: local AI + vision + voice + presence

Jimmy uses two different Ollama roles:

- Text AI: `llama3.2`
- Vision AI: `llava:latest` (or another installed vision-capable Ollama model)

The main Gentoo PC should run Ollama and have both models available. Example:

```bash
ollama pull llama3.2
ollama pull llava:latest
ollama list
```

The voice service is separate:

- Voice service: `http://192.168.10.182:5006`
- STT: `whisper.cpp tiny.en`
- TTS: Piper
- Fixed voice: `en_US-lessac-medium.onnx`
- Speech rate: `0.9`

## 1. Create the backend LXC on the Proxmox host

Create a normal unprivileged Debian or Ubuntu LXC with a fixed LAN address. Give it enough CPU/RAM for the backend and PostgreSQL, but **do not pass the gaming GPU or webcam through to this container** if LLaVA and presence detection are staying on the main Gentoo PC.

Inside the LXC:

```bash
apt update
apt install -y git build-essential curl pkg-config libssl-dev

git clone https://github.com/harry1489/Jimmy.git
cd Jimmy
cargo build --release
```

Install the backend:

```bash
sudo install -Dm755 target/release/jimmy_backend /usr/local/bin/jimmy_backend
sudo install -Dm755 admin/jimmy-admin /usr/local/libexec/jimmy-admin
sudo install -Dm755 openrc/jimmy-backend /etc/init.d/jimmy-backend
```

If this LXC is Debian/Ubuntu rather than Gentoo, the existing OpenRC service file may need adapting to the LXC's init system. The backend binary itself does not require Hyprland.

Copy the configuration:

```bash
sudo mkdir -p /etc/jimmy
sudo install -Dm600 config/jimmy.toml.example /etc/jimmy/config.toml
```

Set the backend's Ollama/vision URL to the **main Gentoo PC**, not localhost:

```toml
ollama_url = "http://192.168.10.181:11434"
vision_url = "http://192.168.10.181:11434"
vision_model = "llava:latest"
```

The LXC can now ask the main PC's Ollama/LLaVA service for vision without running the model itself.

Test from the LXC:

```bash
curl http://192.168.10.181:11434/api/tags
```

You should see the models installed on the main Gentoo PC.

## 2. Install Ollama + LLaVA on the main Gentoo PC

The main PC is the machine with the Intel Arc B580, camera, Hyprland session, and desktop access. Run Ollama there.

Verify that Ollama is reachable locally:

```bash
curl http://127.0.0.1:11434/api/tags
```

Then install the models:

```bash
ollama pull llama3.2
ollama pull llava:latest
ollama list
```

The important part is that **LLaVA lives on this machine**. Jimmy's vision requests should point at:

```text
http://192.168.10.181:11434
```

if `192.168.10.181` is the Gentoo PC's LAN address.

If Ollama is bound only to `127.0.0.1`, the LXC will not be able to reach it. Configure Ollama to listen on the Gentoo PC's private LAN interface, and firewall it so it is reachable only from your trusted LAN/VPN. Do not expose port 11434 to the public Internet.

## 3. Install Jimmy on the main Gentoo PC

On Gentoo:

```bash
git clone https://github.com/harry1489/Jimmy.git
cd Jimmy
cargo build --release

sudo install -Dm755 target/release/jimmy /usr/local/bin/jimmy
sudo install -Dm755 target/release/jimmy_presence /usr/local/bin/jimmy_presence
sudo install -Dm755 target/release/jimmy_ui /usr/local/bin/jimmy_ui
sudo mkdir -p /etc/jimmy
sudo install -Dm600 config/jimmy.toml.example /etc/jimmy/config.toml
```

Generate a real secret and edit the configuration before starting services:

```bash
sudo sed -i "s/REPLACE_WITH_A_64_HEX_CHARACTER_SECRET/$(openssl rand -hex 32)/" /etc/jimmy/config.toml
sudo chown harry:users /etc/jimmy/config.toml
sudo chmod 600 /etc/jimmy/config.toml
```

For this deployment, the important configuration is:

```toml
# Main text AI / Ollama on the Gentoo PC
ollama_url = "http://192.168.10.181:11434"
ollama_model = "llama3.2"

# Vision is also on the Gentoo PC
vision_url = "http://192.168.10.181:11434"
vision_model = "llava:latest"

# Desktop-local services stay local
presence_bind = "127.0.0.1:8791"
ui_bind = "127.0.0.1:8788"

# Remote voice service
voice_url = "http://192.168.10.182:5006"
voice_status_url = "http://192.168.10.182:5006/health"
```

The desktop daemon should stay on localhost unless you intentionally build a private authenticated network endpoint for it.

## 4. Install the local presence detector and UI

Jimmy's presence detector is Rust + OpenCV. It uses a local Haar-cascade face detector only to determine whether somebody is in front of the PC. It does **not** identify the person, store face embeddings, or send camera frames to LLaVA.

Install the OpenRC services:

```bash
sudo install -Dm755 openrc/jimmy /etc/init.d/jimmy
sudo install -Dm755 openrc/jimmy-presence /etc/init.d/jimmy-presence
sudo install -Dm755 openrc/jimmy-ui /etc/init.d/jimmy-ui

sudo rc-update add jimmy default
sudo rc-update add jimmy-presence default
sudo rc-update add jimmy-ui default

sudo rc-service jimmy-presence start
sudo rc-service jimmy-ui start
sudo rc-service jimmy start
```

Make sure the desktop user can access the camera, normally through the `video` group.

The local UI is:

```text
http://127.0.0.1:8788
```

It shows the pink Y2K Jimmy interface, local presence state, and the active state when the UI receives the `Hey Jimmy` wake event.

## 5. Start the backend LXC

Once the main Gentoo PC's Ollama/LLaVA endpoint is reachable from the LXC, start the backend service there.

The backend should use the main PC as its vision provider:

```text
LXC backend
    │
    │ POST vision request
    ▼
192.168.10.181:11434
    │
    ▼
LLaVA on main Gentoo PC
```

This means you only maintain the LLaVA model on the gaming PC instead of duplicating the large model inside the LXC.

## 6. Final recommended layout

```text
Proxmox host
│
└── Jimmy LXC
    ├── jimmy_backend
    ├── PostgreSQL / memory
    ├── Jonathan integration
    └── other backend APIs
          │
          │ private LAN
          ▼
Main Gentoo gaming PC
├── Ollama :11434
│   ├── llama3.2       ← text
│   └── llava:latest   ← vision
├── jimmy :8787
├── jimmy_presence :8791
├── jimmy_ui :8788
├── Hyprland / Wayland
├── camera
└── desktop applications
          │
          └── Voice service: 192.168.10.182:5006
```

This is the cleanest split for your setup: **the LXC is the backend brain/services, while the main Gentoo PC owns the actual vision model and desktop hands.**

## Desktop capabilities

- Run as an unprivileged OpenRC service.
- Authenticate main-agent requests with HMAC-SHA256.
- Lock the session with `loginctl lock-session`.
- Open HTTP(S) URLs with `xdg-open`.
- Launch only explicitly allowlisted applications.
- Inspect Hyprland clients.
- Focus a specific Hyprland window by address.
- Switch workspaces 1..99.
- Take screenshots into `/tmp` using `grim`.
- Require confirmation for dangerous/sensitive actions.
- Store the original arguments with pending confirmations and expire them automatically.
- Refuse arbitrary shell commands.
- Refuse mouse/keyboard injection until its Linux permissions are explicitly configured.

## Security model

Jimmy is intentionally **not** a root daemon. Keep desktop-local services bound to `127.0.0.1` unless you deliberately add a private authenticated network layer.

Do not expose Ollama port 11434, Jimmy port 8787, the UI, or backend administration endpoints to the public Internet. Use a firewall and preferably a private VPN/mesh network for machine-to-machine traffic.

The main AI should never be granted unrestricted shell access. Admin operations should remain fixed allowlisted actions, authenticated, and subject to the confirmation flow.

## Voice / "Hey Jimmy"

The configured voice stack is designed around:

```text
microphone
  ↓
wake-word detector ("Hey Jimmy")
  ↓
whisper.cpp tiny.en
  ↓
Ollama llama3.2
  ↓
structured Jimmy action / response
  ↓
permission engine
  ↓
Piper en_US-lessac-medium.onnx
```

The current Rust daemon records and reports the voice configuration and can health-check the service at port 5006. It does not assume an undocumented audio API for that service.

The UI has a local wake endpoint:

```http
POST http://127.0.0.1:8788/api/wake
Content-Type: application/json

{"phrase":"Hey Jimmy"}
```

The remote voice service still needs a deliberate authenticated event bridge to call this endpoint. Do not expose the unauthenticated local UI endpoint directly to the LAN.

## Main AI / Jonathan / remote control

The intended approval path is:

```text
Jimmy needs approval
        ↓
check local presence
   ┌────┴────┐
   │         │
present    away/unknown
   │         │
ask user   ask Jonathan
locally       ↓
          Jonathan asks user remotely
                ↓
          authenticated approval
                ↓
              Jimmy
```

Jonathan should act as the authenticated orchestrator/messenger. A spoken phrase must never by itself become proof of administrator authority.

Pending approvals should be short-lived, one-time, tied to the exact requested action and arguments, and rejected after expiry.

## Future input-control module

Mouse/keyboard control is deliberately disabled. When it is added, use a narrowly scoped helper backed by Linux `uinput`/`ydotool` permissions rather than making Jimmy root. Give input control its own permission class and require confirmation by default.

## System profile used for the initial design

- Gentoo Linux x86_64
- OpenRC
- Hyprland 0.54.0 / Wayland
- Intel Core i7-8700
- Intel Arc B580
- 32 GiB RAM
- Fish 4.6
- Alacritty 0.16.1
- 1920x1080 @ 240 Hz

## License

MIT
