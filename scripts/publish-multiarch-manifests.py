"""Validate both verified architectures before explicitly publishing any final tag."""
import argparse
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
            refs.append(repository + "@" + entry["digest"])
        output.append({"image": image, "tags": common_tags, "references": refs})
    return output


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
    for image in planned:
        command = ["docker", "buildx", "imagetools", "create"]
        for tag in image["tags"]:
            command += ["--tag", tag]
        subprocess.run(command + image["references"], check=True)
        raw = subprocess.check_output(["docker", "buildx", "imagetools", "inspect", "--raw", image["tags"][0]])
        platforms = {(entry.get("platform", {}).get("os"), entry.get("platform", {}).get("architecture")) for entry in json.loads(raw)["manifests"]}
        if not {("linux", architecture) for architecture in ARCHITECTURES}.issubset(platforms):
            raise RuntimeError("Published index is missing a validated architecture")


if __name__ == "__main__":
    main()
