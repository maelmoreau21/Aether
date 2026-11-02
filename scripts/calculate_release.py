#!/usr/bin/env python3
"""
calculate_release.py
Automated SemVer release calculation for Aether:
- Stable versions: MAJOR.MINOR.PATCH (e.g. 0.1.0, 0.2.0, 0.2.1)
- Development versions: MAJOR.MINOR.PATCH-beta.N (e.g. 0.2.0-beta.0, 0.2.0-beta.1)
- Git tag prefix: 'v' (e.g. v0.1.0, v0.2.0-beta.1)

Supports:
- --channel develop : increments beta counter for current cycle, idempotent on already tagged commits
- --channel main : calculates next stable release, verifies tag immutability and generates changelog
- --next-cycle : calculates next development cycle after a stable release
- --bump AUTO|PATCH|MINOR|MAJOR : explicit or automatic semver bump
"""

import argparse
import json
import os
import re
import subprocess
import sys

def run_git(args, cwd=None):
    res = subprocess.run(
        ["git"] + args,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True
    )
    return res.stdout.strip()

def get_all_tags(cwd=None):
    try:
        raw = run_git(["tag", "-l"], cwd=cwd)
        if not raw:
            return []
        return [t.strip() for t in raw.splitlines() if t.strip()]
    except Exception:
        return []

def get_tags_at_head(cwd=None):
    try:
        raw = run_git(["tag", "--points-at", "HEAD"], cwd=cwd)
        if not raw:
            return []
        return [t.strip() for t in raw.splitlines() if t.strip()]
    except Exception:
        return []

def get_tag_commit(tag, cwd=None):
    try:
        out = run_git(["rev-list", "-n", "1", tag], cwd=cwd)
        out_str = out.strip()
        if re.match(r"^[0-9a-fA-F]{7,40}$", out_str):
            return out_str
        return None
    except Exception:
        return None

def get_head_commit(cwd=None):
    try:
        out = run_git(["rev-parse", "HEAD"], cwd=cwd)
        out_str = out.strip()
        if re.match(r"^[0-9a-fA-F]{7,40}$", out_str):
            return out_str
        return None
    except Exception:
        return None

def parse_semver(tag_str):
    if not tag_str:
        return (0, 1, 0)
    cleaned = tag_str.lstrip("v")
    # Strip any pre-release suffix
    cleaned = cleaned.split("-")[0]
    parts = cleaned.split(".")
    try:
        major = int(parts[0])
        minor = int(parts[1]) if len(parts) > 1 else 0
        patch = int(parts[2]) if len(parts) > 2 else 0
        return (major, minor, patch)
    except (ValueError, IndexError):
        return (0, 1, 0)

def get_latest_stable_tag(cwd=None):
    tags = get_all_tags(cwd)
    stable_pattern = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
    stables = []
    for t in tags:
        m = stable_pattern.match(t)
        if m:
            stables.append((int(m.group(1)), int(m.group(2)), int(m.group(3)), t))
    if not stables:
        return None
    stables.sort(key=lambda item: (item[0], item[1], item[2]))
    return stables[-1][3]

# Alias for backward compatibility with existing tests
get_latest_tag = get_latest_stable_tag

def get_active_beta_cycle(latest_stable_tag, tags):
    """
    Determines the current active development cycle (e.g. '0.2.0').
    Finds the latest beta tags or increments minor from latest stable.
    """
    beta_pattern = re.compile(r"^v(\d+)\.(\d+)\.(\d+)-beta\.(\d+)$")
    betas_by_cycle = {}
    for t in tags:
        m = beta_pattern.match(t)
        if m:
            cycle = f"{m.group(1)}.{m.group(2)}.{m.group(3)}"
            betas_by_cycle.setdefault(cycle, []).append((int(m.group(4)), t))

    if betas_by_cycle:
        # Find latest cycle by SemVer
        def cycle_key(c):
            parts = [int(p) for p in c.split(".")]
            return (parts[0], parts[1], parts[2])
        sorted_cycles = sorted(betas_by_cycle.keys(), key=cycle_key)
        latest_cycle = sorted_cycles[-1]
        # Verify if this cycle was already released as stable
        if latest_stable_tag:
            stable_ver = latest_stable_tag.lstrip("v")
            if cycle_key(latest_cycle) <= cycle_key(stable_ver):
                # Cycle was released, start next minor cycle
                maj, min_, _ = parse_semver(latest_stable_tag)
                return f"{maj}.{min_ + 1}.0", []
        betas = betas_by_cycle[latest_cycle]
        betas.sort(key=lambda x: x[0])
        return latest_cycle, betas

    if latest_stable_tag:
        maj, min_, _ = parse_semver(latest_stable_tag)
        return f"{maj}.{min_ + 1}.0", []

    # Initial default cycle when no tags exist
    return "0.2.0", []

