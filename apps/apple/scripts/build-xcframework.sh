#!/bin/sh
# Builds the shared Rust core for iOS, iPadOS and tvOS (devices and Apple
# silicon simulators) into Generated/AetherFFI.xcframework, and its Swift
# bindings into Generated/aether_ffi.swift. Run on macOS with Xcode, from the
# repository root:
#   sh apps/apple/scripts/build-xcframework.sh
set -eu

OUT=apps/apple/Generated
WORK=target/apple
export IPHONEOS_DEPLOYMENT_TARGET=17.0 TVOS_DEPLOYMENT_TARGET=17.0

for triple in aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-tvos aarch64-apple-tvos-sim; do
    # A static library: the app links it, no dylib to embed or sign.
    cargo rustc --locked --release --target "$triple" -p aether-client-ffi --lib --crate-type staticlib
done

# Bindings come from the compiled library itself (UniFFI library mode). The
# release profile strips symbols, UniFFI metadata included, so read them from
# an unstripped host build.
cargo build --locked -p aether-client-ffi --lib
rm -rf "$WORK" "$OUT/AetherFFI.xcframework" "$OUT/aether_ffi.swift"
mkdir -p "$WORK/bindings" "$WORK/headers" "$OUT"
cargo run --locked -p aether-client-ffi --bin uniffi-bindgen -- generate \
    --library target/debug/libaether_ffi.dylib \
    --language swift --out-dir "$WORK/bindings"
cp "$WORK/bindings/aether_ffiFFI.h" "$WORK/headers/"
# Xcode finds the C module by this exact name inside the XCFramework.
cp "$WORK/bindings/aether_ffiFFI.modulemap" "$WORK/headers/module.modulemap"
cp "$WORK/bindings/aether_ffi.swift" "$OUT/"

xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/libaether_ffi.a -headers "$WORK/headers" \
    -library target/aarch64-apple-ios-sim/release/libaether_ffi.a -headers "$WORK/headers" \
    -library target/aarch64-apple-tvos/release/libaether_ffi.a -headers "$WORK/headers" \
    -library target/aarch64-apple-tvos-sim/release/libaether_ffi.a -headers "$WORK/headers" \
    -output "$OUT/AetherFFI.xcframework"
echo "XCFramework: $OUT/AetherFFI.xcframework"
