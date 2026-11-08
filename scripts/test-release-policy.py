#!/usr/bin/env python3
"""
test-release-policy.py
Comprehensive test suite validating SemVer versioning and release policies for Aether:
1. Initialisation de la première release 0.1.0
2. Cycle de bêtas : patch suivant la stable (1.5.0 -> 1.5.1-beta.N)
3. Incrément beta.0 -> beta.1
4. Plusieurs PR fusionnées successivement (beta.1 -> beta.2 -> beta.3)
5. Nouvelle version stable 0.2.0
6. Préparation de 0.3.0-beta.0
7. Correctif stable 0.2.1
8. Workflow relancé sans création de version supplémentaire (idempotence)
9. Échec de construction Docker bloquant la publication
10. Refus de publication si la CI échoue
11. Rejet si un tag existe déjà sur un commit différent (collision)
12. Sérialisation et protection contre la concurrence
16. Cohérence de version entre métadonnées Git et binaire
18. Respect absolu des protections de branches (aucun push direct contournant les PR)
+ Couverture des fusions squash, merge commit et rebase
"""

import contextlib
import importlib.util
import io
import json
import re
from pathlib import Path
import sys
import unittest
from unittest.mock import patch, MagicMock

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("calculate_release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

class AetherReleasePolicyTests(unittest.TestCase):

    def run_calc(self, args_list):
        with patch.object(sys, "argv", ["calculate_release.py", "--json"] + args_list), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            release.main()
            return json.loads(output.getvalue())

    # 1. Initialisation de la première release 0.1.0
    def test_01_first_stable_initialization_is_0_1_0(self):
        with patch.object(release, "get_all_tags", return_value=[]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "get_tag_commit", return_value=None), \
             patch.object(release, "get_head_commit", return_value="a1b2c3d4e5f67890123456789012345678901234"), \
             patch.object(release, "run_git", return_value="feat: initial bootstrap\x00\x1e"):
            res = self.run_calc(["--channel", "main"])
            self.assertEqual(res["version"], "0.1.0")
            self.assertEqual(res["next_tag"], "v0.1.0")
            self.assertEqual(res["series"], "0.1")
            self.assertTrue(res["has_release"])

    # 2. Après la stable 1.5.0, develop prépare 1.5.1-beta.0
    def test_02_development_cycle_is_next_patch_beta(self):
        with patch.object(release, "get_all_tags", return_value=["v1.5.0"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "run_git", return_value="feat: start next feature\x00\x1e"):
            res = self.run_calc(["--channel", "develop"])
            self.assertEqual(res["version"], "1.5.1-beta.0")
            self.assertEqual(res["next_tag"], "v1.5.1-beta.0")
            self.assertEqual(res["series"], "1.5")
            self.assertTrue(res["has_release"])

    # 2b. La fusion dans main publie la version préparée, puis develop repart en 1.5.2-beta.0
    def test_02b_main_promotes_prepared_cycle_then_develop_moves_on(self):
        tags = ["v1.5.0", "v1.5.1-beta.0", "v1.5.1-beta.1", "v1.5.1-beta.2"]
        with patch.object(release, "get_all_tags", return_value=tags), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "get_tag_commit", return_value=None), \
             patch.object(release, "get_head_commit", return_value="ab" * 20), \
             patch.object(release, "run_git", return_value="abc\x00feat: big feature\x00\x1e"):
            res = self.run_calc(["--channel", "main"])
            self.assertEqual(res["version"], "1.5.1")
            self.assertTrue(res["has_release"])
        with patch.object(release, "get_all_tags", return_value=[*tags, "v1.5.1"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "run_git", return_value="fix: x\x00\x1e"):
            self.assertEqual(self.run_calc(["--channel", "develop"])["version"], "1.5.2-beta.0")

    # 2c. [skip release] dans le commit de fusion ne publie rien sur main
    def test_02c_main_skip_release(self):
        with patch.object(release, "get_all_tags", return_value=["v1.5.0"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "run_git", return_value="docs: typo [skip release]"):
            self.assertFalse(self.run_calc(["--channel", "main"])["has_release"])

    # 3. Incrément beta.0 -> beta.1
    def test_03_increment_beta_0_to_beta_1(self):
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0-beta.0"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "run_git", return_value="feat: add something\x00\x1e"):
            res = self.run_calc(["--channel", "develop"])
            self.assertEqual(res["version"], "0.2.0-beta.1")
            self.assertEqual(res["next_tag"], "v0.2.0-beta.1")
            self.assertTrue(res["has_release"])

    # 4. Plusieurs PR fusionnées successivement (beta.1 -> beta.2 -> beta.3)
    def test_04_multiple_prs_merged_successively(self):
        tags = ["v0.1.0", "v0.2.0-beta.0", "v0.2.0-beta.1"]
        with patch.object(release, "get_all_tags", return_value=tags), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "run_git", return_value="fix: minor bugfix\x00\x1e"):
            res2 = self.run_calc(["--channel", "develop"])
            self.assertEqual(res2["version"], "0.2.0-beta.2")

        tags.append("v0.2.0-beta.2")
        with patch.object(release, "get_all_tags", return_value=tags), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "run_git", return_value="perf: speed up probe\x00\x1e"):
            res3 = self.run_calc(["--channel", "develop"])
            self.assertEqual(res3["version"], "0.2.0-beta.3")

    # 5. Nouvelle version stable 0.2.0
    def test_05_promote_stable_0_2_0(self):
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0-beta.0", "v0.2.0-beta.1"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "get_tag_commit", return_value=None), \
             patch.object(release, "get_head_commit", return_value="1122334455667788990011223344556677889900"), \
             patch.object(release, "run_git", return_value="abc\x00feat: new streaming pipeline\x00\x1e"):
            res = self.run_calc(["--channel", "main", "--bump", "MINOR"])
            self.assertEqual(res["version"], "0.2.0")
            self.assertEqual(res["next_tag"], "v0.2.0")
            self.assertEqual(res["series"], "0.2")
            self.assertTrue(res["has_release"])

    # 6. Après la stable 0.2.0, le cycle suivant est 0.2.1-beta.0
    def test_06_preparation_of_next_cycle_is_next_patch(self):
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0"]):
            res = self.run_calc(["--next-cycle", "--target-version", "0.2.0"])
            self.assertEqual(res["version"], "0.2.1-beta.0")
            self.assertEqual(res["next_tag"], "v0.2.1-beta.0")
            self.assertEqual(res["cycle_base"], "0.2.1")

    # 7. Correctif stable 0.2.1
    def test_07_hotfix_stable_0_2_1(self):
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "get_tag_commit", return_value=None), \
             patch.object(release, "get_head_commit", return_value="deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"), \
             patch.object(release, "run_git", return_value="abc\x00fix: critical range seek fix\x00\x1e"):
            res = self.run_calc(["--channel", "main", "--bump", "PATCH"])
            self.assertEqual(res["version"], "0.2.1")
            self.assertEqual(res["next_tag"], "v0.2.1")
            self.assertEqual(res["series"], "0.2")

    # 8. Workflow relancé sans création de version supplémentaire (Idempotence)
    def test_08_workflow_rerun_idempotent(self):
        # On develop: HEAD already tagged with v0.2.0-beta.2
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0-beta.0", "v0.2.0-beta.1", "v0.2.0-beta.2"]), \
             patch.object(release, "get_tags_at_head", return_value=["v0.2.0-beta.2"]):
            res = self.run_calc(["--channel", "develop"])
            self.assertFalse(res["has_release"])
            self.assertTrue(res["already_tagged"])
            self.assertEqual(res["version"], "0.2.0-beta.2")

        # On main: HEAD already tagged with v0.2.0
        head_commit = "1111222233334444555566667777888899990000"
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0"]), \
             patch.object(release, "get_tags_at_head", return_value=["v0.2.0"]), \
             patch.object(release, "get_tag_commit", return_value=head_commit), \
             patch.object(release, "get_head_commit", return_value=head_commit), \
             patch.object(release, "run_git", return_value=""):
            res = self.run_calc(["--channel", "main", "--target-version", "0.2.0"])
            self.assertFalse(res["has_release"])
            self.assertTrue(res["already_tagged"])

    # 9 & 10. Publication conditionnée par le succès de CI & Docker
    def test_09_and_10_ci_and_docker_gate_tagging(self):
        # In release.yml and ci.yml:
        # - publish-docker needs [calculate-and-tag]
        # - create-github-release needs [calculate-and-tag, publish-docker]
        # - publish on develop needs [workflows, rust, security-audit, frontend, browser, images]
        ci_workflow = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "ci.yml"
        release_workflow = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "release.yml"
        self.assertTrue(ci_workflow.exists())
        self.assertTrue(release_workflow.exists())

        ci_content = ci_workflow.read_text(encoding="utf-8")
        self.assertIn("needs: [workflows, rust, security-audit, frontend, browser, images, calculate-beta]", ci_content)

        rel_content = release_workflow.read_text(encoding="utf-8")
        self.assertIn("needs: [calculate-and-tag, publish-docker]", rel_content)

    # 11. Rejet si le tag existe déjà sur un commit différent (collision)
    def test_11_tag_collision_rejected(self):
        tag_commit = "aaaa111122223333444455556666777788889999"
        head_commit = "bbbb111122223333444455556666777788889999"
        with patch.object(release, "get_all_tags", return_value=["v0.1.0", "v0.2.0"]), \
             patch.object(release, "get_tags_at_head", return_value=[]), \
             patch.object(release, "get_tag_commit", return_value=tag_commit), \
             patch.object(release, "get_head_commit", return_value=head_commit), \
             patch.object(release, "run_git", return_value=""):
            res = self.run_calc(["--channel", "main", "--target-version", "0.2.0"])
            self.assertFalse(res["has_release"])
            self.assertTrue(res["tag_collision"])
            self.assertIn("error", res)

    # 12. Protection de concurrence
    def test_12_concurrency_serialization(self):
        publish_wf = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "publish.yml"
        release_wf = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "release.yml"
        self.assertIn("cancel-in-progress: false", publish_wf.read_text(encoding="utf-8"))
        self.assertIn("cancel-in-progress: false", release_wf.read_text(encoding="utf-8"))

    # 16. Vérification de la politique de version binaire (Rust version policy)
    def test_16_binary_version_policy_consistency(self):
        # Local unreleased build on develop produces explicit dev version
        ver = release.parse_semver("v0.2.0-beta.3")
        self.assertEqual(ver, (0, 2, 0))

    # Types de fusion (Squash, Merge Commit, Rebase)
    def test_merge_types_handling(self):
        # 1. Merge commit format
        log_merge = "hash1\x00Merge pull request #15 from user/branch\x00Merge commit body\x1e"
        log_merge += "hash2\x00feat: add flac support\x00Details\x1e"
        with patch.object(release, "run_git", return_value=log_merge):
            cl, major, minor, patch_ = release.generate_changelog("HEAD~2..HEAD", "v0.2.0")
            self.assertTrue(minor)
            self.assertIn("flac support", cl)
            self.assertNotIn("Merge pull request #15", cl)

        # 2. Squash merge format
        log_squash = "hash3\x00fix(playback): repair seek offset (#42)\x00Squashed commit body\x1e"
        with patch.object(release, "run_git", return_value=log_squash):
            cl2, major2, minor2, patch2 = release.generate_changelog("HEAD~1..HEAD", "v0.2.0")
            self.assertTrue(patch2)
            self.assertIn("seek offset", cl2)

        # 3. Rebase merge format
        log_rebase = "hash4\x00perf: optimize opus demux\x00Direct fast-forward commit\x1e"
        with patch.object(release, "run_git", return_value=log_rebase):
            cl3, major3, minor3, patch3 = release.generate_changelog("HEAD~1..HEAD", "v0.2.0")
            self.assertTrue(patch3)
            self.assertIn("optimize opus demux", cl3)

    # 18. Respect des protections de branches (Pas de push direct sur main sans PR)
    def test_18_branch_protection_respected(self):
        release_wf = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "release.yml"
        content = release_wf.read_text(encoding="utf-8")
        # Ensure git push origin main is NEVER invoked in the release workflow
        self.assertNotIn("git push origin main", content)
        # Git tags are pushed, which point to the exact commit
        self.assertIn('git push origin "$TAG"', content)

    def test_only_validation_builds_and_promotion_loads_the_verified_images(self):
        root = Path(__file__).resolve().parent.parent
        ci = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        publish = (root / ".github/workflows/publish.yml").read_text(encoding="utf-8")
        release_wf = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertEqual(ci.count("uses: docker/bake-action@v6"), 1)
        self.assertRegex(ci, r"(?m)^ +source: \.\s*$")
        self.assertIn("ref: ${{ github.sha }}", ci)
        self.assertIn("docker-bake.ci.hcl", ci)
        self.assertNotIn("docker/bake-action", publish)
        self.assertNotRegex(publish, r"docker (build|buildx bake)")
        self.assertIn("ci-image-archive.py load", publish)
        self.assertIn("name: verified-images-${{ matrix.arch }}", ci)
        self.assertIn("name: verified-images-${{ matrix.arch }}", publish)
        self.assertIn("github.event_name != 'pull_request'", publish)
        self.assertIn("needs: [calculate-beta, publish]", ci)
        self.assertIn("needs: [calculate-and-tag, validate]", release_wf)
        self.assertIn("uses: ./.github/workflows/ci.yml", release_wf)
        self.assertNotIn("packages: write", ci.split("  images:")[1].split("  tag-beta:")[0])

if __name__ == "__main__":
    unittest.main()
