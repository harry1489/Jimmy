# Jimmy approval routing

Jimmy's approval flow is designed around one rule:

> If Jimmy needs Harry's confirmation and Harry is not physically at the desktop, Jimmy asks Jonathan to reach Harry instead of silently approving anything.

## Flow

```text
                    +------------------+
                    |  Jimmy needs     |
                    |  confirmation    |
                    +--------+---------+
                             |
                             v
                    +------------------+
                    | Local presence   |
                    | check            |
                    +----+--------+----+
                         |        |
                    present       away/unknown
                         |        |
                         v        v
                  +----------+  +----------------+
                  | Ask      |  | Delegate to    |
                  | Harry    |  | Jonathan       |
                  +----+-----+  +-------+--------+
                       |                |
                       |                v
                       |        +----------------+
                       |        | Jonathan asks  |
                       |        | Harry          |
                       |        +-------+--------+
                       |                |
                       |          approve/deny
                       |                |
                       +--------+-------+
                                |
                                v
                       +----------------+
                       | Jimmy executes |
                       | only if an     |
                       | authenticated  |
                       | approval exists|
                       +----------------+
```

## Presence is not authentication

`scripts/jimmy_presence.py` only answers whether a person appears to be in front of the camera. It does not identify the person, create a biometric profile, or grant permission. Camera frames are processed locally and are not saved by the helper.

Recommended state handling:

- `present`: ask Harry locally.
- `away`: route the pending approval to Jonathan.
- `unknown`: treat the user as unavailable and route to Jonathan.
- `camera_unavailable`: do not guess that Harry is present; use the delegated path.

A short absence timeout should be used by the eventual long-running presence service so one missed frame does not immediately switch the user to `away`.

## Delegated approval contract

When a sensitive request is pending, Jimmy should create a short-lived request containing:

- request ID
- action name
- human-readable reason
- created/expiry time
- originating source
- required permission level

Jonathan should receive only the information necessary to ask Harry. Jonathan must not be allowed to convert an ordinary natural-language instruction into an arbitrary privileged action.

When Harry answers through Jonathan, the answer should be relayed using a separate authenticated trusted-agent credential and a one-time request ID. Jimmy must verify the request is still pending and unexpired before executing it.

For destructive or administrative actions, an approval relayed by Jonathan should remain an explicit user approval; `trusted_agent_may_authorize_admin` must never mean that Jonathan can invent Harry's answer.

## Privacy defaults

- Camera processing stays on the gaming PC.
- Do not upload frames to Ollama, Home Assistant, Jonathan, or another machine for presence detection.
- Do not store camera frames.
- Do not perform face identification.
- Do not use presence as a replacement for authentication.
- Show a clear local-camera/presence status in the Jimmy UI.
