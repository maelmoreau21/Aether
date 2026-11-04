"""A stalled Docker exec must fail and still remove the disposable container."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("smoke", Path(__file__).with_name("smoke-docker-images.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class HealthTimeoutTests(unittest.TestCase):
    def test_cached_fixture_must_still_contain_real_av1(self):
        smoke.validate_fixture_probe({"streams": [{"codec_type": "video", "codec_name": "av1"},
                                                  {"codec_type": "audio", "codec_name": "opus"}]})
        for streams in [[], [{"codec_type": "audio", "codec_name": "opus"}],
                        [{"codec_type": "video", "codec_name": "h264"}]]:
            with self.subTest(streams=streams), self.assertRaises(ValueError):
                smoke.validate_fixture_probe({"streams": streams})

    def test_daemon_exec_timeout_propagates_and_cleanup_remains_bounded(self):
        calls = []

        def docker(args, **kwargs):
            calls.append((args, kwargs))
            if args[1] == "exec":
                self.assertEqual(kwargs["timeout"], 10)
                raise subprocess.TimeoutExpired(args, 10)
            self.assertEqual(args[:3], ["docker", "rm", "--force"])
            self.assertEqual(kwargs["timeout"], 30)
            return subprocess.CompletedProcess(args, 0)

        with patch.object(smoke, "command"), patch.object(smoke.subprocess, "run", side_effect=docker):
            with self.assertRaises(subprocess.TimeoutExpired):
                smoke.health("fixture", "core", "linux/amd64")
        self.assertEqual(len(calls), 2)


if __name__ == "__main__":
    unittest.main()
