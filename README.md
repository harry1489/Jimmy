# ♡ Jimmy

Jimmy is a small, permissioned desktop companion for Gentoo Linux + Hyprland. The design goal is **agent control without giving an AI an unrestricted shell**.

Test target: Gentoo Linux x86_64, OpenRC, Hyprland 0.54.x, Wayland.

## Recommended deployment: LXC backend + main Gentoo desktop vision

For your setup, keep the **Jimmy backend services in a Proxmox LXC** and keep the **LLaVA vision model on the main Gentoo gaming PC**.

```text
Proxmox host
└── Jimmy LXC
    ├── jimmy_backend
    ├── PostgreSQL / memory
    └── Jonathan/backend services
             │
             │ private LAN
             ▼
Main Gentoo PC
├── Ollama :11434
│   ├── llama3.2       ← text
│   └── llava:latest   ← vision
├── Jimmy :8787
├── presence :8791
├── UI :8788
└── Hyprland / desktop
```

## Creating the LXC from the Proxmox CLI

Run these commands **on the Proxmox host**, not inside the future container.

First find the storage, network bridge, and available container templates:

```bash
pvesm status
ip -br link
pveam update
pveam available --section system | grep -E 'debian|ubuntu'
```

On current Proxmox template lists, use the exact Debian template filename shown by `pveam available`. For example, if the host reports `debian-13-standard_13.6-1_amd64.tar.zst`:

```bash
pveam download local debian-13-standard_13.6-1_amd64.tar.zst
```

If `pveam download` says the file already exists and has the correct checksum, that is fine.

Find the downloaded template:

```bash
pveam list local
```

### Important: rootfs storage must support container volumes

Do **not** assume `local` can be used for `--rootfs`. On many Proxmox installations, `local` is a `dir` storage that supports container templates but is not configured for container root disks. Your host may therefore return:

```text
storage: storage 'local' does not support container directories
```

Check which storage supports `rootdir`:

```bash
pvesm status
```

Look for a storage such as `local-lvm`, `local-zfs`, or another storage configured for `rootdir`.

If your host has `local-lvm`, use it for `--rootfs` while keeping the template on `local`:

```bash
pct create 200 local:vztmpl/debian-13-standard_13.6-1_amd64.tar.zst \
  --hostname jimmy-backend \
  --cores 4 \
  --memory 4096 \
  --swap 1024 \
  --rootfs local-lvm:20 \
  --net0 name=eth0,bridge=vmbr0,ip=192.168.10.190/24,gw=192.168.10.1 \
  --unprivileged 1 \
  --onboot 1 \
  --features nesting=1
```

If your root-disk storage has a different name, replace `local-lvm` with that storage.

If you are unsure which storage to use, **stop before running `pct create` and paste the output of `pvesm status`**. The storage configuration determines the correct command.

The example above uses CT ID `200`, hostname `jimmy-backend`, `4` CPU cores, `4G` RAM, `20G` root disk, bridge `vmbr0`, and example IP `192.168.10.190`.

**Change `192.168.10.190` to an unused IP on your LAN.** Also change `192.168.10.1` if that is not your router/gateway, and change `vmbr0` if your Proxmox LAN bridge has another name.

Set the container password:

```bash
pct set 200 --password
```

Start it:

```bash
pct start 200
```

Enter the new LXC:

```bash
pct enter 200
```

Inside the LXC:

```bash
apt update
apt upgrade -y
apt install -y git curl build-essential pkg-config libssl-dev ca-certificates cargo rustc clang libclang-dev
```

Then clone Jimmy:

```bash
git clone https://github.com/harry1489/Jimmy.git /opt/Jimmy
cd /opt/Jimmy
```

If the LXC is only being used as the backend, **do not install LLaVA in the LXC**. The LXC will call Ollama/LLaVA on the main Gentoo PC over the LAN.

## Build Jimmy on the Gentoo desktop

The Rust OpenCV dependency uses `opencv-binding-generator` during compilation. The generator needs a `clang` executable in addition to the OpenCV development files.

On Gentoo, make sure Clang is installed:

