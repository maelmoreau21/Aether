#!/usr/bin/env python3
"""
test-backup-key-absence.py
Vérifie que l'ancienne clé de sauvegarde compromise (crates/core/secrets/backup.key)
n'est plus présente dans le working tree, l'index Git ni l'historique récent,
et qu'elle reste exclue par .gitignore et .dockerignore.
"""

from pathlib import Path
import subprocess
import sys
import unittest

sys.dont_write_bytecode = True

REPO_ROOT = Path(__file__).resolve().parent.parent


def run_git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


class BackupKeyAbsenceTests(unittest.TestCase):
    def test_compromised_key_file_absent_from_worktree(self) -> None:
        key_path = REPO_ROOT / "crates" / "core" / "secrets" / "backup.key"
        self.assertFalse(
            key_path.exists(),
            "crates/core/secrets/backup.key ne doit jamais être présent dans le dépôt",
        )

    def test_compromised_key_not_tracked_in_git_index(self) -> None:
        tracked = run_git("ls-files", "**/secrets/backup.key", "crates/core/secrets/backup.key")
        self.assertEqual(
            tracked,
            "",
            f"Fichier de clé suivi par Git : {tracked}",
        )

    def test_compromised_key_absent_from_recent_history(self) -> None:
        has_50 = subprocess.run(
            ["git", "rev-parse", "--verify", "HEAD~50"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
        ).returncode == 0
        rev_range = "HEAD~50..HEAD" if has_50 else "HEAD"
        recent = run_git(
            "log",
            rev_range,
            "--format=%H",
            "--",
            "crates/core/secrets/backup.key",
        )
        self.assertEqual(
            recent,
            "",
            f"La clé compromise apparaît dans l'historique récent ({rev_range}) : {recent}",
        )

    def test_ignore_files_exclude_backup_key(self) -> None:
        for ignore_name in (".gitignore", ".dockerignore"):
            ignore_path = REPO_ROOT / ignore_name
            content = ignore_path.read_text(encoding="utf-8")
            self.assertIn(
                "secrets/backup.key",
                content,
                f"{ignore_name} doit exclure **/secrets/backup.key",
            )


if __name__ == "__main__":
    unittest.main()
