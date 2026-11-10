#!/bin/sh
# Builds the shared Rust core for Android, generates its Kotlin bindings and
# assembles the debug APK. Run from the repository root inside the
# `aether-android` image (see apps/android/docker/Dockerfile):
#   docker run --rm -v "$PWD:/work" -w /work aether-android sh apps/android/scripts/build.sh
set -eu

APP=apps/android/app
for target in aarch64-linux-android:arm64-v8a x86_64-linux-android:x86_64; do
    triple=${target%%:*}
    abi=${target##*:}
    cargo build --locked --release --target "$triple" -p aether-client-ffi --lib
    mkdir -p "$APP/src/main/jniLibs/$abi"
    cp "target/$triple/release/libaether_ffi.so" "$APP/src/main/jniLibs/$abi/"
done

# Bindings come from the compiled library itself (UniFFI library mode). The
# release profile strips symbols, UniFFI metadata included, so read them from
# an unstripped host build; the shipped Android libraries stay stripped.
cargo build --locked -p aether-client-ffi --lib
rm -rf "$APP/build/generated/uniffi"
cargo run --locked -p aether-client-ffi --bin uniffi-bindgen -- generate \
    --library target/debug/libaether_ffi.so \
    --language kotlin --out-dir "$APP/build/generated/uniffi" --no-format
test -f "$APP/build/generated/uniffi/uniffi/aether_ffi/aether_ffi.kt"

if [ "${RELEASE:-0}" = 1 ]; then
    # Minified APK (sideload, F-Droid) and AAB (Play Store); signed when
    # keystore.properties or the AETHER_KEY* variables are present.
    gradle --no-daemon -p apps/android assembleRelease bundleRelease
    ls "$APP"/build/outputs/apk/release/*.apk "$APP"/build/outputs/bundle/release/*.aab
else
    gradle --no-daemon -p apps/android assembleDebug
    echo "APK: $APP/build/outputs/apk/debug/app-debug.apk"
fi
