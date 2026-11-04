"""Regression checks for disposable fixtures mounted into non-root images."""
import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "smoke_docker_images", Path(__file__).with_name("smoke-docker-images.py")
)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class FixtureStagingTests(unittest.TestCase):
    def test_supplied_fixture_is_copied_without_changing_source(self):
        with tempfile.TemporaryDirectory() as original, tempfile.TemporaryDirectory() as shared:
            source = Path(original) / "private-source.mkv"
            source.write_bytes(b"synthetic fixture payload")
            source.chmod(0o600)
            source_mode = source.stat().st_mode
            parent_mode = source.parent.stat().st_mode

            staged = SMOKE.stage_fixture(source, Path(shared))

            self.assertNotEqual(source.resolve(), staged.resolve())
            self.assertFalse(os.path.samefile(source, staged))
            self.assertEqual(staged.read_bytes(), b"synthetic fixture payload")
            self.assertEqual(source.read_bytes(), b"synthetic fixture payload")
            self.assertEqual(source.stat().st_mode, source_mode)
            self.assertEqual(source.parent.stat().st_mode, parent_mode)
            staged.write_bytes(b"disposable copy only")
            self.assertEqual(source.read_bytes(), b"synthetic fixture payload")

    @unittest.skipUnless(os.name == "posix", "requires POSIX permission bits")
    def test_generated_fixture_is_readable_without_making_it_world_writable(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = directory / "av1-fixture.mkv"
            fixture.write_bytes(b"generated fixture")
            fixture.chmod(0o600)
            self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o700)

            staged = SMOKE.stage_fixture(fixture, directory)

            self.assertEqual(staged, fixture)
            self.assertEqual(staged.read_bytes(), b"generated fixture")
            self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o755)
            self.assertEqual(stat.S_IMODE(staged.stat().st_mode), 0o644)

    @unittest.skipUnless(
        os.name == "posix" and os.geteuid() == 0,
        "requires POSIX root to check actual access as container UID 10001",
    )
    def test_container_uid_can_read_staged_copy_but_not_private_source(self):
        with tempfile.TemporaryDirectory() as original, tempfile.TemporaryDirectory() as shared:
            source = Path(original) / "private-source.mkv"
            source.write_bytes(b"synthetic fixture payload")
            source.chmod(0o600)
            staged = SMOKE.stage_fixture(source, Path(shared))

            def read_as_container(path):
                return subprocess.run(
                    ["/bin/cat", str(path)],
                    user=10001, group=10001, extra_groups=[], capture_output=True, timeout=10,
                )

            self.assertNotEqual(read_as_container(source).returncode, 0)
            result = read_as_container(staged)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            self.assertEqual(result.stdout, b"synthetic fixture payload")
            self.assertEqual(stat.S_IMODE(source.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(source.parent.stat().st_mode), 0o700)


if __name__ == "__main__":
    unittest.main()
