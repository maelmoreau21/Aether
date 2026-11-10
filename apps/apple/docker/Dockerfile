# Linux image that compiles the Rust core for the Apple targets (static
# libraries only: linking the apps needs Xcode, see the macOS workflow).
FROM rust:slim-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends clang llvm pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-tvos aarch64-apple-tvos-sim
