#!/usr/bin/env python3
"""Partition Cargo integration targets without dropping tests or sharing a runner."""
import argparse
import json
import subprocess

# Conservative max libtest seconds from runs 41903062442 and 41911338866.
# Weights affect scheduling only: discovery remains authoritative and unknown
# targets default to one second. Refresh after a representative CI run.
TARGET_SECONDS = {
    "account": 49,
    "admin_access": 40,
    "admin_backups": 6,
    "admin_postgres": 1,
    "admin_users": 48,
    "adversarial_challenge": 12,
    "applications": 11,
    "artwork_variants": 19,
    "auth_under_sqlite_lock": 43,
    "automation_flow": 37,
    "backup_bundle": 3,
    "backup_restore": 35,
    "browser_admin": 1,
    "browser_auth": 1,
    "browser_nfo": 1,
    "browser_posters": 1,
    "browser_scanner": 1,
    "browser_series": 1,
    "browser_statistics": 1,
    "catalog": 1,
    "catalog_performance": 11,
    "catalog_responsiveness": 55,
    "challenger_security_challenge": 32,
    "chapter_images_remote": 9,
    "concurrency_writes_during_publication": 12,
    "contracts_test": 1,
    "core_restart": 14,
    "distributed_multi_node": 18,
    "distributed_tasks": 2,
    "distributed_tasks_scale": 1,
    "extension_data_bench": 1,
    "extension_storage_lifecycle": 3,
    "extensions": 153,
    "full_backup_restore_e2e": 4,
    "global_catalogue_consolidation": 98,
    "healthcheck_resilience": 6,
    "identity": 41,
    "internal_transport": 16,
    "multi_core_extension_runs": 1,
    "multi_node_scale_and_failover": 25,
    "nfo_flow": 18,
    "node_enrollment_and_relay": 10,
    "p0": 2,
    "permissions": 16,
    "plugin_storage_integration": 9,
    "posters_flow": 9,
    "postgres": 28,
    "preferences": 6,
    "preparation_admission": 1,
    "scan_diagnostics": 8,
    "scanner_flow": 27,
    "scanner_performance": 227,
    "scheduler": 26,
    "series_flow": 32,
    "series_performance": 20,
    "series_responsiveness": 61,
    "settings_avatars": 26,
    "static_web": 11,
    "syncplay_websocket": 13,
    "task_engine_http": 8,
    "task_operations": 4,
    "version_and_nodes": 6,
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
    command = ["cargo", "test", "--locked", "--workspace", "--all-features", "--timings"]
    for target in selected:
        command.extend(["--test", target])
    command.extend(["--", "--test-threads=1"])
    subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
