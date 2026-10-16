# Technical Analysis: Rust Cargo Build Toolchain & Multi-Stage Dockerfile Stage

**Agent**: Milestone 2 Explorer 1 (`explorer_m2_1`)  
**Target Milestone**: Milestone 2 (Container Packaging: Dockerfile & .dockerignore)  
**Date**: 2026-10-16  

---

## Executive Summary

This investigation analyzed the Rust workspace toolchain requirements, Docker builder base image options, build caching patterns, native C dependencies, dynamic linking, and binary size optimizations for building `media-poc` and `media-bootstrap` for x86_64 Linux.

### Key Findings:
1. **Toolchain Requirement**: Workspace root `Cargo.toml` mandates `rust-version = "1.99"` and `edition = "2024"`. Pinning to `rust:1.85-slim-bookworm` is **incompatible** and causes an immediate Cargo MSRV failure. The official `rust:slim-bookworm` image provides `rustc 1.99.1` and `cargo 1.99.1`, matching the host environment.
2. **Native & System Dependencies**: The workspace uses pure Rust crates plus `libsqlite3-sys` with the `sqlite-bundled` feature. It does **not** depend on OpenSSL, pkg-config, or system SQLite. The official `rust:slim-bookworm` base image includes `gcc`, `cc`, and `strip` out of the box, requiring **zero** additional `apt-get` packages in the builder stage.
3. **Workspace Invariants & Build Traps**:
   - `crates/core/src/lib.rs` requires `#[path = "../../shared/src/brand.rs"]`. `crates/shared` must be present in the build tree.
   - `crates/core/src/identity.rs` and `inventory.rs` compile embedded SQLite migrations (`sqlx::migrate!("./migrations")` and `include_str!`). The `crates/core/migrations/` directory must be present when compiling application code.
   - `crates/core/Cargo.toml` explicitly declares `[[test]] name = "p0"` and `[[test]] name = "catalog"`. If dummy manifest caching is used, dummy test files `crates/core/tests/p0.rs` and `crates/core/tests/catalog.rs` **must** exist or Cargo's manifest parser aborts before downloading dependencies.
4. **Build Caching Performance**: Using a scaffolded dummy targets pattern with BuildKit cache mounts reduces rebuild time from **1m 45s** down to **20.7s** when application code changes, with zero dependency recompilations.
5. **Binary Stripping**: Running `strip --strip-all` reduces the release binary sizes from 16.1 MB combined (11 MB `media-poc` + 5.1 MB `media-bootstrap`) down to 11.5 MB (7.6 MB `media-poc` + 3.9 MB `media-bootstrap`), a **29% reduction** (4.6 MB saved) with zero loss of functionality.
6. **ABI & Runtime Compatibility**: `ldd` verification demonstrates that `media-poc` and `media-bootstrap` dynamically link exclusively to standard `glibc` (`libc.so.6`, `libm.so.6`, `libgcc_s.so.1`), matching `Debian GLIBC 2.36` in `debian:bookworm-slim`.

---

## 1. Workspace Manifest, Edition & Compiler Requirements

### 1.1 Manifest Inspection
- **Root `Cargo.toml`**:
  ```toml
  [workspace]
  members = ["crates/core"]
  resolver = "3"

  [workspace.package]
  edition = "2024"
  rust-version = "1.99"
  ```
- **Crate `crates/core/Cargo.toml`**:
  - `name = "media-poc"`
  - `version = "0.1.0"`
  - `edition.workspace = true`
  - `rust-version.workspace = true`
  - Binaries produced:
    1. `media-poc` (`src/main.rs`, default run target)
    2. `media-bootstrap` (`src/bootstrap.rs`, via `[[bin]]`)
  - Test targets declared:
    - `[[test]] name = "p0"`
    - `[[test]] name = "catalog"`
  - Build script: `crates/core/build.rs` (`println!("cargo:rerun-if-changed=migrations");`)

