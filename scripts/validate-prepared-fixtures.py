"""Fail early on incomplete Rust CI fixture caches; no media modification."""
import argparse
import json
from pathlib import Path


def validate(root):
    files = ["video.mp4", "audio-fr.mp4", "audio-en.mp4",
             "fr-full.vtt", "fr-forced.vtt", "en-full.vtt"]
    required = [root / directory / name
                for directory in ["mse/verified", "dash"] for name in files]
    required.append(root / "dash/manifest.mpd")
    required.extend(root / "mse/verified" / f"{track}-{part}.{extension}"
                    for track in ["video", "audio-fr", "audio-en"]
                    for part, extension in [("init", "mp4"), ("media", "m4s")])
    for directory in ["mse/verified", "dash"]:
        report = root / directory / "generation.json"
        required.append(report)
    missing = [str(path) for path in required
               if not path.is_file() or path.stat().st_size == 0]
    if missing:
        raise ValueError("Incomplete P0 fixture cache (regenerate MSE/DASH or invalidate cache): "
                         + ", ".join(missing))
    for directory in ["mse/verified", "dash"]:
        report = json.loads((root / directory / "generation.json").read_text(encoding="utf-8-sig"))
        if report.get("passed") is not True or report.get("originals_unchanged") is not True:
            raise ValueError(f"Unverified P0 fixture preparation: {directory}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("tests/fixtures/generated"))
    args = parser.parse_args()
    try:
        validate(args.root)
    except (ValueError, OSError) as error:
        parser.exit(1, f"{error}\n")
    print("Prepared P0 MSE/DASH fixtures: complete and verified")
