"""Tests for the active Markdown and Halo-reference validator."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_doc_references import check_file, target_path


class DocumentationReferenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "docs").mkdir()

    def write(self, name: str, content: str) -> None:
        file = self.root / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content, encoding="utf-8")

    def test_local_links_and_parent_references(self) -> None:
        self.write("AGENTS.md", "# Agent rules")
        self.write("docs/README.md", "[rules](../AGENTS.md) [in page](#section)")
        self.assertEqual(check_file(self.root, "docs/README.md"), [])

    def test_broken_local_link_is_reported(self) -> None:
        self.write("docs/README.md", "[broken](MISSING.md)")
        errors = check_file(self.root, "docs/README.md")
        self.assertEqual(len(errors), 1)
        self.assertIn("MISSING.md", errors[0])

    def test_external_links_and_fenced_examples_are_ignored(self) -> None:
        self.write(
            "docs/README.md",
            "[source](https://example.org/x) [mail](mailto:a@example.org)\n"
            "\x60\x60\x60md\n[example](not-in-repo.md)\n\x60\x60\x60\n",
        )
        self.assertEqual(check_file(self.root, "docs/README.md"), [])

    def test_encoded_spaces_and_fragments(self) -> None:
        self.write("docs/a b.md", "# A")
        self.write("docs/README.md", "[space](a%20b.md#top)")
        self.assertEqual(check_file(self.root, "docs/README.md"), [])

    def test_outside_repository_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            target_path(self.root, self.root / "docs/README.md", "../../elsewhere.md")

    def test_halo_plain_text_paths_are_checked(self) -> None:
        self.write("docs/UI_DIRECTION.md", "# Design")
        self.write("apps/web/src/lib/styles/halo.css", ":root {}")
        self.write(
            ".claude/skills/aether-halo/SKILL.md",
            "1. docs/UI_DIRECTION.md\n2. apps/web/src/lib/styles/halo.css\n"
            "Optional: .agents/skills/aether-halo/SKILL.md\n",
        )
        self.assertEqual(check_file(self.root, ".claude/skills/aether-halo/SKILL.md"), [])
        (self.root / "apps/web/src/lib/styles/halo.css").unlink()
        self.assertTrue(any("halo.css" in e for e in check_file(self.root, ".claude/skills/aether-halo/SKILL.md")))

    def test_missing_document_is_reported(self) -> None:
        self.assertTrue(check_file(self.root, "docs/missing.md"))


if __name__ == "__main__":
    unittest.main()
