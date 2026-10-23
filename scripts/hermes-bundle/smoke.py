"""Exercise real imports and the built dashboard in an offline network namespace."""
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request

home = Path("/home/ubuntu")
hermes = str(home / ".local/bin/hermes")
for args in [[hermes, "--version"], [hermes, "tools", "list"],
             [str(home / ".opencode/bin/opencode"), "--version"],
             [str(home / ".local/lib/clawmacdo/cloudflared"), "--version"]]:
    subprocess.run(args, check=True, timeout=120)
# No provider keys or Telegram token are required to serve the local dashboard.
(home / ".hermes/config.yaml").write_text("terminal:\n  backend: local\n")
log = home / "hermes-dashboard-smoke.log"
with log.open("w") as output:
    process = subprocess.Popen([hermes, "dashboard", "--host", "127.0.0.1", "--port", "9119", "--no-open"], stdout=output, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 90
        while True:
            if process.poll() is not None:
                raise RuntimeError("Dashboard exited: " + log.read_text()[-5000:])
            try:
                with urllib.request.urlopen("http://127.0.0.1:9119/", timeout=2) as response:
                    assert b"<html" in response.read().lower(), "Missing compiled dashboard"
                with urllib.request.urlopen("http://127.0.0.1:9119/api/status", timeout=2) as response:
                    assert isinstance(json.load(response), dict)
                break
            except (urllib.error.URLError, TimeoutError):
                if time.monotonic() >= deadline:
                    raise RuntimeError("Dashboard startup timed out: " + log.read_text()[-5000:])
                time.sleep(1)
    finally:
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
print("Native bundle: CLI, tools, OpenCode, cloudflared and dashboard work offline")
