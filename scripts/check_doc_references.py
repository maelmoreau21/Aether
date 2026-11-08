#!/usr/bin/env python3
"""Check local links in Aether's active agent instructions and documentation index.

Historical archives are intentionally excluded: they can preserve superseded links.
This script is read-only and has no third-party dependencies.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]
ACTIVE_DOCS = (
    "README.md",
    "AGENTS.md",
    "CLAUDE.md",
    "docs/README.md",
    "docs/AI_WORKFLOW.md",
    ".ai/PROMPT_DEMARRAGE_IA.md",
    ".claude/skills/aether-halo/SKILL.md",
    ".cursor/rules/aether-halo.mdc",
)
# Markdown links (including image links); an optional <...> supports encoded spaces.
LINK = re.compile(r"\]\((?:<([^>]+)>|([^\s)]+))")
# Halo instructions also reference plain-text paths rather than Markdown links.
HALO_PATH = re.compile(r"(?<![\w.-])(?:docs|apps|\.claude|\.cursor|\.ai)/[\w./-]+")
HALO_DOCS = (
    ".claude/skills/aether-halo/SKILL.md",
    ".cursor/rules/aether-halo.mdc",
)
FENCE = re.compile(r"(?ms)^\s*(" + chr(96) * 3 + r"|~~~).*?^\s*\1[^\n]*(?:\n|$)")


def target_path(root: Path, source: Path, target: str) -> Path | None:
    """Resolve local targets; external links and intra-page fragments need no file."""
    target = target.strip()
    if not target or target.startswith(("#", "//")):
        return None
    if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I):
        return None
    target = unquote(target.split("#", 1)[0].split("?", 1)[0])
    if not target:
        return None
    candidate = (source.parent / target).resolve()
    if not candidate.is_relative_to(root.resolve()):
        raise ValueError(f"reference escapes repository: {target}")
    return candidate


def check_file(root: Path, relative: str) -> list[str]:
    source = root / relative
    if not source.is_file():
        return [f"{relative}: required document missing"]
    content = FENCE.sub("", source.read_text(encoding="utf-8"))
    errors = []
    for match in LINK.finditer(content):
        ref = (match.group(1) or match.group(2)).strip()
        try:
            resolved = target_path(root, source, ref)
        except ValueError as exc:
            errors.append(f"{relative}: {exc}")
            continue
        if resolved is not None and not resolved.exists():
            errors.append(f"{relative}: missing local link {ref}")
    if relative in HALO_DOCS:
        # .agents is an optional, Git-ignored local folder, not a required input.
        for match in HALO_PATH.finditer(content):
            ref = match.group().rstrip(".,:;")
            if ref.startswith(".agents/"):
                continue
            resolved = root / ref
            if not resolved.exists():
                errors.append(f"{relative}: missing Halo reference {ref}")
    return sorted(set(errors))


def main() -> int:
    problems = [error for file in ACTIVE_DOCS for error in check_file(ROOT, file)]
    if problems:
        print("Broken active documentation references:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1
    print(f"OK: {len(ACTIVE_DOCS)} active documentation files checked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
