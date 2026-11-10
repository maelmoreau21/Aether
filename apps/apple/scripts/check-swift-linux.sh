#!/bin/sh
# What can be checked of the Swift side without a Mac, in the swift image,
# after check-rust-linux.sh (same target volume):
# - every app source parses (SwiftUI, UIKit and AVKit are absent on Linux, so
#   the apps themselves are only type-checked by the macOS workflow);
# - the UniFFI bindings compile and call the Rust core (smoke-test.swift).
#
#   docker run --rm -v "$PWD:/work" -w /work -v aether-target-apple:/work/target \
#     swift:6.2-bookworm sh apps/apple/scripts/check-swift-linux.sh
set -eu

for source in apps/apple/Sources/Shared/*.swift; do
    swiftc -parse "$source"
done
echo "Parsed: $(ls apps/apple/Sources/Shared/*.swift | wc -l) sources"

out=target/apple-linux
mkdir -p "$out/module" "$out/smoke"
cp "$out/aether_ffiFFI.h" "$out/module/"
cp "$out/aether_ffiFFI.modulemap" "$out/module/module.modulemap"
cp apps/apple/scripts/smoke-test.swift "$out/smoke/main.swift"
swiftc -I "$out/module" -L target/debug -laether_ffi \
    "$out/aether_ffi.swift" "$out/smoke/main.swift" -o "$out/smoke/smoke"
LD_LIBRARY_PATH=target/debug "$out/smoke/smoke"
