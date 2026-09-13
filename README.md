# ♡ Jimmy

Jimmy is a small, permissioned desktop companion for Gentoo Linux + Hyprland. The design goal is **agent control without giving an AI an unrestricted shell**.

Test target: Gentoo Linux x86_64, OpenRC, Hyprland 0.54.x, Wayland.

## What v0.1 can do

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

## Security model

Jimmy is intentionally **not** a root daemon. Keep it bound to `127.0.0.1` unless you deliberately add a private network layer.

There are three important boundaries:

1. The main AI authenticates to Jimmy using an HMAC signature.
2. Jimmy accepts named actions, not arbitrary commands.
3. Sensitive actions enter a pending-confirmation state instead of executing immediately.

Do not put a real secret in Git. Because the OpenRC service runs as your normal desktop user, `/etc/jimmy/config.toml` must be readable by that user and should otherwise be private. For example, use owner `harry` and mode `0600`:

```bash
sudo chown harry:users /etc/jimmy/config.toml
sudo chmod 600 /etc/jimmy/config.toml
```

## Gentoo dependencies

At minimum, install the runtime tools that Jimmy uses:

- `sys-apps/util-linux` for `loginctl`
- `x11-misc/xdg-utils` for `xdg-open`
- `gui-apps/grim` for screenshots
- Rust/Cargo for building Jimmy

Hyprland itself provides `hyprctl`.

Package names/use flags can vary with your Gentoo repository, so verify them with `emerge -s` before installing.

## Build

```bash
git clone https://github.com/harry1489/Jimmy.git
cd Jimmy
cargo build --release
sudo install -Dm755 target/release/jimmy /usr/local/bin/jimmy
sudo install -Dm600 config/jimmy.toml.example /etc/jimmy/config.toml
sudo sed -i "s/REPLACE_WITH_A_64_HEX_CHARACTER_SECRET/$(openssl rand -hex 32)/" /etc/jimmy/config.toml
sudo chown harry:users /etc/jimmy/config.toml
```

Edit the allowlist before starting Jimmy.

Install the OpenRC service:

```bash
sudo install -Dm755 openrc/jimmy /etc/init.d/jimmy
sudo rc-update add jimmy default
sudo rc-service jimmy start
```

Check it:

```bash
curl http://127.0.0.1:8787/health
```

## HMAC requests

The signature is the lowercase hexadecimal HMAC-SHA256 of the exact request body using `shared_secret`.

Example request body:

```json
{"action":"lock","args":{},"source":"main_ai"}
```

Python example for generating a signature:

```python
import hashlib, hmac
secret = b"YOUR_SECRET"
body = b'{"action":"lock","args":{},"source":"main_ai"}'
sig = hmac.new(secret, body, hashlib.sha256).hexdigest()
print(sig)
```

Then send `X-Jimmy-Signature: <signature>`.

The repository also includes `scripts/jimmy_request.py` for testing requests from your main agent.

## Confirmation flow

For a sensitive action, Jimmy returns a request ID instead of executing it:

```text
POST /v1/action
        ↓
permission check
        ↓
pending_confirmation
        ↓
YOU approve
        ↓
POST /v1/confirm/<request_id>
        ↓
execute
```

The confirmation endpoint itself is authenticated too.

## Main AI / remote control

For your eventual main-agent integration, do **not** expose port 8787 directly to the public Internet. Prefer a private VPN/mesh network and keep the HMAC authentication in place. A future version can add mTLS/device certificates and replay protection.

The intended architecture is:

```text
You
 │
 ▼
Main AI / planner
 │
 │ authenticated action request
 ▼
Jimmy on Gentoo desktop
 │
 ├── Hyprland
 ├── applications
 └── controlled Linux capabilities

Proxmox
 ├── PostgreSQL / memory
 └── main-agent services
```

## Voice / "Hey Jimmy"

Voice should be a separate process from the privileged action layer. The safe pipeline is:

```text
microphone
  ↓
wake-word detector ("Hey Jimmy")
  ↓
speech-to-text
  ↓
intent parser / main AI
  ↓
structured Jimmy action
  ↓
permission engine
  ↓
Jimmy
```

Jimmy itself should never treat raw speech as a shell command. The voice layer produces structured actions such as `open_app`, `open_url`, `workspace`, or `lock`.

## Future input-control module

Mouse/keyboard control is deliberately disabled in v0.1. When it is added, use a narrowly scoped helper backed by Linux `uinput`/`ydotool` permissions rather than making Jimmy root. Give input control its own permission class and require confirmation by default.

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
