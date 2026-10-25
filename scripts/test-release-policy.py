import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("calculate_release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

class ReleasePolicyTests(unittest.TestCase):
    def test_explicit_bumps_override_commit_messages(self):
        for bump, expected in [("PATCH", "0.4.8"), ("MINOR", "0.5.0"), ("MAJOR", "1.0.0")]:
            with self.subTest(bump=bump), patch.object(sys, "argv", ["release", "--bump", bump, "--json"]), patch.object(release, "get_latest_tag", return_value="v0.4.7"), patch.object(release, "run_git", return_value="abc\x00feat!: breaking feature\x00BREAKING CHANGE: contract\x1e"), contextlib.redirect_stdout(io.StringIO()) as output:
                release.main()
                self.assertEqual(json.loads(output.getvalue())["version"], expected)

    def test_auto_mode_remains_available_for_offline_previews(self):
        with patch.object(sys, "argv", ["release", "--json"]), patch.object(release, "get_latest_tag", return_value="v1.2.3"), patch.object(release, "run_git", return_value="abc\x00fix: repair\x00\x1e"), contextlib.redirect_stdout(io.StringIO()) as output:
            release.main()
            self.assertEqual(json.loads(output.getvalue())["version"], "1.2.4")

if __name__ == "__main__":
    unittest.main()
