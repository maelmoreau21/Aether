#!/usr/bin/env python3
"""
calculate_release.py
Analyse l'historique Git depuis le dernier tag (ou HEAD~100) pour déterminer
la prochaine version SemVer selon Conventional Commits et générer le changelog.

Règles :
- 'fix:', 'perf:' -> PATCH
- 'feat:' -> MINOR
- 'feat!:', 'fix!:', ou message contenant 'BREAKING CHANGE:' -> MAJOR (ou MINOR en 0.x tant que < 1.0.0)
- Si aucun commit conventionnel identifié -> incrément minimal PATCH.
- Préfixe de tag : 'v' (ex: v0.1.1).
"""

import sys
import subprocess
import re
import json

def run_git(args):
    res = subprocess.run(["git"] + args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
    return res.stdout.strip()

def get_latest_tag():
    try:
        # Obtenir le tag le plus récent accessible depuis HEAD
        tag = run_git(["describe", "--tags", "--abbrev=0"])
        return tag
    except Exception:
        return None

def parse_semver(tag_str):
    if not tag_str:
        return (0, 1, 0)
    cleaned = tag_str.lstrip("v")
    parts = cleaned.split(".")
    try:
        major = int(parts[0])
        minor = int(parts[1]) if len(parts) > 1 else 0
        patch = int(parts[2]) if len(parts) > 2 else 0
        return (major, minor, patch)
    except ValueError:
        return (0, 1, 0)

def main():
    latest_tag = get_latest_tag()
    major, minor, patch = parse_semver(latest_tag)

    if latest_tag:
        rev_range = f"{latest_tag}..HEAD"
    else:
        # Si aucun tag initial, on analyse les commits du merge ou les 50 derniers
        rev_range = "HEAD~50..HEAD"
        # S'assurer que le range existe
        try:
            run_git(["rev-parse", "HEAD~50"])
        except Exception:
            rev_range = "HEAD"

    try:
        log_output = run_git(["log", rev_range, "--pretty=format:%H%x00%s%x00%b%x1e"])
    except Exception:
        log_output = ""

    entries = log_output.strip().split("\x1e") if log_output.strip() else []

    has_major = False
    has_minor = False
    has_patch = False

    features = []
    fixes = []
    others = []

    for entry in entries:
        parts = entry.strip().split("\x00")
        if len(parts) < 2:
            continue
        commit_hash = parts[0].strip()
        subject = parts[1].strip()
        body = parts[2].strip() if len(parts) > 2 else ""

        # Ignorer les messages de merge pull request purs
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

    # Incrémentation SemVer
    if major == 0:
        # En phase 0.x :
        # breaking change ou feature -> minor (0.2.0)
        # fix ou aucun changement conventionnel -> patch (0.1.1)
        if has_major or has_minor:
            next_version = f"0.{minor + 1}.0"
        else:
            next_version = f"0.{minor}.{patch + 1}"
    else:
        if has_major:
            next_version = f"{major + 1}.0.0"
        elif has_minor:
            next_version = f"{major}.{minor + 1}.0"
        else:
            next_version = f"{major}.{minor}.{patch + 1}"

    next_tag = f"v{next_version}"

    # Construction du changelog markdown
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

    changelog_str = "\n".join(changelog_parts).strip()

    result = {
        "current_tag": latest_tag or "none",
        "next_tag": next_tag,
        "version": next_version,
        "major": next_version.split(".")[0],
        "minor": f"{next_version.split('.')[0]}.{next_version.split('.')[1]}",
        "changelog": changelog_str
    }

    if len(sys.argv) > 1 and sys.argv[1] == "--json":
        print(json.dumps(result, indent=2))
    else:
        print(f"NEXT_TAG={next_tag}")
        print(f"VERSION={next_version}")
        print(f"SERIES={result['minor']}")

if __name__ == "__main__":
    main()
