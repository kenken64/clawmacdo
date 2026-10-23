"""Exercise the generated cloud-init Python against temporary files and HTTP servers.

Run with: python3 tests/unit/test_hermes_cloudflare.py
No AWS resources, system services, or Cloudflare connections are created.
"""
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


COMMANDS = Path(__file__).resolve().parents[2] / "crates/clawmacdo-cli/src/commands"
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("hermes_cloudflare_ready", COMMANDS / "hermes_cloudflare_ready.py")
ready = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ready)


def python_block(filename):
    return (COMMANDS / filename).read_text().split("python3 - <<'PY'\n", 1)[1].split("\nPY", 1)[0]


class HermesCloudflareTests(unittest.TestCase):
    def test_configuration_hashes_password_before_tunnel_starts(self):
        settings = {
            "password": "long ' quoted $(password) with unicode 🔑",
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "config.yaml"
            config.write_text("terminal:\n  backend: local\n")
            config.chmod(0o600)
            script = python_block("hermes_cloudflare.sh")
            script = script.replace("__TUNNEL_CONFIG_B64__", base64.b64encode(json.dumps(settings).encode()).decode())
            script = script.replace("/etc/hermes-cloudflared", directory).replace("/home/ubuntu/.hermes", directory)
            result = subprocess.run([sys.executable, "-c", script], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "")
            self.assertFalse((root / "token").exists())
            self.assertEqual(stat.S_IMODE((root / "dashboard.env").stat().st_mode), 0o600)
            self.assertIn('HERMES_DASHBOARD_PUBLIC_URL=https://pending.invalid', (root / "dashboard.env").read_text())
            self.assertEqual(stat.S_IMODE(config.stat().st_mode), 0o600)
            content = config.read_text()
            self.assertNotIn(settings["password"], content)
            self.assertTrue(content.startswith("terminal:\n  backend: local\n"))
            dashboard = json.loads(content.split("\ndashboard: ", 1)[1])
            self.assertEqual(dashboard["public_url"], 'https://pending.invalid')
            auth = dashboard["basic_auth"]
            self.assertEqual(auth["username"], "admin")
            self.assertGreaterEqual(len(bytes.fromhex(auth["secret"])), 32)
            scheme, n, r, p, salt, digest = auth["password_hash"].split("$")
            self.assertEqual(scheme, "scrypt")
            actual = hashlib.scrypt(settings["password"].encode(), salt=base64.b64decode(salt), n=int(n), r=int(r), p=int(p), dklen=32)
            self.assertEqual(actual, base64.b64decode(digest))

    def check_readiness(self, status, auth_status, edge_status=200):
        seen_hosts = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                seen_hosts.append(self.headers.get("Host"))
                if self.path == "/api/status":
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(json.dumps(status).encode())
                elif self.path == "/api/auth/me":
                    self.send_response(auth_status)
                    self.end_headers()
                else:
                    self.send_response(edge_status)
                    self.end_headers()

            def log_message(self, *_):
                pass

        with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                origin = f"http://127.0.0.1:{server.server_port}"
                with patch.object(ready, "ORIGIN", origin), patch.object(ready, "READY_URL", origin + '/ready'), patch.object(ready.time, "sleep"), patch.object(ready, "range", lambda _: range(2), create=True):
                    ready.check_dashboard('https://test.trycloudflare.com')
            finally:
                server.shutdown()
                thread.join()
        self.assertTrue(seen_hosts)
        self.assertEqual(seen_hosts[:2], ["test.trycloudflare.com"] * 2)

    def test_authenticated_dashboard_is_ready(self):
        self.check_readiness({"auth_required": True, "auth_providers": ["basic"]}, 401)

    def test_dashboard_without_auth_fails_closed(self):
        with self.assertRaisesRegex(SystemExit, "refusing to keep the tunnel running"):
            self.check_readiness({"auth_required": False, "auth_providers": []}, 401)

    def test_unprotected_api_fails_closed(self):
        with self.assertRaisesRegex(SystemExit, "unauthenticated request"):
            self.check_readiness({"auth_required": True, "auth_providers": ["basic"]}, 200)

    def test_wrong_auth_provider_fails_closed(self):
        with self.assertRaisesRegex(SystemExit, "refusing to keep the tunnel running"):
            self.check_readiness({"auth_required": True, "auth_providers": ["nous"]}, 401)

    def test_disconnected_tunnel_does_not_pass_readiness(self):
        with self.assertRaisesRegex(SystemExit, "did not become ready"):
            self.check_readiness({"auth_required": True, "auth_providers": ["basic"]}, 401, edge_status=503)

    def test_generated_url_is_updated_on_each_restart(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(ready, "LOG_PATH", root / 'cloudflared.log'), patch.object(ready, "ENV_PATH", root / 'dashboard.env'), patch.object(ready, "URL_PATH", root / 'dashboard-url'), patch.object(ready.subprocess, "run") as restart, patch.object(ready, "check_dashboard") as check, patch('builtins.print'):
                for hostname in ['first.trycloudflare.com', 'second.trycloudflare.com']:
                    url = 'https://' + hostname
                    ready.LOG_PATH.write_text('INF | ' + url + ' |\n')
                    ready.main()
                    self.assertEqual(ready.URL_PATH.read_text().strip(), url)
                    self.assertEqual(ready.ENV_PATH.read_text(), 'HERMES_DASHBOARD_PUBLIC_URL=' + url + '\n')
                    self.assertEqual(stat.S_IMODE(ready.ENV_PATH.stat().st_mode), 0o600)
                    self.assertEqual(stat.S_IMODE(ready.URL_PATH.stat().st_mode), 0o644)
                    check.assert_called_with(url)
                self.assertEqual(restart.call_count, 2)
                restart.assert_called_with(['systemctl', 'restart', 'hermes-dashboard.service'], check=True, timeout=120)
                check.side_effect = SystemExit('auth failed')
                with self.assertRaises(SystemExit):
                    ready.main()
                self.assertFalse(ready.URL_PATH.exists())

    def test_discovery_rejects_urls_outside_quick_tunnel_domain(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory, 'log')
            with patch.object(ready, "LOG_PATH", log), patch.object(ready.time, "sleep"):
                for url in ['https://evil.example.com', 'http://test.trycloudflare.com', 'https://test.trycloudflare.com.evil.com', 'https://-test.trycloudflare.com', 'https://test.trycloudflare.com/path', 'https://test.trycloudflare.com:443']:
                    log.write_text('INF | ' + url + ' |\n')
                    with self.assertRaisesRegex(SystemExit, 'did not report'):
                        ready.discover_url()


if __name__ == "__main__":
    unittest.main()