def generate_changelog(rev_range, next_tag, cwd=None):
    try:
        log_output = run_git(["log", rev_range, "--pretty=format:%H%x00%s%x00%b%x1e"], cwd=cwd)
    except Exception:
        log_output = ""

    entries = log_output.strip().split("\x1e") if log_output.strip() else []

    features = []
    fixes = []
    others = []
    has_major = False
    has_minor = False
    has_patch = False

    for entry in entries:
        parts = entry.strip().split("\x00")
        if len(parts) < 2:
            continue
        commit_hash = parts[0].strip()
        subject = parts[1].strip()
        body = parts[2].strip() if len(parts) > 2 else ""

        # Ignore pure merge commits
        if subject.startswith("Merge pull request ") or subject.startswith("Merge branch "):
            continue

        full_msg = f"{subject}\n{body}"
        is_breaking = False
        if "!:" in subject or "BREAKING CHANGE:" in full_msg or "BREAKING-CHANGE:" in full_msg:
            is_breaking = True
            has_major = True

        m = re.match(r"^([a-zA-Z]+)(\([^\)]+\))?!?: (.+)$", subject)
        if m:
            c_type = m.group(1).lower()
            scope = m.group(2) or ""
            desc = m.group(3)
            formatted = f"- {scope}{': ' if scope else ''}{desc} (`{commit_hash[:7]}`)"

            if is_breaking or c_type in ["feat"]:
                has_minor = True
                features.append(formatted)
            elif c_type in ["fix", "perf"]:
                has_patch = True
                fixes.append(formatted)
            else:
                others.append(formatted)
        else:
            if subject:
                others.append(f"- {subject} (`{commit_hash[:7]}`)")

    changelog_parts = [f"## Release {next_tag}\n"]
    if features:
        changelog_parts.append("### 🚀 Nouveautés & Évolutions")
        changelog_parts.extend(features)
        changelog_parts.append("")
    if fixes:
        changelog_parts.append("### 🐛 Corrections de bugs")
        changelog_parts.extend(fixes)
        changelog_parts.append("")
    if others:
        changelog_parts.append("### 🛠️ Autres améliorations & Maintenance")
        changelog_parts.extend(others)
        changelog_parts.append("")

    return "\n".join(changelog_parts).strip(), has_major, has_minor, has_patch

