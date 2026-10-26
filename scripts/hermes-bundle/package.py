"""Package only software from a clean CI install, never a whole Hermes home."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import posixpath
import sys
import tarfile

# Fixed Ubuntu paths preserve Hermes's launcher/venv bindings and update support.
ROOTS = (
    ".hermes/hermes-agent", ".hermes/tools", ".hermes/installs", ".hermes/skills",
    ".local/bin/hermes", ".local/bin/hermes-acp", ".local/bin/hermes-agent",
    ".local/share/uv/python", ".local/lib/clawmacdo/cloudflared", ".local/lib/clawmacdo/opencode",
)

# Local bundles hold the software under the Hermes home. clawmacdo moves them into each
# user's own Hermes home, so launchers (bound to absolute paths) are left out: upstream's
# completion step republishes them, and syncs bundled skills, after relocation.
LOCAL_ROOTS = ("hermes-agent", "tools", "installs")
LOCAL_EXCLUDED = ("hermes-agent/.hermes/bin",)
LOCAL_PLATFORMS = {
    "macos-arm64": {"os": "macos", "store_python": ("bin", "python3")},
    "windows-x64": {"os": "windows", "store_python": ("python.exe",)},
}


def permitted_path(name, roots=ROOTS):
    return any(name == root or name.startswith(root + "/") for root in roots)


def include_software(member):
    # Only direct tool-store children are download caches. npm's runtime includes
    # minipass-fetch/lib/fetch-error.js, which must not be filtered by its basename.
    path = Path(member.name)
    if path.parent == Path(".hermes/tools") and path.name.startswith((".staging-", "fetch-")):
        return None
    if not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
        raise ValueError(f"Unexpected special file: {member.name}")
    if not permitted_path(member.name):
        raise ValueError(f"Not a software path: {member.name}")
    member.uid = member.gid = 0
    member.uname = member.gname = ""
    member.mode &= 0o777  # Never ship setuid/setgid bits.
    return member


def local_software_filter(hermes_home, excluded=LOCAL_EXCLUDED):
    """Tar filter for local bundles built in `hermes_home`.

    uv points each venv's `bin/python` at the tool store with an absolute symlink; links
    into the Hermes home become relative so they survive the move to another home.
    """
    home = Path(hermes_home).as_posix().rstrip("/") + "/"

    def include(member):
        path = Path(member.name)
        if path.parent == Path("tools") and path.name.startswith((".staging-", "fetch-")):
            return None
        if permitted_path(member.name, excluded):
            return None
        if not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
            raise ValueError(f"Unexpected special file: {member.name}")
        if member.issym() and (member.linkname.startswith("/") or ":" in member.linkname):
            if not member.linkname.startswith(home):
                raise ValueError(f"Absolute symlink cannot be relocated: {member.name} -> {member.linkname}")
            target = member.linkname[len(home):]
            if not permitted_path(target, LOCAL_ROOTS):
                raise ValueError(f"Symlink leaves the packaged software: {member.name} -> {member.linkname}")
            member.linkname = posixpath.relpath(target, posixpath.dirname(member.name))
        if not permitted_path(member.name, LOCAL_ROOTS):
            raise ValueError(f"Not a software path: {member.name}")
        member.uid = member.gid = 0
        member.uname = member.gname = ""
        member.mode &= 0o777
        return member

    return include


def install_key(project_root):
    """Upstream's per-install state folder name (pm/environments.py: install_key)."""
    return hashlib.sha256(str(Path(project_root).resolve()).encode("utf-8")).hexdigest()[:16]


def path_needles(path_text, windows):
    """Spellings of a build path that files may contain."""
    needles = {path_text}
    if windows:
        needles |= {path_text.replace("\\", "/"), path_text.replace("\\", "\\\\")}
    return sorted(needles)


