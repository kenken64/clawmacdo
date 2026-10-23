"""Download and install the pinned native bundle as ubuntu; no apt operations."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import posixpath
import re
import shutil
import tarfile
import tempfile
import time
import urllib.error
import urllib.request

SOFTWARE_ROOTS = (
    ".hermes/hermes-agent", ".hermes/tools", ".hermes/installs", ".hermes/skills",
    ".local/bin/hermes", ".local/bin/hermes-acp", ".local/bin/hermes-agent",
    ".local/share/uv/python", ".local/lib/clawmacdo/cloudflared", ".opencode/bin/opencode",
)
PROGRESS = Path("/var/lib/clawmacdo/hermes-progress/bundle.status")


class BundleUnavailable(Exception):
    """No bundle for this OS/architecture/release; source fallback is allowed."""


def report(message):
    line = f"Bundle: {message}"
    print(f"[clawmacdo] {line}", flush=True)
    if PROGRESS.parent.is_dir():
        temporary = PROGRESS.with_suffix(".tmp")
        temporary.write_text(line + "\n")
        temporary.replace(PROGRESS)


def permitted_path(name):
    return any(name == root or name.startswith(root + "/") for root in SOFTWARE_ROOTS)


def validate_manifest(manifest, config, arch):
    expected = {
        "schema": 1, "release": config["release"], "os": "ubuntu", "os_version": "24.04",
        "arch": arch, "filename": f"hermes-ubuntu-24.04-{arch}.tar.gz",
        "hermes_commit": config["hermes_commit"], "hermes_version": config["hermes_version"],
        "opencode_version": config["opencode_version"], "cloudflared_version": config["cloudflared_version"],
    }
    if not isinstance(manifest, dict) or any(manifest.get(k) != v for k, v in expected.items()):
        raise ValueError("Bundle identity does not match the pinned release and platform")
    if not re.fullmatch(r"[a-f0-9]{64}", str(manifest.get("sha256", ""))):
        raise ValueError("Invalid bundle checksum")
    size = manifest.get("size")
    if type(size) is not int or not 0 < size <= 2 * 1024**3:
        raise ValueError("Invalid bundle size")


def download(url, destination, maximum):
    for attempt in range(3):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "clawmacdo-hermes-bundle"})
            with urllib.request.urlopen(request, timeout=45) as response, destination.open("wb") as output:
                if not response.url.startswith("https://"):
                    raise ValueError("Bundle download redirected away from HTTPS")
                size = 0
                while chunk := response.read(1024 * 1024):
                    size += len(chunk)
                    if size > maximum:
                        raise ValueError("Bundle download exceeded its declared size")
                    output.write(chunk)
            return
        except urllib.error.HTTPError as error:
            if error.code == 404:
                raise BundleUnavailable("release asset is not published") from error
            if attempt == 2:
                raise
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            if attempt == 2:
                raise
        time.sleep(attempt + 1)


def extract_bundle(archive, manifest, home):
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    if archive.stat().st_size != manifest["size"] or digest != manifest["sha256"]:
        raise ValueError("Bundle checksum or size verification failed")
    for root in SOFTWARE_ROOTS:
        destination = home / root
        if destination.exists() or destination.is_symlink():
            raise ValueError(f"Refusing to overwrite an existing install: {root}")

    with tempfile.TemporaryDirectory(prefix=".clawmacdo-bundle-", dir=home) as directory:
        stage = Path(directory)

        def checked(member, destination):
            name = member.name
            if name.startswith("/") or ".." in PurePosixPath(name).parts or not permitted_path(name):
                raise ValueError(f"Unexpected bundle path: {name}")
            if not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
                raise ValueError(f"Special file in bundle: {name}")
            if member.issym() or member.islnk():
                link = member.linkname
                if link.startswith("/home/ubuntu/"):
                    link = link.removeprefix("/home/ubuntu/")
                    if member.issym():
                        link = posixpath.relpath(link, posixpath.dirname(name))
                target = posixpath.normpath(posixpath.join(posixpath.dirname(name), link) if member.issym() else link)
                if target.startswith("/") or not permitted_path(target):
                    raise ValueError(f"Bundle link escapes software directories: {name}")
                member = member.replace(linkname=link, deep=False)
            # Enforces link-chain containment and strips owners/privileged permission bits.
            return tarfile.data_filter(member, destination)

        with tarfile.open(archive) as source:
            members = source.getmembers()
            if len(members) > 250_000 or sum(m.size for m in members) > 10 * 1024**3:
                raise ValueError("Bundle is too large when extracted")
            source.extractall(stage, members=members, filter=checked)
        for root in SOFTWARE_ROOTS:
            if not (stage / root).exists():
                raise ValueError(f"Bundle is missing required software: {root}")
        dashboard = stage / ".hermes/hermes-agent/hermes_cli/web_dist/index.html"
        if not dashboard.is_file():
            raise ValueError("Bundle is missing the compiled dashboard")
        moved = []
        try:
            for root in SOFTWARE_ROOTS:
                destination = home / root
                destination.parent.mkdir(parents=True, exist_ok=True)
                (stage / root).rename(destination)
                moved.append(destination)
            (home / ".hermes").chmod(0o700)
            (home / ".clawmacdo-hermes-bundle.json").write_text(json.dumps(manifest) + "\n")
        except Exception:
            for destination in reversed(moved):
                if destination.is_dir() and not destination.is_symlink():
                    shutil.rmtree(destination)
                else:
                    destination.unlink()
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--archive", type=Path, help="Local archive for clean-runner verification")
    args = parser.parse_args()
    start = time.monotonic()
    config = json.loads(args.config.read_text())
    os_info = dict(line.strip().split("=", 1) for line in Path("/etc/os-release").read_text().splitlines() if "=" in line)
    arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(platform.machine())
    if os_info.get("ID", "").strip('"') != "ubuntu" or os_info.get("VERSION_ID", "").strip('"') != "24.04" or not arch:
        raise BundleUnavailable("platform is not Ubuntu 24.04 amd64/arm64")
    with tempfile.TemporaryDirectory(prefix="hermes-download-") as directory:
        filename = f"hermes-ubuntu-24.04-{arch}.tar.gz"
        if args.archive:
            archive = args.archive
            manifest = json.loads(archive.with_name(archive.name + ".json").read_text())
        else:
            base = f"https://github.com/{config['repository']}/releases/download/{config['release']}"
            report(f"checking {config['release']} ({arch})")
            metadata = Path(directory) / "manifest.json"
            download(f"{base}/{filename}.json", metadata, 64 * 1024)
            manifest = json.loads(metadata.read_text())
            validate_manifest(manifest, config, arch)
            report(f"downloading {manifest['size'] // 1024**2} MiB")
            archive = Path(directory) / filename
            download(f"{base}/{filename}", archive, manifest["size"])
        validate_manifest(manifest, config, arch)
        report("verifying checksum and extracting software")
        extract_bundle(archive, manifest, Path("/home/ubuntu"))
    report(f"installed in {time.monotonic() - start:.0f}s")


if __name__ == "__main__":
    try:
        main()
    except BundleUnavailable as error:
        report(f"{error}; source installation will follow OS setup")
        raise SystemExit(42)
