"""Reject archive substitution before loading or promoting any image."""
import copy
import hashlib
import importlib.util
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("archive", Path(__file__).with_name("ci-image-archive.py"))
archive = importlib.util.module_from_spec(spec)
spec.loader.exec_module(archive)


class ArchiveTests(unittest.TestCase):
    def manifest(self):
        return {"schema": 1, "revision": "a" * 40, "architecture": "arm64", "version": "0.2.0-beta.1",
                "source": "https://github.com/fixture/Aether", "archive_sha256": "1" * 64,
                "images": {role: "sha256:" + "2" * 64 for role in archive.TARGETS}}

    def test_identity_requires_exact_commit_platform_version_and_source(self):
        value = self.manifest()
        archive.verify_identity(value, value["revision"], "arm64", value["version"], value["source"])
        for key, bad in [("schema", 2), ("revision", "b" * 40), ("architecture", "amd64"),
                         ("version", "0.2.0"), ("source", "https://foreign.invalid"),
                         ("archive_sha256", "invalid"), ("images", {})]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                changed = copy.deepcopy(value)
                changed[key] = bad
                archive.verify_identity(changed, value["revision"], "arm64", value["version"], value["source"])

    def test_loaded_config_must_be_the_same_image_that_passed_smoke(self):
        manifest = self.manifest()
        image = {"Id": "sha256:" + "2" * 64, "Os": "linux", "Architecture": "arm64",
                 "Config": {"Labels": {"org.opencontainers.image." + k: manifest[k]
                                        for k in ("source", "version", "revision")}}}
        with patch.object(archive, "inspect", return_value=image):
            archive.verify_images(manifest)
        for key, bad in [("Id", "sha256:" + "3" * 64), ("Os", "windows"), ("Architecture", "amd64"),
                         ("Config", {"Labels": {}})]:
            with self.subTest(key=key), patch.object(archive, "inspect", return_value={**image, key: bad}):
                with self.assertRaises(ValueError):
                    archive.verify_images(manifest)

    def test_checksum_covers_the_archive_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            file = Path(temporary) / "archive"
            file.write_bytes(b"original image archive")
            expected = hashlib.sha256(file.read_bytes()).hexdigest()
            self.assertEqual(archive.checksum(file), expected)
            file.write_bytes(b"substituted image archive")
            self.assertNotEqual(archive.checksum(file), expected)


if __name__ == "__main__":
    unittest.main()
