"""Offline publication-gate contracts; no registry writes or real credentials."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("multiarch", Path(__file__).with_name("publish-multiarch-manifests.py"))
multiarch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(multiarch)
REVISION = "a" * 40


class MultiArchitectureTests(unittest.TestCase):
    def fixtures(self, directory):
        for arch in multiarch.ARCHITECTURES:
            target = Path(directory) / arch
            target.mkdir()
            for image in multiarch.IMAGES:
                repository = "ghcr.io/fixture/" + image
                (target / f"{image}.json").write_text(json.dumps({"image": image, "architecture": arch, "revision": REVISION,
                    "digest": "sha256:" + ("1" if arch == "amd64" else "2") * 64,
                    "tags": [f"{repository}:develop-{arch}", f"{repository}:sha-{REVISION}-{arch}"]}), encoding="utf-8")

    def test_plan_requires_two_verified_architectures_for_each_image(self):
        with tempfile.TemporaryDirectory() as directory:
            self.fixtures(directory)
            plan = multiarch.plan(directory, "Fixture", REVISION)
            self.assertEqual(len(plan), 3)
            self.assertTrue(all(len(image["references"]) == 2 for image in plan))
            self.assertEqual(plan[0]["tags"], ["ghcr.io/fixture/aether:develop", f"ghcr.io/fixture/aether:sha-{REVISION}"])

    def test_missing_architecture_blocks_complete_plan(self):
        with tempfile.TemporaryDirectory() as directory:
            self.fixtures(directory)
            (Path(directory) / "arm64/aether-node.json").unlink()
            with self.assertRaises(FileNotFoundError):
                multiarch.plan(directory, "fixture", REVISION)

    def test_foreign_revision_repository_or_digest_is_rejected(self):
        for field, bad in [("revision", "b" * 40), ("architecture", "amd64"), ("digest", "sha256:invalid"),
                           ("tags", [f"ghcr.io/foreign/aether:sha-{REVISION}-arm64"])]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                self.fixtures(directory)
                path = Path(directory) / "arm64/aether.json"
                value = json.loads(path.read_text())
                value[field] = bad
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    multiarch.plan(directory, "fixture", REVISION)

    def test_architectures_must_agree_on_the_final_tags(self):
        with tempfile.TemporaryDirectory() as directory:
            self.fixtures(directory)
            path = Path(directory) / "arm64/aether.json"
            value = json.loads(path.read_text())
            value["tags"].append("ghcr.io/fixture/aether:latest-arm64")
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                multiarch.plan(directory, "fixture", REVISION)


if __name__ == "__main__":
    unittest.main()
