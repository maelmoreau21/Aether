import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FIXTURES_DIR = ROOT / "target" / "ffprobe_matrix_fixtures"
FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

def generate_fixtures():
    def run_ffmpeg(*args):
        cmd = ["ffmpeg", "-y", "-v", "error", *args]
        res = subprocess.run(cmd, capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"FFmpeg failed: {res.stderr}\nCmd: {' '.join(cmd)}")

    h264_aac_mp4 = FIXTURES_DIR / "h264_aac.mp4"
    if not h264_aac_mp4.exists():
        run_ffmpeg(
            "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:a", "aac",
            "-movflags", "+faststart",
            str(h264_aac_mp4)
        )

    h264_aac_mkv = FIXTURES_DIR / "h264_aac.mkv"
    if not h264_aac_mkv.exists():
        run_ffmpeg(
            "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:a", "aac",
            str(h264_aac_mkv)
        )

    hevc_ac3_mp4 = FIXTURES_DIR / "hevc_ac3.mp4"
    if not hevc_ac3_mp4.exists():
        run_ffmpeg(
            "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx265", "-pix_fmt", "yuv420p",
            "-c:a", "ac3",
            "-tag:v", "hvc1",
            "-movflags", "+faststart",
            str(hevc_ac3_mp4)
        )

    hevc_eac3_multitrack_mkv = FIXTURES_DIR / "hevc_eac3_multitrack.mkv"
    sub_srt = FIXTURES_DIR / "sample.srt"
    sub_srt.write_text("1\n00:00:00,000 --> 00:00:01,000\nHello Aether\n", encoding="utf-8")

    if not hevc_eac3_multitrack_mkv.exists():
        run_ffmpeg(
            "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=24",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-f", "lavfi", "-i", "sine=frequency=880:duration=1",
            "-i", str(sub_srt),
            "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s",
            "-c:v", "libx265", "-pix_fmt", "yuv420p",
            "-c:a:0", "eac3", "-metadata:s:a:0", "language=fre", "-metadata:s:a:0", "title=Français EAC3",
            "-c:a:1", "ac3", "-metadata:s:a:1", "language=eng", "-metadata:s:a:1", "title=English AC3",
            "-c:s", "subrip", "-metadata:s:s:0", "language=fre", "-metadata:s:s:0", "title=Français Forcé",
            str(hevc_eac3_multitrack_mkv)
        )

    fixtures = [
        ("av1_opus_mkv", ROOT / "tests/fixtures/generated/av1-opus.mkv", {"video": "av1", "audio": "opus"}),
        ("av1_opus_mp4", ROOT / "tests/fixtures/generated/av1-opus.mp4", {"video": "av1", "audio": "opus"}),
        ("representative_mkv", ROOT / "tests/fixtures/generated/representative/representative.mkv", {"video": "av1", "audio": "opus", "subtitles": True}),
        ("representative_mp4", ROOT / "tests/fixtures/generated/representative/representative.mp4", {"video": "av1", "audio": "opus"}),
        ("h264_aac_mp4", h264_aac_mp4, {"video": "h264", "audio": "aac"}),
        ("h264_aac_mkv", h264_aac_mkv, {"video": "h264", "audio": "aac"}),
        ("hevc_ac3_mp4", hevc_ac3_mp4, {"video": "hevc", "audio": "ac3"}),
        ("hevc_eac3_multitrack_mkv", hevc_eac3_multitrack_mkv, {"video": "hevc", "audio": ["eac3", "ac3"], "subtitles": True}),
    ]
    return fixtures

def probe_file(file_path, docker_image=None, ffprobe_binary="ffprobe"):
    args_base = [
        "-v", "error",
        "-protocol_whitelist", "fd,pipe,file",
        "-format_whitelist", "matroska,webm,mov,mp4",
        "-max_alloc", "67108864",
        "-probesize", "5000000",
        "-analyzeduration", "5000000",
        "-show_streams",
        "-show_format",
        "-show_data",
        "-of", "json",
        "-"
    ]
    with open(file_path, "rb") as f:
        data = f.read()

    if docker_image:
        cmd = ["docker", "run", "-i", "--rm", docker_image, "ffprobe", *args_base]
    else:
        cmd = [ffprobe_binary, *args_base]

    res = subprocess.run(cmd, input=data, capture_output=True, timeout=15)
    if res.returncode != 0:
        raise RuntimeError(f"ffprobe failed (code {res.returncode}): {res.stderr.decode('utf-8', errors='replace')}")
    return json.loads(res.stdout)

def verify_probe(result, expectations):
    streams = result.get("streams", [])
    assert len(streams) > 0, "No streams found"
    codecs_by_type = {}
    for s in streams:
        c_type = s.get("codec_type")
        c_name = s.get("codec_name")
        codecs_by_type.setdefault(c_type, []).append(c_name)
    
    # Check video
    if "video" in expectations:
        expected_v = expectations["video"]
        assert "video" in codecs_by_type, f"Missing video stream, found {codecs_by_type}"
        assert expected_v in codecs_by_type["video"], f"Expected video {expected_v}, got {codecs_by_type['video']}"
    
    # Check audio
    if "audio" in expectations:
        expected_a = expectations["audio"]
        if isinstance(expected_a, list):
            for ea in expected_a:
                assert ea in codecs_by_type.get("audio", []), f"Expected audio {ea}, got {codecs_by_type.get('audio')}"
        else:
            assert expected_a in codecs_by_type.get("audio", []), f"Expected audio {expected_a}, got {codecs_by_type.get('audio')}"

    # Check subtitles
    if expectations.get("subtitles"):
        assert "subtitle" in codecs_by_type, f"Expected subtitles, got {codecs_by_type}"

    assert "format" in result, "Missing format section"
    assert "duration" in result["format"] or any("duration" in s for s in streams), "Missing duration in probe"
    return codecs_by_type

if __name__ == "__main__":
    docker_image = None
    if len(sys.argv) > 1 and sys.argv[1].startswith("--docker="):
        docker_image = sys.argv[1].split("=", 1)[1]
    elif len(sys.argv) > 2 and sys.argv[1] == "--docker":
        docker_image = sys.argv[2]

    fixtures = generate_fixtures()
    target_desc = f"Docker image '{docker_image}'" if docker_image else "local ffprobe"
    print(f"Testing {target_desc} on {len(fixtures)} media configurations:")
    for name, path, expectations in fixtures:
        res = probe_file(path, docker_image=docker_image)
        found = verify_probe(res, expectations)
        print(f" [PASS] {name:<26}: {found}")
    print("\nALL MATRIX CHECKS PASSED SUCCESSFULLY.")
