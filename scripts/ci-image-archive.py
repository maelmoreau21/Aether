"""Bind a Docker archive to the exact native images that passed the smoke checks."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

TARGETS = {"standalone": "runtime", "core": "runtime-core", "node": "runtime-node"}


def inspect(reference):
    return json.loads(subprocess.check_output(["docker", "image", "inspect", reference]))[0]


def checksum(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_identity(manifest, revision, architecture, version, source):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Expected an exact commit SHA")
    expected = (1, revision, architecture, version, source)
    actual = tuple(manifest.get(k) for k in ("schema", "revision", "architecture", "version", "source"))
    if actual != expected or set(manifest.get("images", {})) != set(TARGETS):
        raise ValueError("Archive identity mismatch")
    if not re.fullmatch(r"[0-9a-f]{64}", manifest.get("archive_sha256", "")):
        raise ValueError("Invalid archive checksum")
    for identifier in manifest["images"].values():
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", identifier):
            raise ValueError("Invalid verified image ID")


def verify_images(manifest):
    for role, target in TARGETS.items():
        image = inspect("media-ci:" + target)
        labels = image["Config"].get("Labels", {})
        if (image["Id"] != manifest["images"][role] or image["Os"] != "linux"
                or image["Architecture"] != manifest["architecture"]):
            raise ValueError("Loaded image differs from verified native image")
        for key in ("revision", "source", "version"):
            if labels.get("org.opencontainers.image." + key) != manifest[key]:
                raise ValueError("OCI label mismatch: " + key)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["record", "load"])
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--arch", choices=["amd64", "arm64"], required=True)
    parser.add_argument("--version", default="")
    parser.add_argument("--source", required=True)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    archive = args.directory / "images.tar.gz"
    record = args.directory / "verified.json"
    if args.mode == "record":
        report = json.loads(args.report.read_text(encoding="utf-8"))
        if report.get("passed") is not True or report["architecture"] != args.arch:
            raise ValueError("Successful native smoke report required")
        if report["executionKind"] != "native-container":
            raise ValueError("Production archives require native validation")
        manifest = {"schema": 1, "revision": args.revision, "architecture": args.arch,
                    "version": args.version, "source": args.source,
                    "archive_sha256": checksum(archive),
                    "images": {role: report["images"][role]["id"] for role in TARGETS}}
    else:
        manifest = json.loads(record.read_text(encoding="utf-8"))
    verify_identity(manifest, args.revision, args.arch, args.version, args.source)
    if checksum(archive) != manifest["archive_sha256"]:
        raise ValueError("Docker archive checksum mismatch")
    if args.mode == "load":
        # Docker reads gzip directly. No build, registry lookup, or tag substitution.
        subprocess.run(["docker", "load", "--input", str(archive)], check=True)
    verify_images(manifest)
    if args.mode == "record":
        record.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"verified": True, "archive_bytes": archive.stat().st_size, **manifest}, indent=2))


if __name__ == "__main__":
    main()