### 1.2 Compiler Version Analysis
- Host environment: `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `cargo 1.99.1 (5f94df478 2026-08-27)`.
- In commit `b198691fb6338f60264c5a63269a3218b9d1f4a1`, `rust-version` was updated from `1.85` to `1.99`.
- **MSRV Consequence**: Cargo strictly enforces `rust-version`. Attempting to build with `rust:1.85-slim-bookworm` results in:
  ```
  error: package `media-poc v0.1.0` cannot be built because it requires rustc 1.99 or newer, while the currently active rustc version is 1.85.0
  ```
- Therefore, the builder image must be at least Rust 1.99. The official `rust:slim-bookworm` (or `rust:1-slim-bookworm` / `rust:1.99-slim-bookworm`) provides `rustc 1.99.1`.

### 1.3 Dependency & Native Library Audit
Inspection of `Cargo.lock` (2,046 lines) reveals:
- **No OpenSSL**: Crate stack uses pure Rust cryptographic implementations (`argon2`, `sha2`, `blake2`, `password-hash`). No `openssl-sys` or `native-tls` exists in the dependency tree.
- **SQLite Engine**: `sqlx` is configured with `features = ["sqlite-bundled", ...]`. This pulls `libsqlite3-sys` with bundled SQLite source code.
- **C Compiler**: `libsqlite3-sys` uses the `cc` build crate to compile bundled `sqlite3.c`. It requires a C compiler (`gcc` or `clang`), but does not require system SQLite shared libraries (`libsqlite3-dev`) or `pkg-config`.
- **Pre-installed Tools in `rust:slim-bookworm`**:
  - `/usr/bin/gcc` (present)
  - `/usr/bin/cc` (symlink to gcc, present)
  - `/usr/bin/strip` (present from `binutils`)
  - `pkg-config` (absent, not needed)
  - Result: No `apt-get install` commands are needed in the Rust builder stage!

---

## 2. Docker Image Selection for Rust Builder Stage

| Docker Image Tag | Rustc Version | Debian Base | Glibc Version | Toolchain Compatibility | Recommendation |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `rust:1.85-slim-bookworm` | 1.85.0 | Bookworm (12) | 2.36 | **FAILED** (MSRV 1.99 violation) | ❌ Incompatible |
| `rust:bookworm` (full) | 1.99.1 | Bookworm (12) | 2.36 | Clean build | ⚠️ Unnecessary bloat (~900MB base) |
| `rust:alpine` | 1.99.1 | Alpine Linux | musl | Requires musl target / libsqlite3-sys handling | ⚠️ Risk of glibc vs musl mismatch with Debian runtime |
| `rust:slim-bookworm` | 1.99.1 | Bookworm (12) | 2.36 | Clean build, exact GLIBC match with runtime | ✅ **Optimal Recommendation** |

### Rationale for `rust:slim-bookworm`:
1. **GLIBC ABI Parity**: Both `rust:slim-bookworm` and `debian:bookworm-slim` share `GLIBC 2.36-9+deb12u14`. Binaries built in the builder run seamlessly in the runtime container without dynamic linker or glibc version conflicts.
2. **Slim Footprint**: Base image download size is ~220 MB (compared to ~900 MB for `rust:bookworm`).
3. **Batteries Included**: Contains GCC and `strip` out of the box.

---

## 3. Cargo Build Caching Optimizations

### 3.1 The Problem with Naive `COPY . .`
In a standard Dockerfile, copying all source files before `cargo build` invalidates the entire build layer whenever any file (e.g. Svelte frontend, markdown docs, tests) changes. Compiling all 200+ dependencies takes **~1 minute 45 seconds** on a modern multi-core CPU.

### 3.2 Dummy Scaffolding Pattern
By copying only the manifests first and generating dummy placeholder files, Docker caches the compiled dependencies in an intermediate image layer:

1. Copy `Cargo.toml`, `Cargo.lock`, `crates/core/Cargo.toml`.
2. Generate dummy source files:
   - `crates/core/src/lib.rs` -> `pub fn dummy() {}`
   - `crates/core/src/main.rs` -> `fn main() {}`
   - `crates/core/src/bootstrap.rs` -> `fn main() {}`
3. **Crucial Pitfall Discovered During Investigation**:
   `crates/core/Cargo.toml` explicitly specifies:
   ```toml
   [[test]]
   name = "p0"
   required-features = ["fixture-bench"]

   [[test]]
   name = "catalog"
   required-features = ["fixture-bench"]
   ```
   If `tests/p0.rs` and `tests/catalog.rs` are missing, Cargo rejects the manifest before downloading crates:
   ```
   error: crates/core/Cargo.toml: can't find `catalog` test at `tests/catalog.rs`
   error: crates/core/Cargo.toml: can't find `p0` test at `tests/p0.rs`
   ```
   Therefore, dummy test files (`touch crates/core/tests/p0.rs crates/core/tests/catalog.rs`) must also be created during dummy scaffolding.
4. Precompile dependencies:
   ```dockerfile
   RUN --mount=type=cache,target=/usr/local/cargo/registry \
       --mount=type=cache,target=/usr/local/cargo/git \
       cargo build --release --locked --bin media-poc --bin media-bootstrap
   ```
5. Copy actual workspace sources (`COPY crates/ ./crates/`).
6. Remove dummy binary/deps artifacts and touch real source files:
   ```dockerfile
   RUN touch crates/core/src/lib.rs crates/core/src/main.rs crates/core/src/bootstrap.rs && \
       rm -f target/release/deps/media_poc* target/release/deps/media_bootstrap* target/release/media-poc target/release/media-bootstrap && \
       cargo build --release --locked --bin media-poc --bin media-bootstrap && \
       strip --strip-all target/release/media-poc target/release/media-bootstrap
   ```

### 3.3 Empirical Benchmark Results
Empirical tests executed inside Docker container on x86_64 Linux:
- **Cold dependency precompilation**: 1m 37s (compiles 184 dependency crates + libsqlite3-sys).
- **Application recompilation on source code change**: **20.70s** (compiles only `media-poc v0.1.0`).
- **Speedup**: **> 4.5x faster** incremental builds. Zero dependencies recompiled.

---

## 4. Binary Stripping & Size Reduction

### 4.1 Measured Binary Sizes
Measured inside `rust:slim-bookworm` container:

| Binary | Unstripped Size | Stripped Size (`strip --strip-all`) | Size Reduction | % Saved |
| :--- | :--- | :--- | :--- | :--- |
| `media-poc` | 11.0 MB (11,534,336 bytes) | 7.6 MB (7,969,176 bytes) | 3.4 MB | 31.0% |
| `media-bootstrap` | 5.1 MB (5,347,840 bytes) | 3.9 MB (4,089,472 bytes) | 1.2 MB | 23.5% |
| **Total** | **16.1 MB** | **11.5 MB** | **4.6 MB** | **28.6%** |

### 4.2 Dynamic Linking Verification
Output of `ldd` on stripped binaries:
```
linux-vdso.so.1 (0x00007ffc880e4000)
libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1 (0x0000718dab25f000)
libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x0000718dab17f000)
libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x0000718daaf9d000)
/lib64/ld-linux-x86-64.so.2 (0x0000718dab66a000)
```
- No external libraries (e.g. OpenSSL, SQLite, zlib, libxml2) are required.
- Runtime image `debian:bookworm-slim` contains all required libraries by default (`libc6` and `libgcc-s1`).

### 4.3 Runtime Execution Verification
Both stripped binaries were verified executing inside a fresh `debian:bookworm-slim` container:
- `media-poc --help` successfully invoked without dynamic linker error.
- `media-bootstrap` successfully invoked and reported:
  `bootstrap requires a local interactive terminal and accepts no arguments` (matching `src/bootstrap.rs:17-21`).

---

## 5. Concrete Recommendation & Dockerfile Snippet for Stage 2

Here is the exact, tested Dockerfile specification for Stage 2 (`rust-builder`):

```dockerfile
# ==============================================================================
# Stage 2: Rust Workspace Builder
# ==============================================================================
FROM rust:slim-bookworm AS rust-builder
WORKDIR /build

