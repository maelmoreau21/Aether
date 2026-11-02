"""Exercise the real shell downloader offline, stopping before source extraction."""
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("build-ffmpeg.sh")
ARCHIVE = b"synthetic pinned FFmpeg source archive"


@unittest.skipUnless(os.name == "posix" and shutil.which("bash"), "requires POSIX shell tools")
class SourceDownloadTests(unittest.TestCase):
    def run_download(self, mode):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            commands = root / "bin"
            commands.mkdir()
            script = root / "build.sh"
            # Test a synthetic release hash using the real sha256sum command.
            script.write_text(re.sub(r"(?m)^sha256=[0-9a-f]{64}$",
                                    "sha256=" + hashlib.sha256(ARCHIVE).hexdigest(),
                                    SCRIPT.read_text()), encoding="utf-8")
            curl = commands / "curl"
            curl.write_text('''#!/bin/sh
printf '%s\\n' "$*" >> "$DOWNLOAD_LOG"
output=
source=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) output=$2; shift ;;
    https:*) source=$1 ;;
  esac
  shift
done
case "$DOWNLOAD_MODE:$source" in
  offline:*) printf partial > "$output"; exit 35 ;;
  fallback:https://ffmpeg.org/*|corrupt_mirror:https://ffmpeg.org/*)
    printf partial > "$output"; exit 35 ;;
  corrupt_primary:*|corrupt_mirror:*) printf corrupted > "$output" ;;
  *) printf 'synthetic pinned FFmpeg source archive' > "$output" ;;
esac
''', encoding="utf-8")
            curl.chmod(0o755)
            tar = commands / "tar"
            tar.write_text("#!/bin/sh\nexit 42\n", encoding="utf-8")
            tar.chmod(0o755)
            log = root / "downloads.log"
            result = subprocess.run(["bash", str(script), "/unused", "runtime"],
                                    env={**os.environ, "PATH": str(commands) + os.pathsep + os.environ["PATH"],
                                         "DOWNLOAD_MODE": mode, "DOWNLOAD_LOG": str(log)},
                                    capture_output=True, text=True, timeout=10)
            return result, log.read_text().splitlines()

    def test_primary_archive_passes_hash_before_extraction(self):
        result, requests = self.run_download("primary")
        self.assertEqual(result.returncode, 42, result.stderr)
        self.assertEqual(len(requests), 1)
        self.assertIn("--retry-all-errors", requests[0])
        self.assertIn("--proto =https --proto-redir =https", requests[0])

    def test_tls_failure_falls_back_and_overwrites_partial_download(self):
        result, requests = self.run_download("fallback")
        self.assertEqual(result.returncode, 42, result.stderr)
        self.assertEqual(len(requests), 2)
        self.assertIn("distfiles.macports.org", requests[1])

    def test_all_hosts_unavailable_stops_before_extraction(self):
        result, requests = self.run_download("offline")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(requests), 2)
        self.assertIn("Unable to download", result.stderr)

    def test_corrupt_archive_is_never_extracted_from_either_host(self):
        for mode, attempts in [("corrupt_primary", 1), ("corrupt_mirror", 2)]:
            with self.subTest(mode=mode):
                result, requests = self.run_download(mode)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(len(requests), attempts)
                self.assertIn("checksum mismatch", result.stderr)


if __name__ == "__main__":
    unittest.main()
