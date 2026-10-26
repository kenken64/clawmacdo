"""Native bundle integrity, profile isolation and extraction regression tests."""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("hermes_bundle", ROOT / "crates/clawmacdo-cli/src/commands/hermes_bundle.py")
bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundle)
spec = importlib.util.spec_from_file_location("hermes_package", ROOT / "scripts/hermes-bundle/package.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
config = json.loads((ROOT / "scripts/hermes-bundle/bundle.json").read_text())


class BundleTests(unittest.TestCase):
    def test_package_keeps_npm_fetch_runtime_but_omits_tool_download_caches(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            runtime = home / ".hermes/tools/node/lib/node_modules/npm/node_modules/minipass-fetch/lib/fetch-error.js"
            runtime.parent.mkdir(parents=True)
            runtime.write_text("module.exports = FetchError")
            cache = home / ".hermes/tools/fetch-abc/download.tar.gz"
            cache.parent.mkdir()
            cache.write_text("download cache")
            output = home / "test.tar.gz"
            with tarfile.open(output, "w:gz") as archive:
                archive.add(home / ".hermes/tools", arcname=".hermes/tools", filter=package.include_software)
            with tarfile.open(output) as archive:
                self.assertEqual(archive.extractfile(runtime.relative_to(home).as_posix()).read(), b"module.exports = FetchError")
                self.assertNotIn(cache.relative_to(home).as_posix(), archive.getnames())

    def archive(self, directory, extra=None):
        path = Path(directory) / "bundle.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            for name in bundle.SOFTWARE_ROOTS:
                member = tarfile.TarInfo(name)
                member.type = tarfile.DIRTYPE if name in bundle.SOFTWARE_ROOTS[:4] or name.endswith("uv/python") else tarfile.REGTYPE
                member.mode = 0o755
                archive.addfile(member)
            member = tarfile.TarInfo(".hermes/hermes-agent/hermes_cli/web_dist/index.html")
            member.size = len(b"<html>dashboard</html>")
            archive.addfile(member, io.BytesIO(b"<html>dashboard</html>"))
            if extra:
                archive.addfile(extra)
        data = path.read_bytes()
        return path, {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def test_installs_software_without_touching_profile_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory) / "home"
            (home / ".hermes").mkdir(parents=True)
            secret = home / ".hermes/.env"
            secret.write_text("my existing key")
            archive, metadata = self.archive(directory)
            bundle.extract_bundle(archive, metadata, home)
            self.assertEqual(secret.read_text(), "my existing key")
            self.assertTrue((home / ".hermes/hermes-agent/hermes_cli/web_dist/index.html").exists())
            self.assertEqual((home / ".hermes").stat().st_mode & 0o777, 0o700)

    def test_tampered_archive_is_not_installed(self):
        with tempfile.TemporaryDirectory() as directory:
            archive, metadata = self.archive(directory)
            metadata["sha256"] = "0" * 64
            with self.assertRaisesRegex(ValueError, "verification failed"):
                bundle.extract_bundle(archive, metadata, Path(directory))
            self.assertFalse((Path(directory) / ".hermes").exists())

    def test_rejects_paths_credentials_and_links_outside_software(self):
        cases = []
        for name in ["../outside", "/tmp/outside", ".hermes/.env", ".ssh/authorized_keys", ".hermes/hermes-agent/../../.ssh/key"]:
            cases.append(tarfile.TarInfo(name))
        for target in ["/etc/passwd", "../../.env", "/home/ubuntu/.ssh/key"]:
            member = tarfile.TarInfo(".hermes/hermes-agent/link")
            member.type, member.linkname = tarfile.SYMTYPE, target
            cases.append(member)
        for member in cases:
            with self.subTest(name=member.name, link=member.linkname), tempfile.TemporaryDirectory() as directory:
                archive, metadata = self.archive(directory, member)
                with self.assertRaises((ValueError, tarfile.FilterError)):
                    bundle.extract_bundle(archive, metadata, Path(directory))
                self.assertFalse((Path(directory) / ".hermes").exists())

    def test_preserves_internal_absolute_symlinks(self):
        member = tarfile.TarInfo(".hermes/hermes-agent/python")
        member.type = tarfile.SYMTYPE
        member.linkname = "/home/ubuntu/.local/bin/hermes"
        with tempfile.TemporaryDirectory() as directory:
            archive, metadata = self.archive(directory, member)
            home = Path(directory)
            bundle.extract_bundle(archive, metadata, home)
            self.assertEqual((home / member.name).resolve(), (home / ".local/bin/hermes").resolve())

    def test_existing_software_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home / ".hermes/hermes-agent").mkdir(parents=True)
            archive, metadata = self.archive(directory)
            with self.assertRaisesRegex(ValueError, "overwrite"):
                bundle.extract_bundle(archive, metadata, home)

    def test_manifest_pins_version_architecture_and_filename(self):
        manifest = {k: config[k] for k in ["schema", "release", "os", "os_version", "hermes_commit", "hermes_version", "opencode_version", "cloudflared_version"]}
        manifest.update(arch="amd64", filename="hermes-ubuntu-24.04-amd64.tar.gz", size=123, sha256="1" * 64)
        bundle.validate_manifest(manifest, config, "amd64")
        for key, value in [("arch", "arm64"), ("hermes_commit", "bad"), ("filename", "../other"), ("size", -1), ("sha256", "invalid")]:
            invalid = copy.deepcopy(manifest)
            invalid[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                bundle.validate_manifest(invalid, config, "amd64")

    def test_only_missing_assets_allow_source_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            for code in [404, 403, 500]:
                error = urllib.error.HTTPError("https://example.com", code, "test", {}, None)
                expected = bundle.BundleUnavailable if code == 404 else urllib.error.HTTPError
                with self.subTest(code=code), patch.object(bundle.urllib.request, "urlopen", side_effect=error), patch.object(bundle.time, "sleep"), self.assertRaises(expected):
                    bundle.download("https://example.com", Path(directory) / "download", 123)


class LocalPackageTests(unittest.TestCase):
    def test_install_key_matches_upstream_formula(self):
        # A real macOS install: ~/.hermes/installs/3daa46fffbd74d1d.
        with patch.object(package.Path, "resolve", lambda self: self):
            self.assertEqual(package.install_key("/Users/kennethphang/.hermes/hermes-agent"), "3daa46fffbd74d1d")

    def test_windows_path_needles_cover_json_and_forward_slash_spellings(self):
        needles = package.path_needles("C:\\Users\\runneradmin\\AppData\\Local\\hermes", windows=True)
        self.assertIn("C:\\Users\\runneradmin\\AppData\\Local\\hermes", needles)
        self.assertIn("C:/Users/runneradmin/AppData/Local/hermes", needles)
        self.assertIn("C:\\\\Users\\\\runneradmin\\\\AppData\\\\Local\\\\hermes", needles)
        self.assertEqual(package.path_needles("/Users/runner/.hermes", windows=False), ["/Users/runner/.hermes"])

    def test_path_references_list_text_files_and_report_binaries(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            state = home / "installs/abc/inputs"
            state.mkdir(parents=True)
            (state / ".project-root").write_text("/Users/runner/.hermes/hermes-agent")
            (home / "installs/abc/facts.json").write_text('{"env": "/users/RUNNER/.hermes/x"}')
            native = home / "tools/node/bin/node"
            native.parent.mkdir(parents=True)
            native.write_bytes(b"\xff\xfe/Users/runner/.hermes/tools\x00")
            (home / "tools/node/clean.txt").write_text("nothing here")
            cached = home / "hermes-agent/hermes_cli/__pycache__/x.cpython-314.pyc"
            cached.parent.mkdir(parents=True)
            cached.write_bytes(b"/Users/runner/.hermes/hermes-agent")
            launcher = home / "hermes-agent/.hermes/bin/hermes"
            launcher.parent.mkdir(parents=True)
            launcher.write_text("exec /Users/runner/.hermes/tools/python")
            text, binary = package.find_path_references(home, package.LOCAL_ROOTS, ["/Users/runner/.hermes"], windows=False)
            self.assertEqual(text, ["installs/abc/inputs/.project-root"])
            self.assertEqual(binary, ["tools/node/bin/node"])
            # Windows paths are case-insensitive.
            text, _ = package.find_path_references(home, package.LOCAL_ROOTS, ["/Users/runner/.hermes"], windows=True)
            self.assertIn("installs/abc/facts.json", text)

    def test_local_package_omits_launchers_and_rejects_absolute_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            launcher = home / "hermes-agent/.hermes/bin/hermes"
            launcher.parent.mkdir(parents=True)
            launcher.write_text("launcher")
            (home / "hermes-agent/run_agent.py").write_text("print()")
            output = home / "local.tar.gz"
            with tarfile.open(output, "w:gz") as archive:
                archive.add(home / "hermes-agent", arcname="hermes-agent", filter=package.include_local_software)
            with tarfile.open(output) as archive:
                names = archive.getnames()
            self.assertIn("hermes-agent/run_agent.py", names)
            self.assertNotIn("hermes-agent/.hermes/bin/hermes", names)
            link = tarfile.TarInfo("tools/python/bin/python3")
            link.type, link.linkname = tarfile.SYMTYPE, "/Users/runner/.hermes/tools/python/bin/python3.14"
            with self.assertRaisesRegex(ValueError, "Absolute symlink"):
                package.include_local_software(link)
            outside = tarfile.TarInfo("Library/Preferences/x.plist")
            with self.assertRaisesRegex(ValueError, "Not a software path"):
                package.include_local_software(outside)


class ParallelInstallTests(unittest.TestCase):
    def test_foreground_refreshes_indexes_and_installs_runtime_without_full_upgrade(self):
        script = r'''
set -euo pipefail
source "$HERMES_HELPER"
hermes_status() { printf '%s\n' "$2"; }
apt-get() { printf 'APT:'; printf ' %s' "$@"; printf '\n'; }
hermes_os_setup
'''
        result = subprocess.run(["bash", "-c", script], text=True, capture_output=True, timeout=5,
                                env=dict(os.environ, HERMES_HELPER=str(ROOT / "crates/clawmacdo-cli/src/commands/hermes_install.sh")))
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = [line.split() for line in result.stdout.splitlines() if line.startswith("APT:")]
        self.assertEqual(len(commands), 2)
        self.assertIn("update", commands[0])
        self.assertIn("install", commands[1])
        self.assertTrue(all("upgrade" not in command for command in commands))

    def run_setup(self, os_status=0, bundle_status=0):
        with tempfile.TemporaryDirectory() as directory:
            # A serial implementation deadlocks: each job waits for the other to start.
            script = r'''
set -euo pipefail
source "$HERMES_HELPER"
hermes_os_setup() {
  touch "$TEST_DIR/os-started"
  while [ ! -f "$TEST_DIR/bundle-started" ]; do sleep 0.01; done
  sleep 0.05
  touch "$TEST_DIR/os-finished"
  return "$OS_RESULT"
}
hermes_bundle_install() {
  touch "$TEST_DIR/bundle-started"
  while [ ! -f "$TEST_DIR/os-started" ]; do sleep 0.01; done
  touch "$TEST_DIR/bundle-finished"
  return "$BUNDLE_RESULT"
}
hermes_source_install() {
  test -f "$TEST_DIR/os-finished"
  echo source-fallback
}
hermes_parallel_setup
test -f "$TEST_DIR/os-finished"
test -f "$TEST_DIR/bundle-finished"
echo ready-for-services
'''
            environment = dict(os.environ, TEST_DIR=directory, OS_RESULT=str(os_status), BUNDLE_RESULT=str(bundle_status),
                               HERMES_HELPER=str(ROOT / "crates/clawmacdo-cli/src/commands/hermes_install.sh"))
            result = subprocess.run(["bash", "-c", script], env=environment, text=True, capture_output=True, timeout=5)
            self.assertTrue((Path(directory) / "os-finished").exists())
            self.assertTrue((Path(directory) / "bundle-finished").exists())
            return result

    def test_os_and_bundle_overlap_and_join_before_services(self):
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ready-for-services", result.stdout)
        self.assertNotIn("source-fallback", result.stdout)

    def test_failure_of_either_job_blocks_service_startup(self):
        for os_status, bundle_status in [(17, 0), (0, 18), (17, 42)]:
            with self.subTest(os_status=os_status, bundle_status=bundle_status):
                result = self.run_setup(os_status, bundle_status)
                self.assertEqual(result.returncode, os_status or bundle_status)
                self.assertNotIn("ready-for-services", result.stdout)
                self.assertNotIn("source-fallback", result.stdout)

    def test_source_fallback_waits_for_os_package_manager(self):
        result = self.run_setup(bundle_status=42)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("source-fallback", result.stdout)
        self.assertIn("ready-for-services", result.stdout)


class BackgroundUpgradeTests(unittest.TestCase):
    def run_upgrade(self, apt_status=0, reboot_required=False):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            state = directory / "state"
            reboot = directory / "reboot-required"
            if reboot_required:
                reboot.touch()
            # Fail safely if a regression tries to reboot or manage live services.
            for command in ["reboot", "shutdown", "systemctl", "sudo"]:
                stub = directory / command
                stub.write_text("#!/bin/sh\nexit 99\n")
                stub.chmod(0o755)
            binary = directory / "apt-get"
            binary.write_text('''#!/bin/bash
set -euo pipefail
test "$DEBIAN_FRONTEND" = noninteractive
test "$NEEDRESTART_MODE" = l
test "$(cat "$TEST_STATE/status")" = running
printf '%s\\n' "$@" > "$TEST_ARGS"
exit "$APT_RESULT"
''')
            binary.chmod(0o755)
            script = (ROOT / "crates/clawmacdo-cli/src/commands/hermes_os_upgrade.sh").read_text()
            script = script.replace("/var/lib/clawmacdo/hermes-os-upgrade", str(state))
            script = script.replace("/var/run/reboot-required", str(reboot))
            result = subprocess.run(["bash", "-c", script], text=True, capture_output=True, timeout=5,
                                    env=dict(os.environ, PATH=str(directory) + os.pathsep + os.environ["PATH"],
                                             TEST_STATE=str(state), TEST_ARGS=str(directory / "args"), APT_RESULT=str(apt_status)))
            args = (directory / "args").read_text().splitlines()
            self.assertIn("upgrade", args)
            self.assertIn("DPkg::Lock::Timeout=300", args)
            self.assertIn("Dpkg::Options::=--force-confold", args)
            self.assertFalse((state / "status.tmp").exists())
            return result, (state / "status").read_text().strip()

    def test_upgrade_reports_completion(self):
        result, status = self.run_upgrade()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(status, "completed")

    def test_upgrade_failure_is_recorded_and_returns_nonzero(self):
        result, status = self.run_upgrade(apt_status=42)
        self.assertEqual(result.returncode, 42)
        self.assertIn("failed (exit 42)", status)
        self.assertNotIn("completed", result.stdout)

    def test_required_reboot_is_reported_without_rebooting(self):
        result, status = self.run_upgrade(reboot_required=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(status, "completed; reboot required (not performed automatically)")


if __name__ == "__main__":
    unittest.main()
