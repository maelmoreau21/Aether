"""Validate both verified architectures before explicitly publishing any final tag."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

IMAGES = ("aether", "aether-core", "aether-node")
ARCHITECTURES = ("amd64", "arm64")


def plan(directory, owner, revision):
    if not re.fullmatch(r"[A-Za-z0-9-]+", owner) or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Invalid repository owner or exact revision")
    output = []
    for image in IMAGES:
        repository = f"ghcr.io/{owner.lower()}/{image}"
        refs = []
        common_tags = None
        for architecture in ARCHITECTURES:
            entry = json.loads((Path(directory) / architecture / f"{image}.json").read_text(encoding="utf-8"))
            if (entry["image"], entry["architecture"], entry["revision"]) != (image, architecture, revision):
                raise ValueError("Digest artifact identity mismatch")
            if not re.fullmatch(r"sha256:[0-9a-f]{64}", entry["digest"]):
                raise ValueError("Invalid immutable digest")
            tags = []
            suffix = "-" + architecture
            for tag in entry["tags"]:
                if not isinstance(tag, str) or not tag.startswith(repository + ":") or not tag.endswith(suffix):
                    raise ValueError("Tag repository or architecture mismatch")
                tag = tag[:-len(suffix)]
                if not re.fullmatch(re.escape(repository) + r":[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}", tag):
                    raise ValueError("Invalid final tag")
                tags.append(tag)
            if not tags or f"{repository}:sha-{revision}" not in tags:
                raise ValueError("Exact revision tag missing")
            tags = sorted(set(tags))
            if common_tags is not None and common_tags != tags:
                raise ValueError("Architectures disagree on final tags")
            common_tags = tags
            if not re.fullmatch(r"sha256:[0-9a-f]{64}", entry.get("image_id", "")):
                raise ValueError("Verified native image ID missing")
            refs.append({"reference": repository + "@" + entry["digest"],
                         "architecture": architecture, "image_id": entry["image_id"]})
        output.append({"image": image, "tags": common_tags, "references": refs})
    return output


def validate_remote(planned):
    # Inspect every immutable registry manifest before writing any final tag.
    # Classic Docker IDs identify the config; containerd image-store IDs can
    # identify the manifest. Bind to the exact tested bytes in either store.
    for image in planned:
        for entry in image["references"]:
            raw = subprocess.check_output(["docker", "buildx", "imagetools", "inspect", "--raw", entry["reference"]])
            manifest = json.loads(raw)
            identities = {manifest.get("config", {}).get("digest"), "sha256:" + hashlib.sha256(raw).hexdigest()}
            if entry["image_id"] not in identities:
                raise ValueError("Registry config differs from verified native image")


def validate_index(manifest, image):
    actual = {(item.get("platform", {}).get("os"), item.get("platform", {}).get("architecture"), item.get("digest"))
              for item in manifest.get("manifests", [])}
    expected = {("linux", entry["architecture"], entry["reference"].split("@")[1]) for entry in image["references"]}
    if actual != expected or len(manifest["manifests"]) != 2:
        raise ValueError("Index must contain exactly the two verified native digests")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--owner", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--publish", action="store_true", help="Perform registry writes; omitted means review only")
    args = parser.parse_args()
    # Build the complete plan first: a missing/invalid architecture cannot publish any image.
    planned = plan(args.directory, args.owner, args.revision)
    print(json.dumps(planned, indent=2))
    if not args.publish:
        return
    validate_remote(planned)
    # Preflight the complete set of index descriptors before any public tag moves.
    for image in planned:
        refs = [entry["reference"] for entry in image["references"]]
        raw = subprocess.check_output(["docker", "buildx", "imagetools", "create", "--dry-run", *refs])
        validate_index(json.loads(raw), image)
    for image in planned:
        command = ["docker", "buildx", "imagetools", "create"]
        for tag in image["tags"]:
            command += ["--tag", tag]
        subprocess.run(command + [entry["reference"] for entry in image["references"]], check=True)
        raw = subprocess.check_output(["docker", "buildx", "imagetools", "inspect", "--raw", image["tags"][0]])
        validate_index(json.loads(raw), image)
        descriptor = subprocess.check_output(["docker", "buildx", "imagetools", "inspect", image["tags"][0],
                                              "--format", "{{json .Manifest}}"], text=True)
        digest = json.loads(descriptor)["digest"]
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise ValueError("Invalid published index digest")
        (args.directory / (image["image"] + ".digest")).write_text(digest + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