def main():
    parser = argparse.ArgumentParser(description="SemVer release calculation for Aether")
    parser.add_argument("--json", action="store_true", help="Output JSON format")
    parser.add_argument("--channel", choices=["auto", "develop", "main"], default="main", help="Release channel")
    parser.add_argument("--bump", choices=["AUTO", "PATCH", "MINOR", "MAJOR"], default="AUTO", help="SemVer bump type")
    parser.add_argument("--target-version", help="Explicit target version to release or initialize (e.g. 0.1.0)")
    parser.add_argument("--next-cycle", action="store_true", help="Calculate next cycle for develop after release")
    parser.add_argument("--check-release-pr", action="store_true", help="On main, verify if PR indicates release")
    parser.add_argument("--pr-title", default="", help="Pull Request title if running in CI")
    parser.add_argument("--pr-labels", default="", help="Comma-separated Pull Request labels")
    args = parser.parse_args()

    tags = get_all_tags()
    head_tags = get_tags_at_head()
    head_commit = get_head_commit()
    latest_stable = get_latest_tag()

    # Determine channel if auto
    channel = args.channel
    if channel == "auto":
        try:
            branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"])
            if branch == "develop":
                channel = "develop"
            else:
                channel = "main"
        except Exception:
            channel = "main"

    # 1. Mode: Next development cycle calculation
    if args.next_cycle:
        ref_stable = args.target_version or (latest_stable.lstrip("v") if latest_stable else "0.1.0")
        maj, min_, _ = parse_semver(ref_stable)
        next_cycle_ver = f"{maj}.{min_ + 1}.0-beta.0"
        next_cycle_tag = f"v{next_cycle_ver}"
        res = {
            "channel": "develop",
            "cycle_base": f"{maj}.{min_ + 1}.0",
            "next_tag": next_cycle_tag,
            "version": next_cycle_ver,
            "has_release": True
        }
        if args.json:
            print(json.dumps(res, indent=2))
        else:
            print(f"NEXT_TAG={next_cycle_tag}")
            print(f"VERSION={next_cycle_ver}")
        return

    # 2. Mode: Develop branch (beta releases)
    if channel == "develop":
        cycle_base, existing_betas = get_active_beta_cycle(latest_stable, tags)
        
        # Check if HEAD is already tagged with a beta tag in this cycle
        beta_at_head = None
        for ht in head_tags:
            if ht.startswith(f"v{cycle_base}-beta."):
                beta_at_head = ht
                break

        if beta_at_head:
            # Idempotent: already tagged commit
            ver = beta_at_head.lstrip("v")
            res = {
                "channel": "develop",
                "version": ver,
                "next_tag": beta_at_head,
                "current_tag": beta_at_head,
                "series": f"{ver.split('.')[0]}.{ver.split('.')[1]}",
                "has_release": False,
                "already_tagged": True,
                "reason": f"HEAD is already tagged with {beta_at_head}"
            }
            if args.json:
                print(json.dumps(res, indent=2))
            else:
                print(f"NEXT_TAG={beta_at_head}")
                print(f"VERSION={ver}")
                print("HAS_RELEASE=false")
            return

        # Check for [skip ci] or [skip release]
        try:
            last_msg = run_git(["log", "-1", "--pretty=%B"])
            if "[skip ci]" in last_msg or "[skip release]" in last_msg:
                res = {
                    "channel": "develop",
                    "has_release": False,
                    "already_tagged": False,
                    "reason": "Commit message requested skip release"
                }
                if args.json:
                    print(json.dumps(res, indent=2))
                else:
                    print("HAS_RELEASE=false")
                return
        except Exception:
            pass

        # Calculate next beta index
        if not existing_betas:
            next_idx = 0
        else:
            next_idx = existing_betas[-1][0] + 1

        next_ver = f"{cycle_base}-beta.{next_idx}"
        next_tag = f"v{next_ver}"

        # Range for changelog
        rev_range = f"{existing_betas[-1][1]}..HEAD" if existing_betas else (f"{latest_stable}..HEAD" if latest_stable else "HEAD~10..HEAD")
        try:
            run_git(["rev-parse", rev_range.split("..")[0]])
        except Exception:
            rev_range = "HEAD"

        changelog, _, _, _ = generate_changelog(rev_range, next_tag)

        res = {
            "channel": "develop",
            "current_tag": existing_betas[-1][1] if existing_betas else (latest_stable or "none"),
            "next_tag": next_tag,
            "version": next_ver,
            "series": f"{cycle_base.split('.')[0]}.{cycle_base.split('.')[1]}",
            "has_release": True,
            "already_tagged": False,
            "changelog": changelog
        }
        if args.json:
            print(json.dumps(res, indent=2))
        else:
            print(f"NEXT_TAG={next_tag}")
            print(f"VERSION={next_ver}")
            print(f"SERIES={res['series']}")
            print("HAS_RELEASE=true")
        return

    # 3. Mode: Main branch (stable releases)
    # Check if a release PR is required
    if args.check_release_pr:
        title = (args.pr_title or "").lower()
        labels = [l.strip().lower() for l in args.pr_labels.split(",") if l.strip()]
        is_rel_pr = "release" in labels or "release" in title or "promote" in title
        if not is_rel_pr:
            res = {
                "channel": "main",
                "has_release": False,
                "already_tagged": False,
                "reason": "Not identified as a release pull request"
            }
            if args.json:
                print(json.dumps(res, indent=2))
            else:
                print("HAS_RELEASE=false")
            return

    # Revision range for changelog and commit inspection
    if latest_stable:
        rev_range = f"{latest_stable}..HEAD"
    else:
        try:
            run_git(["rev-parse", "HEAD~50"])
            rev_range = "HEAD~50..HEAD"
        except Exception:
            rev_range = "HEAD"

    changelog, has_major, has_minor, has_patch = generate_changelog(rev_range, "")

    major, minor, patch = parse_semver(latest_stable)

    # Calculate version
    if args.target_version:
        next_ver = args.target_version.lstrip("v")
    elif args.bump == "MAJOR":
        next_ver = f"{major + 1}.0.0"
    elif args.bump == "MINOR":
        next_ver = f"{major}.{minor + 1}.0"
    elif args.bump == "PATCH":
        next_ver = f"{major}.{minor}.{patch + 1}"
    elif not latest_stable:
        # First stable initialization
        next_ver = "0.1.0"
    elif major == 0:
        if has_major or has_minor:
            next_ver = f"0.{minor + 1}.0"
        else:
            next_ver = f"0.{minor}.{patch + 1}"
    else:
        if has_major:
            next_ver = f"{major + 1}.0.0"
        elif has_minor:
            next_ver = f"{major}.{minor + 1}.0"
        else:
            next_ver = f"{major}.{minor}.{patch + 1}"

    next_tag = f"v{next_ver}"
    # Re-generate changelog with exact tag in header
    changelog = changelog.replace("## Release \n", f"## Release {next_tag}\n")

    # Check tag existence and immutability
    tag_commit = get_tag_commit(next_tag)
    already_tagged = False
    has_release = True
    tag_collision = False

    if tag_commit:
        if head_commit and tag_commit == head_commit:
            already_tagged = True
            has_release = False
        else:
            tag_collision = True
            has_release = False

    res = {
        "channel": "main",
        "current_tag": latest_stable or "none",
        "next_tag": next_tag,
        "version": next_ver,
        "major": next_ver.split(".")[0],
        "minor": f"{next_ver.split('.')[0]}.{next_ver.split('.')[1]}",
        "series": f"{next_ver.split('.')[0]}.{next_ver.split('.')[1]}",
        "has_release": has_release,
        "already_tagged": already_tagged,
        "tag_collision": tag_collision,
        "changelog": changelog
    }

    if tag_collision:
        res["error"] = f"Tag {next_tag} already exists on a different commit ({tag_commit[:7]})."

    if args.json:
        print(json.dumps(res, indent=2))
    else:
        print(f"NEXT_TAG={next_tag}")
        print(f"VERSION={next_ver}")
        print(f"SERIES={res['series']}")
        print(f"HAS_RELEASE={'true' if has_release else 'false'}")

if __name__ == "__main__":
    main()
