"""Native bundle integrity, profile isolation and extraction regression tests."""
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
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
config = json.loads((ROOT / "scripts/hermes-bundle/bundle.json").read_text())


class BundleTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