```bash
sudo emerge --ask llvm-core/clang
clang --version
```

Then build:

```bash
cd ~/Jimmy
cargo build --release
```

If the build previously stopped at:

```text
Can't find clang binary
```

install Clang first and rerun the build. There is no need to reinstall OpenCV if the build already detects your OpenCV headers and libraries correctly.

## Build Jimmy in the LXC

Inside the LXC:

```bash
cd /opt/Jimmy
cargo build --release
install -Dm755 target/release/jimmy_backend /usr/local/bin/jimmy_backend
```

The current crate includes the Rust/OpenCV desktop components, so the LXC build also needs its OpenCV/Clang build dependencies even though the LXC itself will not access the webcam or GPU. A future feature split can make the backend-only build smaller.

Configure the backend to point to the main Gentoo PC:

```toml
ollama_url = "http://192.168.10.181:11434"
vision_url = "http://192.168.10.181:11434"
vision_model = "llava:latest"
```

The LXC therefore acts as the backend, while the main PC owns the vision model and desktop hardware.

## Put LLaVA on the main Gentoo PC

On the **main Gentoo gaming PC**, verify Ollama:

```bash
curl http://127.0.0.1:11434/api/tags
```

Install the text and vision models there:

```bash
ollama pull llama3.2
ollama pull llava:latest
ollama list
```

The main PC should have Ollama listening on its private LAN address as well as locally. In your current layout that is intended to be:

```text
http://192.168.10.181:11434
```

Test from the LXC:

```bash
curl http://192.168.10.181:11434/api/tags
```

If that works, the LXC can use the main PC's LLaVA without having its own copy of the model.

Do not expose port `11434` to the public Internet. Allow it only from your trusted LAN/VPN and firewall it appropriately.

## Proxmox CLI management

From the Proxmox host you can manage the LXC with:

```bash
pct status 200
pct start 200
pct shutdown 200
pct stop 200
pct enter 200
pct exec 200 -- systemctl status
pct console 200
pct config 200
```

To see all containers:

```bash
pct list
```

To remove the container later, stop it first and then destroy it:

```bash
pct stop 200
pct destroy 200
```

Be careful with `pct destroy`; it deletes the container and its local data.

## Gentoo desktop services

The main Gentoo PC runs:

- Ollama + `llama3.2`
- Ollama + `llava:latest`
- `jimmy`
- Rust/OpenCV `jimmy_presence`
- `jimmy_ui`
- Hyprland
- Desktop applications

Desktop-local endpoints should remain on localhost:

```text
Jimmy       127.0.0.1:8787
Presence    127.0.0.1:8791
UI          127.0.0.1:8788
```

Remote services use their LAN addresses:

```text
Ollama/LLaVA  192.168.10.181:11434
Voice         192.168.10.182:5006
Jimmy LXC     192.168.10.190   ← example; choose your actual unused IP
```

Do not blindly replace every `127.0.0.1` in the configuration with a LAN address. The local desktop services are intentionally localhost-only.

## Voice / "Hey Jimmy"

The configured voice service is currently:

```text
Voice service: 192.168.10.182:5006
STT:           whisper.cpp tiny.en
TTS:           Piper
Voice:         en_US-lessac-medium.onnx
```

The pink UI runs locally at `http://127.0.0.1:8788` and has a wake-event endpoint for `Hey Jimmy`. The remote voice service still needs a deliberate authenticated event bridge before it can notify that local UI over the LAN.

## Security

Keep the LXC unprivileged. It does not need the gaming GPU or webcam when LLaVA and presence detection are on the main PC.

Do not expose these services directly to the public Internet:

- Ollama `:11434`
- Jimmy `:8787`
- UI `:8788`
- Presence `:8791`
- Backend administration endpoints

Use a firewall and, for remote access, a private VPN/mesh network. Keep Jimmy's HMAC authentication enabled and do not treat a spoken phrase as administrator authentication.

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
- Refuse arbitrary shell commands.
- Refuse mouse/keyboard injection until its Linux permissions are explicitly configured.

## License

MIT
