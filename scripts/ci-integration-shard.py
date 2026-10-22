#!/usr/bin/env python3
"""Partition Cargo integration targets without dropping tests or sharing a runner."""
import argparse
import json
import subprocess


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
    return sorted(set(targets))[index::count]


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
                      "total_targets": len(targets), "targets": selected}), flush=True)
    if args.list or not selected:
        return
    command = ["cargo", "test", "--locked", "--workspace", "--all-features"]
    for target in selected:
        command.extend(["--test", target])
    command.extend(["--", "--test-threads=1"])
    subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
