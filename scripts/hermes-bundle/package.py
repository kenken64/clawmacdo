"""Package only software from a clean CI install, never a whole Hermes home."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tarfile

# Fixed Ubuntu paths preserve Hermes's launcher/venv bindings and update support.
ROOTS = (
    ".hermes/hermes-agent", ".hermes/tools", ".hermes/installs", ".hermes/skills",
    ".local/bin/hermes", ".local/bin/hermes-acp", ".local/bin/hermes-agent",
    ".local/share/uv/python", ".local/lib/clawmacdo/cloudflared", ".local/lib/clawmacdo/opencode",
)


def permitted_path(name):
    return any(name == root or name.startswith(root + "/") for root in ROOTS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--arch", choices=["amd64", "arm64"], required=True)
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Only clean GitHub Actions builds may produce a distributable bundle")
    config = json.loads(args.config.read_text())
    home = Path("/home/ubuntu")
    for root in ROOTS:
        if not (home / root).exists():
            raise SystemExit(f"Missing required software: {root}")
    if not (home / ".hermes/hermes-agent/hermes_cli/web_dist/index.html").is_file():
        raise SystemExit("Hermes dashboard was not compiled")
    filename = f"hermes-ubuntu-24.04-{args.arch}.tar.gz"
    archive = args.output / filename

    def include(member):
        # Download caches and transient locks are not runtime dependencies.
        if any(part.startswith((".staging-", "fetch-")) for part in Path(member.name).parts):
            return None
        if not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
            raise ValueError(f"Unexpected special file: {member.name}")
        if not permitted_path(member.name):
            raise ValueError(f"Not a software path: {member.name}")
        member.uid = member.gid = 0
        member.uname = member.gname = ""
        member.mode &= 0o777  # Never ship setuid/setgid bits.
        return member

    with tarfile.open(archive, "w:gz", compresslevel=6, format=tarfile.PAX_FORMAT) as tar:
        for root in ROOTS:
            tar.add(home / root, arcname=root, filter=include)
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    manifest = {
        "schema": 1, "release": config["release"], "os": "ubuntu", "os_version": "24.04",
        "arch": args.arch, "filename": filename, "sha256": digest,
        "size": archive.stat().st_size, "hermes_commit": config["hermes_commit"],
        "hermes_version": config["hermes_version"], "opencode_version": config["opencode_version"],
        "cloudflared_version": config["cloudflared_version"],
    }
    (args.output / f"{filename}.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (args.output / f"{filename}.sha256").write_text(f"{digest}  {filename}\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