def find_path_references(hermes_home, roots, needles, windows, excluded=LOCAL_EXCLUDED):
    """Files under the packaged roots that mention a build path.

    Text files are listed for clawmacdo to rewrite on install. Binary files are only
    reported: bytecode and native debug strings name build paths without using them,
    and the clean-runner verification proves a relocated install still works.
    """
    encoded = [needle.lower().encode("utf-8") if windows else needle.encode("utf-8") for needle in needles]
    text, binary = [], []
    for root in roots:
        for directory, dirnames, filenames in os.walk(hermes_home / root):
            dirnames.sort()
            for filename in sorted(filenames):
                path = Path(directory) / filename
                relative = path.relative_to(hermes_home).as_posix()
                if path.is_symlink() or permitted_path(relative, excluded) or path.suffix == ".pyc":
                    continue
                data = path.read_bytes()
                haystack = data.lower() if windows else data
                if not any(needle in haystack for needle in encoded):
                    continue
                try:
                    data.decode("utf-8")
                except UnicodeDecodeError:
                    binary.append(relative)
                else:
                    text.append(relative)
    return text, binary


def pyvenv_home(cfg):
    for line in cfg.read_text().splitlines():
        key, _, value = line.partition("=")
        if key.strip() == "home":
            return value.strip()
    return ""


def stale_pm_runtimes(hermes_home, key):
    """PM runtime generations other than the selected one, which are left out.

    A fresh install first builds PM's runtime on the bootstrap Python uv downloads to
    ~/.local/share/uv/python; the selected generation must use the tool store's Python.
    """
    runtime = hermes_home / "installs" / key / "pm-runtime"
    try:
        selected = json.loads((runtime / "selected.json").read_text())["generation"]
    except FileNotFoundError:
        return ()
    home = pyvenv_home(runtime / selected / "pyvenv.cfg")
    if not Path(home).resolve().is_relative_to(hermes_home / "tools"):
        raise SystemExit(f"PM's selected runtime {selected} uses a Python outside the tool store: {home}")
    return tuple(
        f"installs/{key}/pm-runtime/generations/{path.name}"
        for path in sorted((runtime / "generations").iterdir())
        if path.is_dir() and f"generations/{path.name}" != selected
    )


def split_tool_store(names):
    """PM verifies the tool store against recorded digests, so its files are never
    rewritten. uv records the install path in Python's build-time sysconfig, which only
    matters when compiling extensions against it."""
    store = [name for name in names if permitted_path(name, ("tools",))]
    return [name for name in names if name not in store], store


def find_store_python(hermes_home, windows):
    pattern = LOCAL_PLATFORMS["windows-x64" if windows else "macos-arm64"]["store_python"]
    matches = sorted(
        path for path in (hermes_home / "tools").glob("python-*")
        if path.is_dir() and path.joinpath(*pattern).is_file()
    )
    if len(matches) != 1:
        raise SystemExit(f"Expected one store Python in {hermes_home / 'tools'}, found {matches}")
    return matches[0].joinpath(*pattern).relative_to(hermes_home).as_posix()