# 1. Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml ./crates/core/

# 2. Scaffold dummy source hierarchy to allow dependency precompilation.
# NOTE: Cargo manifest validation requires tests declared in [[test]] blocks (p0, catalog)
# to exist, even if only building binary targets.
RUN mkdir -p crates/core/src crates/core/tests && \
    echo 'pub fn dummy() {}' > crates/core/src/lib.rs && \
    echo 'fn main() {}' > crates/core/src/main.rs && \
    echo 'fn main() {}' > crates/core/src/bootstrap.rs && \
    touch crates/core/tests/p0.rs crates/core/tests/catalog.rs

# 3. Precompile workspace dependencies (cached in layer and BuildKit cache mount)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked --bin media-poc --bin media-bootstrap

# 4. Copy actual workspace crates (including shared brand.rs and core migrations)
COPY crates/ ./crates/

# 5. Clean dummy binary/deps artifacts and recompile actual release binaries
RUN touch crates/core/src/lib.rs crates/core/src/main.rs crates/core/src/bootstrap.rs && \
    rm -f target/release/deps/media_poc* target/release/deps/media_bootstrap* target/release/media-poc target/release/media-bootstrap && \
    cargo build --release --locked --bin media-poc --bin media-bootstrap && \
    strip --strip-all target/release/media-poc target/release/media-bootstrap
```

### Integration into Stage 3 (Runtime):
In the final runtime stage (`FROM debian:bookworm-slim`), the binaries are copied simply via:
```dockerfile
COPY --from=rust-builder /build/target/release/media-poc /usr/local/bin/media-poc
COPY --from=rust-builder /build/target/release/media-bootstrap /usr/local/bin/media-bootstrap
```

---

## 6. Context Filter Recommendations for `.dockerignore`

To ensure fast Docker context transfer and prevent local build artifacts from polluting the container, `.dockerignore` should include:

```dockerignore
# VCS & Agent Metadata
.git
.gitignore
.agents/
ORIGINAL_REQUEST.md

# Rust Build Artifacts & Caches
target/
**/*.rs.bk

# Node & Frontend Build Artifacts
node_modules/
apps/web/node_modules/
apps/web/.svelte-kit/
apps/web/build/

# Media Fixtures & Test Data
tests/fixtures/generated/
tests/evidence/
data/
*.sqlite3
*.sqlite3-wal
*.sqlite3-shm

# Environment & Editor Files
.env*
*.log
.DS_Store
Thumbs.db
```
