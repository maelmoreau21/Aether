import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "sharding", Path(__file__).with_name("ci-integration-shard.py")
)
sharding = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sharding)


class CoverageTests(unittest.TestCase):
    def test_all_targets_are_assigned_exactly_once(self):
        targets = [f"target-{i}" for i in range(41)]
        shards = [sharding.shard_targets(targets, i, 3) for i in range(3)]
        self.assertEqual(sorted(sum(shards, [])), sorted(targets))
        self.assertLessEqual(max(map(len, shards)) - min(map(len, shards)), 1)
        self.assertEqual(shards[1], sharding.shard_targets(reversed(targets), 1, 3))

    def test_only_workspace_integration_targets_are_selected(self):
        def package(identifier, targets):
            return {"id": identifier, "targets": [
                {"name": name, "kind": [kind]} for name, kind in targets
            ]}
        metadata = {"workspace_members": ["core", "node"], "packages": [
            package("core", [("http", "test"), ("core", "lib"), ("cli", "bin"), ("speed", "bench")]),
            package("node", [("http", "test"), ("ranges", "test")]),
            package("dependency", [("external", "test")]),
        ]}
        self.assertEqual(sharding.integration_targets(metadata), ["http", "ranges"])

    def test_long_targets_are_distributed_without_dropping_new_targets(self):
        targets = ["scanner_performance", "global_catalogue_consolidation", "admin_users", "new_target"]
        shards = [sharding.shard_targets(targets, i, 3) for i in range(3)]
        self.assertEqual(sorted(sum(shards, [])), sorted(targets))
        self.assertNotEqual(next(i for i, shard in enumerate(shards) if "scanner_performance" in shard),
                            next(i for i, shard in enumerate(shards) if "global_catalogue_consolidation" in shard))
        self.assertEqual(shards, [sharding.shard_targets(reversed(targets), i, 3) for i in range(3)])

    def test_invalid_partition_cannot_silently_skip_tests(self):
        for index, count in [(-1, 3), (3, 3), (0, 0)]:
            with self.assertRaises(ValueError):
                sharding.shard_targets(["http"], index, count)


if __name__ == "__main__":
    unittest.main()
