"""Exercise a relocated local bundle install: CLI, package doctor and the built dashboard."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOTS = ("hermes-agent", "tools", "installs")


def leftover_build_paths(hermes_home, manifest):
    """Text files that still name the build's location after relocation."""
    windows = sys.platform == "win32"
    needles = set()
    # The same paths the packager looks for: this build's Hermes home, and dot-folders
    # under its home. Wheels built on other CI runners name /Users/runner/work/...
    for path in (manifest["build_hermes_home"], manifest["build_home"] + os.sep + "."):
        needles.add(path)
        if windows:
            needles |= {path.replace("\\", "/"), path.replace("\\", "\\\\")}
    encoded = [needle.lower().encode() if windows else needle.encode() for needle in needles]
    found = []
    for root in ROOTS:
        for path in (hermes_home / root).rglob("*"):
            if not path.is_file() or path.is_symlink() or path.suffix == ".pyc":
                continue
            data = path.read_bytes()
            haystack = data.lower() if windows else data
            if not any(needle in haystack for needle in encoded):
                continue
            try:
                data.decode("utf-8")
            except UnicodeDecodeError:
                continue  # Native debug strings; reported when the bundle was built.
            found.append(str(path.relative_to(hermes_home)))
    return found


def check_dashboard(launcher, log):
    with log.open("w") as output:
        process = subprocess.Popen(
            [launcher, "dashboard", "--host", "127.0.0.1", "--port", "9119", "--no-open"],
            stdout=output, stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 120
            while True:
                if process.poll() is not None:
                    raise RuntimeError("Dashboard exited: " + log.read_text()[-5000:])
                try:
                    with urllib.request.urlopen("http://127.0.0.1:9119/", timeout=2) as response:
                        assert b"<html" in response.read().lower(), "Missing compiled dashboard"
                    with urllib.request.urlopen("http://127.0.0.1:9119/api/status", timeout=2) as response:
                        assert isinstance(json.load(response), dict)
                    return
                except (urllib.error.URLError, TimeoutError, ConnectionError):
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--launcher", required=True)
    parser.add_argument("--hermes-home", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text())

    leftovers = leftover_build_paths(args.hermes_home, manifest)
    if leftovers:
        raise SystemExit("Text files still name the build location:\n  " + "\n  ".join(leftovers))
    for command in ([args.launcher, "--version"], [args.launcher, "pm", "doctor"]):
        subprocess.run(command, check=True, timeout=300)
    check_dashboard(args.launcher, args.hermes_home.parent / "hermes-dashboard-smoke.log")
    print("Relocated bundle: no build paths left; CLI, package doctor and dashboard work offline")


if __name__ == "__main__":
    main()
