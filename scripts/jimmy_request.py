#!/usr/bin/env python3
"""Tiny dependency-free client for Jimmy's localhost API."""
import argparse, hashlib, hmac, json, urllib.request

p = argparse.ArgumentParser()
p.add_argument("action")
p.add_argument("--args", default="{}", help="JSON object")
p.add_argument("--reason", default=None)
p.add_argument("--source", default="main_ai")
p.add_argument("--secret", required=True)
p.add_argument("--confirm", default=None, help="confirmation request id")
p.add_argument("--url", default="http://127.0.0.1:8787")
a = p.parse_args()

if a.confirm:
    body = a.confirm.encode()
    endpoint = f"{a.url}/v1/confirm/{a.confirm}"
else:
    payload = {"action": a.action, "args": json.loads(a.args), "source": a.source}
    body = json.dumps(payload, separators=(",", ":")).encode()
    endpoint = f"{a.url}/v1/action"

sig = hmac.new(a.secret.encode(), body, hashlib.sha256).hexdigest()
req = urllib.request.Request(endpoint, data=None if a.confirm else body, method="POST")
req.add_header("X-Jimmy-Signature", sig)
if not a.confirm:
    req.add_header("Content-Type", "application/json")

with urllib.request.urlopen(req, timeout=10) as r:
    print(r.read().decode())