def write_outputs(archive, manifest, output):
    with archive.open("rb") as source:
        manifest["sha256"] = hashlib.file_digest(source, "sha256").hexdigest()
    manifest["size"] = archive.stat().st_size
    (output / f"{archive.name}.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (output / f"{archive.name}.sha256").write_text(f"{manifest['sha256']}  {archive.name}\n")
    print(json.dumps({k: v for k, v in manifest.items() if k != "relocate"}, indent=2))


def package_ubuntu(config, output, arch):
    home = Path("/home/ubuntu")
    for root in ROOTS:
        if not (home / root).exists():
            raise SystemExit(f"Missing required software: {root}")
    if not (home / ".hermes/hermes-agent/hermes_cli/web_dist/index.html").is_file():
        raise SystemExit("Hermes dashboard was not compiled")
    filename = f"hermes-ubuntu-24.04-{arch}.tar.gz"
    archive = output / filename

    with tarfile.open(archive, "w:gz", compresslevel=6, format=tarfile.PAX_FORMAT) as tar:
        for root in ROOTS:
            tar.add(home / root, arcname=root, filter=include_software)
    manifest = {
        "schema": 1, "release": config["release"], "os": "ubuntu", "os_version": "24.04",
        "arch": arch, "filename": filename, "hermes_commit": config["hermes_commit"],
        "hermes_version": config["hermes_version"], "opencode_version": config["opencode_version"],
        "cloudflared_version": config["cloudflared_version"],
    }
    write_outputs(archive, manifest, output)


def package_local(config, output, platform, report):
    windows = LOCAL_PLATFORMS[platform]["os"] == "windows"
    home = Path.home().resolve()
    if windows:
        hermes_home = (Path(os.environ["LOCALAPPDATA"]) / "hermes").resolve()
    else:
        hermes_home = (home / ".hermes").resolve()
    for root in LOCAL_ROOTS:
        if not (hermes_home / root).is_dir():
            raise SystemExit(f"Missing required software: {hermes_home / root}")
    if not (hermes_home / "hermes-agent/hermes_cli/web_dist/index.html").is_file():
        raise SystemExit("Hermes dashboard was not compiled")
    key = install_key(hermes_home / "hermes-agent")
    states = sorted(path.name for path in (hermes_home / "installs").iterdir() if path.is_dir())
    if states != [key]:
        raise SystemExit(f"Expected only installs/{key}, found {states}")

    # Only paths of this build: wheels and tools built on other GitHub runners also name
    # /Users/runner/work/..., which nothing here reads. Dot-folders under the home
    # (.cache, .local, ...) are where an install could still point outside its home.
    needles = path_needles(str(hermes_home), windows) + path_needles(str(home) + os.sep + ".", windows)
    excluded = LOCAL_EXCLUDED + stale_pm_runtimes(hermes_home, key)
    relocate, binary = find_path_references(hermes_home, LOCAL_ROOTS, needles, windows, excluded)
    relocate, store = split_tool_store(relocate)
    report.write_text(
        "Text files rewritten on install:\n" + "".join(f"  {name}\n" for name in relocate)
        + "Tool store files naming a build path (digest-verified, not rewritten):\n"
        + "".join(f"  {name}\n" for name in store)
        + "Binary files naming a build path (not rewritten):\n" + "".join(f"  {name}\n" for name in binary)
    )
    print(f"{len(relocate)} text, {len(store)} tool store and {len(binary)} binary files name a build path; see {report}")

    filename = f"hermes-{platform}.tar.gz"
    archive = output / filename
    with tarfile.open(archive, "w:gz", compresslevel=6, format=tarfile.PAX_FORMAT) as tar:
        for root in LOCAL_ROOTS:
            tar.add(hermes_home / root, arcname=root, filter=local_software_filter(hermes_home, excluded))
    manifest = {
        "schema": 1, "release": config["release"], "platform": platform, "filename": filename,
        "hermes_commit": config["hermes_commit"], "hermes_version": config["hermes_version"],
        "build_home": str(home), "build_hermes_home": str(hermes_home), "install_key": key,
        "store_python": find_store_python(hermes_home, windows), "relocate": relocate,
    }
    write_outputs(archive, manifest, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--arch", choices=["amd64", "arm64"], help="Ubuntu 24.04 bundle")
    target.add_argument("--platform", choices=sorted(LOCAL_PLATFORMS), help="Local macOS/Windows bundle")
    parser.add_argument("--report", type=Path, help="Where local bundles list files naming build paths")
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise SystemExit("Only clean GitHub Actions builds may produce a distributable bundle")
    config = json.loads(args.config.read_text())
    args.output.mkdir(parents=True, exist_ok=True)
    if args.arch:
        package_ubuntu(config, args.output, args.arch)
    else:
        if not args.report:
            parser.error("--report is required for local bundles")
        args.report.parent.mkdir(parents=True, exist_ok=True)
        package_local(config, args.output, args.platform, args.report)


if __name__ == "__main__":
    sys.exit(main())
