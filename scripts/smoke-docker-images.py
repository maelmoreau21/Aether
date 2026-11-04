"""Run disposable, network-isolated image checks. Never publish or mount user data."""
import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time
import uuid


def command(args, timeout=90, binary=False):
    result = subprocess.run(args, capture_output=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"Command failed: {' '.join(args[:6])}: {result.stderr.decode(errors='replace')[-2000:]}")
    return result.stdout if binary else result.stdout.decode(errors="replace").strip()


def jpeg_dimensions(data):
    if not data.startswith(b"\xff\xd8"):
        raise ValueError("Not a JPEG")
    offset = 2
    while offset + 4 <= len(data):
        if data[offset] != 255:
            raise ValueError("Invalid JPEG marker")
        marker = data[offset + 1]
        offset += 2
        if marker in (216, 217) or 208 <= marker <= 215:
            continue
        length = struct.unpack(">H", data[offset:offset + 2])[0]
        if marker in (192, 193, 194):
            height, width = struct.unpack(">HH", data[offset + 3:offset + 7])
            return width, height
        if length < 2:
            raise ValueError("Invalid JPEG marker size")
        offset += length
    raise ValueError("JPEG dimensions unavailable")


def stage_fixture(source, directory):
    """Share only a disposable fixture copy with the image's non-root user."""
    fixture = directory / "av1-fixture.mkv"
    if source.resolve() != fixture.resolve():
        shutil.copyfile(source, fixture)
    # TemporaryDirectory is 0700 on POSIX; its bind-mounted root must be
    # traversable by UID 10001. Never change permissions on a supplied source.
    directory.chmod(0o755)
    fixture.chmod(0o644)
    return fixture


def preview(image, fixture, output, platform):
    args = ["docker", "run", "--rm", "--network", "none", "--platform", platform,
            "--mount", f"type=bind,source={fixture.parent.resolve()},target=/fixtures,readonly",
            "--entrypoint", "/usr/local/bin/ffmpeg", image, "-v", "error", "-nostdin",
            "-threads", "1", "-filter_threads", "1", "-ss", "1", "-i", f"/fixtures/{fixture.name}",
            "-map", "0:v:0", "-an", "-sn", "-dn", "-vf", "scale=320:-2:flags=fast_bilinear,format=yuvj420p",
            "-frames:v", "1", "-q:v", "7", "-c:v", "mjpeg", "-f", "image2pipe", "pipe:1"]
    started = time.monotonic()
    data = command(args, binary=True)
    width, height = jpeg_dimensions(data)
    if width != 320 or height <= 0:
        raise ValueError("Unexpected generated preview dimensions")
    output.write_bytes(data)
    return {"width": width, "height": height, "bytes": len(data), "elapsedSeconds": round(time.monotonic() - started, 3)}


def health(image, role, platform):
    name = "media-image-smoke-" + uuid.uuid4().hex
    command(["docker", "run", "--detach", "--rm", "--network", "none", "--platform", platform,
             "--name", name, image])
    endpoint = "http://127.0.0.1:4000/internal/v1/health" if role == "node" else "http://127.0.0.1:3000/internal/v1/health" if role == "core" else "http://127.0.0.1:3000/api/v1/health"
    try:
        started = time.monotonic()
        while time.monotonic() - started < 60:
            # curl's timeout cannot bound a stalled Docker daemon/exec request.
            result = subprocess.run(["docker", "exec", name, "curl", "--fail", "--silent", "--max-time", "2", endpoint], capture_output=True, timeout=10)
            if result.returncode == 0:
                return {"passed": True, "elapsedSeconds": round(time.monotonic() - started, 3)}
            time.sleep(1)
        raise RuntimeError(f"Disposable {role} image did not become healthy")
    finally:
        subprocess.run(["docker", "rm", "--force", name], capture_output=True, timeout=30)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--standalone", default="media-ci:runtime")
    parser.add_argument("--core", default="media-ci:runtime-core")
    parser.add_argument("--node", default="media-ci:runtime-node")
    parser.add_argument("--arch", required=True, choices=["amd64", "arm64"])
    parser.add_argument("--version", default="")
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--report", type=Path, default=Path("target/docker-smoke/report.json"))
    args = parser.parse_args()
    args.report.parent.mkdir(parents=True, exist_ok=True)
    report = {"passed": False, "architecture": args.arch, "executionHost": os.uname().machine if hasattr(os, "uname") else os.environ.get("PROCESSOR_ARCHITECTURE"), "scope": "Disposable containers and generated fixture; no physical ARM/NAS or playback proof", "images": {}}
    report["dockerServerVersion"] = command(["docker", "version", "--format", "{{.Server.Version}}"])
    report["dockerServerArchitecture"] = command(["docker", "info", "--format", "{{.Architecture}}"])
    native = {"x86_64": "amd64", "aarch64": "arm64"}.get(report["dockerServerArchitecture"], report["dockerServerArchitecture"])
    report["executionKind"] = "native-container" if native == args.arch else "platform-emulation"
    platform = "linux/" + args.arch
    with tempfile.TemporaryDirectory(prefix="media-image-fixture-") as temporary:
        fixture = args.fixture or Path(temporary) / "av1-fixture.mkv"
        if args.fixture is None:
            report["fixtureGenerator"] = command(["ffmpeg", "-version"]).splitlines()[0]
            command(["ffmpeg", "-v", "error", "-nostdin", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=5",
                     "-t", "3", "-c:v", "libaom-av1", "-cpu-used", "8", "-crf", "45", "-an", str(fixture)])
        try:
            fixture = stage_fixture(fixture, Path(temporary))
            for role, image, binary in [("standalone", args.standalone, "aether"), ("core", args.core, "aether-core"), ("node", args.node, "aether-node")]:
                inspected = json.loads(command(["docker", "image", "inspect", image]))[0]
                if inspected["Architecture"] != args.arch:
                    raise ValueError(f"{role} architecture mismatch")
                config = inspected["Config"]
                if not config.get("Healthcheck") or config.get("User") in (None, "", "root", "0", "0:0"):
                    raise ValueError(f"{role} needs non-root user and healthcheck")
                uid = command(["docker", "run", "--rm", "--network", "none", "--platform", platform, "--entrypoint", "id", image, "-u"])
                if uid != "10001":
                    raise ValueError(f"{role} unexpected UID")
                version = command(["docker", "run", "--rm", "--network", "none", "--platform", platform, "--entrypoint", binary, image, "--version"])
                if args.version and args.version not in version:
                    raise ValueError(f"{role} version mismatch")
                result = {"id": inspected["Id"], "bytes": inspected["Size"], "uid": uid, "version": version, "health": health(image, role, platform)}
                report["images"][role] = result
                if role != "core":
                    result["ffmpeg"] = command(["docker", "run", "--rm", "--network", "none", "--platform", platform, "--entrypoint", "ffmpeg", image, "-version"]).splitlines()[0]
                    result["ffprobe"] = command(["docker", "run", "--rm", "--network", "none", "--platform", platform, "--entrypoint", "ffprobe", image, "-version"]).splitlines()[0]
                    if "9.0.2" not in result["ffmpeg"] or "9.0.2" not in result["ffprobe"]:
                        raise ValueError("Embedded FFmpeg version mismatch")
                    result["chapterPreview"] = preview(image, fixture, args.report.parent / f"{role}-{args.arch}.jpg", platform)
                if role == "node" and inspected["Size"] >= 200_000_000:
                    raise ValueError("Node image exceeds existing 200 MB gate")
            report["passed"] = True
        finally:
            args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Three {platform} images passed isolated smoke checks: {args.report}")


if __name__ == "__main__":
    main()
