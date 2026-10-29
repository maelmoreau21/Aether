#!/usr/bin/env python3
"""Partition Cargo integration targets without dropping tests or sharing a runner."""
import argparse
import json
import subprocess

# Approximate seconds measured on GitHub run 40945793905 (ubuntu-latest).
# Weights affect scheduling only: discovery remains authoritative and unknown
# targets default to one second. Refresh after a representative CI run.
TARGET_SECONDS = {
    "account": 38, "artwork_variants": 14, "catalog_performance": 7,
    "concurrency_writes_during_publication": 8, "distributed_multi_node": 16,
    "identity": 35, "nfo_flow": 13, "permissions": 12, "postgres": 21,
    "scan_diagnostics": 6, "scheduler": 16, "series_responsiveness": 47,
    "task_engine_http": 6, "internal_transport": 15, "admin_access": 39,
    "admin_users": 46, "auth_under_sqlite_lock": 44, "backup_restore": 35,
    "catalog_responsiveness": 49, "extension_storage_lifecycle": 2, "extensions": 51,
    "global_catalogue_consolidation": 104, "node_enrollment_and_relay": 10,
    "plugin_storage_integration": 8, "preferences": 5, "scanner_flow": 27,
    "series_flow": 30, "settings_avatars": 18, "task_operations": 3,
    "admin_backups": 4, "adversarial_challenge": 9, "automation_flow": 21,
    "challenger_security_challenge": 23, "core_restart": 10,
    "healthcheck_resilience": 4, "multi_node_scale_and_failover": 20,
    "posters_flow": 6, "scanner_performance": 155, "series_performance": 15,
    "static_web": 8, "version_and_nodes": 4,
}


def integration_targets(metadata):
    members = set(metadata["workspace_members"])
    # Identically named targets in different packages stay on the same runner;
    # Cargo's workspace selection executes all matching integration targets.
    return sorted({
        target["name"]
        for package in metadata["packages"] if package["id"] in members
        for target in package["targets"] if "test" in target["kind"]
    })


def shard_targets(targets, index, count):
    if count < 1 or not 0 <= index < count:
        raise ValueError("require 0 <= index < count")
    shards = [[] for _ in range(count)]
    totals = [0] * count
    # Largest jobs first; names and shard number break ties deterministically.
    for target in sorted(set(targets), key=lambda name: (-TARGET_SECONDS.get(name, 1), name)):
        shard = min(range(count), key=lambda i: (totals[i], len(shards[i]), i))
        shards[shard].append(target)
        totals[shard] += TARGET_SECONDS.get(target, 1)
    return sorted(shards[index])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", type=int, required=True)
    parser.add_argument("--count", type=int, required=True)
    parser.add_argument("--list", action="store_true", help="show plan without building/running tests")
    args = parser.parse_args()
    if args.count < 1 or not 0 <= args.index < args.count:
        parser.error("require 0 <= index < count")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"], text=True
    ))
    targets = integration_targets(metadata)
    if not targets:
        raise SystemExit("No integration targets discovered; refusing an empty test suite")
    selected = shard_targets(targets, args.index, args.count)
    print(json.dumps({"shard": args.index, "count": args.count,
                      "total_targets": len(targets), "targets": selected,
                      "estimated_test_seconds": sum(TARGET_SECONDS.get(target, 1) for target in selected),
                      "estimate_scope": "historical scheduling weights, not a time guarantee"}), flush=True)
    if args.list or not selected:
        return
    command = ["cargo", "test", "--locked", "--workspace", "--all-features"]
    for target in selected:
        command.extend(["--test", target])
    command.extend(["--", "--test-threads=1"])
    subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
