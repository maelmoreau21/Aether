#!/bin/sh
# Compiles the shared Rust core as static libraries for iOS and tvOS from Linux
# (Docker), without Xcode: a static library is only archived, never linked, and
# ring's few C/assembly files build with clang against a stand-in SDK root that
# holds the one SDK header they read (TargetConditionals.h); ring builds
# without the C library headers when RING_CORE_NOSTDLIBINC is defined.
# The real build (with the SDKs) is scripts/build-xcframework.sh on macOS.
#
#   docker build -t aether-apple-check apps/apple/docker
#   docker run --rm -v "$PWD:/work" -w /work \
#     -v aether-cargo:/usr/local/cargo/registry -v aether-target-apple:/work/target \
#     aether-apple-check sh apps/apple/scripts/check-rust-linux.sh
set -eu

export IPHONEOS_DEPLOYMENT_TARGET=17.0 TVOS_DEPLOYMENT_TARGET=17.0
for triple in aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-tvos aarch64-apple-tvos-sim; do
    case $triple in *tvos*) ios=0 tv=1 ;; *) ios=1 tv=0 ;; esac
    case $triple in *-sim) sim=1 ;; *) sim=0 ;; esac
    sdk=$(mktemp -d)
    mkdir -p "$sdk/usr/include"
    cat > "$sdk/usr/include/TargetConditionals.h" <<EOF
#define TARGET_OS_MAC 1
#define TARGET_OS_OSX 0
#define TARGET_OS_IPHONE 1
#define TARGET_OS_IOS $ios
#define TARGET_OS_TV $tv
#define TARGET_OS_WATCH 0
#define TARGET_OS_VISION 0
#define TARGET_OS_MACCATALYST 0
#define TARGET_OS_SIMULATOR $sim
#define TARGET_OS_EMBEDDED $((1 - sim))
EOF
    var=$(echo "$triple" | tr - _)
    export "CC_$var=clang" "AR_$var=llvm-ar" "CFLAGS_$var=-isysroot $sdk -DRING_CORE_NOSTDLIBINC=1"
    SDKROOT=$sdk cargo rustc --locked --release --target "$triple" \
        -p aether-client-ffi --lib --crate-type staticlib
    ls -l "target/$triple/release/libaether_ffi.a"
done

# A Linux build of the same core and its Swift bindings, for
# scripts/check-swift-linux.sh (run next, in the swift image).
cargo build --locked -p aether-client-ffi --lib
cargo run --locked -p aether-client-ffi --bin uniffi-bindgen -- generate \
    --library target/debug/libaether_ffi.so --language swift --out-dir target/apple-linux
