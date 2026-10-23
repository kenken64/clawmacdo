"""Systemd startup hook: apply each Quick Tunnel URL and verify Hermes auth."""
import json
from pathlib import Path
import re
import subprocess
import time
import urllib.error
import urllib.request


LOG_PATH = Path('/run/hermes-cloudflared/cloudflared.log')
ENV_PATH = Path('/etc/hermes-cloudflared/dashboard.env')
URL_PATH = Path('/var/lib/hermes-cloudflared/dashboard-url')
ORIGIN = 'http://127.0.0.1:9119'
READY_URL = 'http://127.0.0.1:20242/ready'
QUICK_URL = re.compile(r'https://[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.trycloudflare\.com(?=[\s"|]|$)')


def discover_url():
    for _ in range(60):
        try:
            matches = QUICK_URL.findall(LOG_PATH.read_text())
            if matches:
                return matches[-1]
        except OSError:
            pass
        time.sleep(1)
    raise SystemExit('Cloudflare did not report a Quick Tunnel URL. Check journalctl -u hermes-cloudflared.')


def check_dashboard(url):
    # First launch can build the frontend. Never publish a link before auth works.
    hostname = url.removeprefix('https://')
    for _ in range(90):
        try:
            req = urllib.request.Request(ORIGIN + '/api/status', headers={'Host': hostname})
            with urllib.request.urlopen(req, timeout=2) as response:
                status = json.load(response)
            if status.get('auth_required') is not True or 'basic' not in status.get('auth_providers', []):
                raise SystemExit('Hermes dashboard authentication is not enabled; refusing to keep the tunnel running.')
            req = urllib.request.Request(ORIGIN + '/api/auth/me', headers={'Host': hostname})
            try:
                urllib.request.urlopen(req, timeout=2).close()
            except urllib.error.HTTPError as error:
                error.close()
                if error.code != 401:
                    raise
            else:
                raise SystemExit('Hermes accepted an unauthenticated request; refusing to keep the tunnel running.')
            with urllib.request.urlopen(READY_URL, timeout=2):
                pass
            return
        except (OSError, ValueError) as error:
            if isinstance(error, urllib.error.HTTPError):
                error.close()
            time.sleep(2)
    raise SystemExit('Hermes dashboard or Cloudflare connection did not become ready. Check their service logs.')


def main():
    URL_PATH.unlink(missing_ok=True)
    url = discover_url()
    # Only the validated hostname goes into the environment file. This preserves
    # the password hash and AI configuration across connector restarts.
    temp = ENV_PATH.with_suffix('.tmp')
    temp.write_text('HERMES_DASHBOARD_PUBLIC_URL=' + url + '\n')
    temp.chmod(0o600)
    temp.replace(ENV_PATH)
    subprocess.run(['systemctl', 'restart', 'hermes-dashboard.service'], check=True, timeout=120)
    check_dashboard(url)
    temp = URL_PATH.with_suffix('.tmp')
    temp.write_text(url + '\n')
    temp.chmod(0o644)
    temp.replace(URL_PATH)
    print('Hermes dashboard Quick Tunnel: ' + url, flush=True)


if __name__ == '__main__':
    main()
