"""Live local account check. Credentials are read locally and never printed."""
import argparse
import json
import os
import sqlite3
import urllib.error
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument("--generate", action="store_true")
parser.add_argument("--enable", action="store_true")
args = parser.parse_args()
base = "http://localhost:8000"

def request(path, body, bearer=None, timeout=90):
    headers = {"Content-Type": "application/json"}
    if bearer:
        headers["Authorization"] = "Bearer " + bearer
    req = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"HTTP {error.code}: {error.read().decode()[:600]}") from None

c = sqlite3.connect("data/flow.db")
c.row_factory = sqlite3.Row
creds = dict(c.execute("select username,password from admin_config limit 1").fetchone())
session = request("/api/login", creds)["token"]
email = os.environ.get("FLOW2API_VERIFY_EMAIL", "").strip()
if not email:
    raise SystemExit("Set FLOW2API_VERIFY_EMAIL to the account to check (kept local, not in git).")
row = c.execute("select id from tokens where email=?", (email,)).fetchone()
assert row, f"Account missing: {email}"
token_id = row["id"]
result = request(f"/api/tokens/{token_id}/refresh-credits", {}, session)
print("Balance:", result)
assert result.get("success") and result.get("credits", 0) > 0, "Expected positive Flow balance"
if args.enable:
    print("Enable:", request(f"/api/tokens/{token_id}/enable", {}, session))
if args.generate:
    result = request("/v1/chat/completions", {"model":"gemini-3.1-flash-image-square", "messages":[{"role":"user","content":"A simple red apple centered on a plain white background, studio photograph."}], "stream":False}, c.execute("select api_key from admin_config limit 1").fetchone()[0], timeout=600)
    text = json.dumps(result, ensure_ascii=False)
    print(text[:2200])
    assert "error" not in result and ("http" in text or "data:image" in text), "No generated image returned"
