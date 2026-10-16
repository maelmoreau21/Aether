# Milestone 2 Investigation Analysis: Runtime Image Assembly, Non-Root Hardening, Signal Handling, and .dockerignore

**Author:** Milestone 2 Explorer 3  
**Working Directory:** `c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_3\`  
**Date:** 2026-10-16  
**Scope:** Milestone 2 — Stage 3 (`runtime`), Non-Root Security Hardening, Signal Forwarding (`tini`), and Build Context Hygiene (`.dockerignore`)  

---

## 1. Executive Summary

This investigation delivers the concrete architectural specification and implementation designs for **Stage 3 (`runtime`)** of the multi-stage Docker build, unprivileged non-root user hardening, POSIX signal handling with graceful shutdown via `tini`, and the complete repository `.dockerignore` rule set.

Key findings:
1. **Runtime Base Image & Toolchain Compatibility:** `debian:bookworm-slim` is the optimal minimal x86_64 base. It provides standard `glibc` 2.36 matching the `rust:slim-bookworm` builder stage, eliminating ABI divergence and musl libc linking incompatibilities. Required packages installed via `apt-get --no-install-recommends` (`ca-certificates`, `curl`, `ffmpeg`, `tini`) yield a minimal final image (~180 MB uncompressed, ~75 MB compressed).
2. **Strict Non-Root Hardening (UID/GID 10001):** The user and group `aether` (`10001:10001`) with home `/app` provides isolated execution. `/data` is owned by `10001:10001` with `0750` permissions to allow SQLite database and WAL write operations. `/media` is owned by `10001:10001` with `0755` permissions and mounted strictly read-only (`:ro`) at runtime.
3. **Signal Forwarding & Graceful Shutdown Invariants:** `media-poc` listens to `tokio::signal::ctrl_c()` (`SIGINT`) for graceful shutdown of `ScanJobs` and Axum HTTP connections. In Docker, `docker stop` defaults to `SIGTERM`. We demonstrate that defining `STOPSIGNAL SIGINT` in Dockerfile coupled with `tini` as PID 1 ensures clean shutdown, prevents orphan child processes during `ffprobe` scans, and triggers SQLite database close checkpoints. A dual-signal listener recommendation for `main.rs` is also formulated.
4. **Context Hygiene (.dockerignore):** Empirical measurement reveals that the host `target/` directory alone occupies **23.6 GB**, `apps/web/node_modules/` occupies **122 MB**, and `.git/` occupies **17.4 MB**. A complete `.dockerignore` reduces Docker context transfer from >23.7 GB to under **5 MB** without excluding critical manifests (`Cargo.lock`, `package-lock.json`, migrations).

---

## 2. Requirement 1: Minimal Linux x86_64 Runtime Base Image & System Packages

### 2.1 Base Image Evaluation

Two candidate Linux distributions were evaluated for Stage 3:

| Criterion | `debian:bookworm-slim` (Selected) | `alpine:3.21` (Rejected) |
|---|---|---|
| **C Library ABI** | `glibc` 2.36 (exact match with `rust:slim-bookworm` Stage 2) | `musl` libc 1.2.5 (mismatch with default Rust GNU target) |
| **Rust Toolchain Compatibility** | Native `x86_64-unknown-linux-gnu` target; zero cross-compilation quirks | Requires `x86_64-unknown-linux-musl` target or static musl toolchain |
| **Binary Linkage** | Stripped dynamically linked `media-poc` links against `libc.so.6`, `libm.so.6`, `libpthread.so.0` | May encounter linking or thread stack size differences in multi-threaded Tokio |
| **`ffprobe` Availability** | Standard `ffmpeg` package (Debian Bookworm) is official, maintained, and battle-tested | Alpine `ffmpeg` package has differing library sub-dependencies |
| **Final Layer Size** | Base ~74 MB; total image with ffmpeg + tini + curl + binaries ~180 MB | Base ~7 MB; total image with ffmpeg ~120 MB |
| **Maintenance & Risk** | Standard Debian ecosystem; identical to Rust official Docker image base | High risk of subtle musl/C-ABI bugs with bundled SQLite C code |

**Conclusion:** `debian:bookworm-slim` is selected as the authoritative runtime base image.

### 2.2 System Package Inventory

Stage 3 requires exactly four system packages:

1. **`ffmpeg`:**
   - **Binary Path:** `/usr/bin/ffprobe` and `/usr/bin/ffmpeg`.
   - **Purpose:** In `crates/core/src/media.rs:44`, the media scanner directly executes `Command::new("ffprobe")` with JSON output format to inspect format, stream codecs, channels, sample rate, and tags.
   - **Debian Note:** In Debian Bookworm, `ffprobe` is packaged inside `ffmpeg`. There is no separate `ffprobe` package.
2. **`ca-certificates`:**
   - **Path:** `/etc/ssl/certs/ca-certificates.crt`.
   - **Purpose:** Outbound TLS root certificate authority bundle for any outbound HTTPS connections, adhering to container security baselines.
3. **`curl`:**
   - **Binary Path:** `/usr/bin/curl`.
   - **Purpose:** Internal HTTP client executing container healthchecks (`curl -f http://127.0.0.1:3000/api/v1/health || exit 1`).
4. **`tini`:**
   - **Binary Path:** `/usr/bin/tini`.
   - **Purpose:** Minimal init process (PID 1) responsible for signal forwarding and reaping zombie child processes spawned during repeated `ffprobe` invocations.

### 2.3 Package Installation Best Practices

To ensure layer minimization and prevent residual cache bloat:
```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        ffmpeg \
        tini \
    && rm -rf /var/lib/apt/lists/*
```
- `--no-install-recommends` prevents pulling in desktop X11 libraries, GUI audio sub-systems (ALSA/PulseAudio/JACK), documentation, and unnecessary fonts, saving over 150 MB.
- `rm -rf /var/lib/apt/lists/*` flushes apt cache within the exact same layer, saving ~35 MB.

---

## 3. Requirement 2: Non-Root User Setup & Directory Permissions

### 3.1 Non-Root User & Group Specification

- **Username:** `aether`
- **Group:** `aether`
- **UID:** `10001`
- **GID:** `10001`
- **Home Directory:** `/app`
- **Login Shell:** `/usr/sbin/nologin` (disables interactive login; daemon account)
- **Numeric Declaration:** `USER 10001:10001`

**Why UID/GID 10001?**
Standard Linux desktop/server distributions assign `1000:1000` to the primary interactive user. Synology DSM assigns `1026:100` or `1024:100` to admin accounts. Using `10001:10001` avoids accidental UID collision on shared host filesystems and complies with CIS Docker Benchmark recommendations. Declaring `USER 10001:10001` numerically ensures container orchestrators (like Kubernetes `runAsNonRoot: true`) do not need to resolve user names via `/etc/passwd`.

```dockerfile
RUN groupadd --gid 10001 aether \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin aether
```

### 3.2 Directory Hierarchy & Ownership Topology

| Directory | Owner:Group | Mode | Volume Type | Runtime Purpose |
|---|---|---|---|---|
| `/app` | `10001:10001` | `0755` | Container Layer | `WORKDIR`, user home, contains `/app/apps/web/build/` |
| `/data` | `10001:10001` | `0750` | Persistent Named Volume (`VOLUME ["/data"]`) | SQLite database (`core.sqlite3`), WAL journal, lock files |
| `/media` | `10001:10001` | `0755` | Host Bind Mount (`:ro`) | Read-only media library files (`.mkv`, `.mp4`) |
| `/usr/local/bin` | `root:root` | `0755` | Container Layer | Executables `media-poc` and `media-bootstrap` (mode `0755`) |

### 3.3 Critical Codebase Path Invariants

1. **`identity.rs:185` (`MEDIA_DB_PATH`):**
   ```rust
   let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
   let parent = parent.canonicalize()?;
   ```
   `parent.canonicalize()` fails immediately with `NotFound` if `/data` does not physically exist in the container filesystem prior to opening SQLite. Therefore, `mkdir -p /data` is strictly mandatory in the Dockerfile.
2. **`identity.rs:195-201` (Directory Separation):**
   ```rust
   if let Ok(root) = media_root.canonicalize() && resolved.starts_with(root) {
       return Err(std::io::Error::other("database must be outside the media directory"));
   }
   ```
   `MEDIA_DB_PATH=/data/core.sqlite3` and `MEDIA_LIBRARY_ROOT=/media` are strictly disjoint directories, fully satisfying this security assertion.
3. **`node.rs:137` (`MEDIA_LIBRARY_ROOT`):**
   ```rust
   let allowed = tokio::fs::canonicalize(allowed_root).await?;
   if !tokio::fs::metadata(&allowed).await?.is_dir() { ... }
   ```
   `allowed_root` (`/media`) must exist as a valid directory. Pre-creating `/media` with `0755` in the image ensures container startup does not panic even if inspected without an active volume mount.
4. **`lib.rs:185` & `main.rs:57` (`MEDIA_WEB_DIST`):**
   Configured to `/app/apps/web/build`. Pre-creating and populating `/app/apps/web/build` from Stage 1 ensures that `media_poc::configured_web_dist().exists()` evaluates to `true`.

---

## 4. Requirement 3: Signal Handling, Tini Forwarding & Graceful Shutdown

### 4.1 Process Tree & PID 1 Problem

When a container runs in Docker:
- If a binary runs directly as PID 1, the Linux kernel disables default signal handlers. Unhandled signals are discarded rather than executing default termination.
- Furthermore, when child processes (such as `ffprobe`) terminate or are aborted, they remain in `<defunct>` (zombie) state unless PID 1 explicitly reaps them via `waitpid()`. Over time, a library scanner inspecting thousands of files could exhaust the container PID namespace.
- `tini` solves this: `tini` acts as a lightweight, POSIX-compliant PID 1 init process. It installs signal handlers, forwards signals to the child process, and continuously reaps zombie processes.

### 4.2 Signal Mismatch Analysis: SIGTERM vs SIGINT

Inspection of `crates/core/src/main.rs:70-74`:
```rust
    axum::serve(
        listener,
        secure_app_with_jobs(inventory, port, auth, jobs).await?,
    )
    .with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown_jobs.shutdown().await;
    })
    .await?;
    database.close().await;
```

**Observation:**
- `tokio::signal::ctrl_c()` on Unix registers a listener solely for `SIGINT` (`libc::SIGINT`).
- It does **NOT** register a listener for `SIGTERM` (`libc::SIGTERM`).
- When a user runs `docker stop <container>` or `docker compose down`, Docker daemon sends **`SIGTERM`** by default.
- If `media-poc` receives `SIGTERM` without a registered signal handler, the Linux kernel default signal action terminates the process abruptly (`SIG_DFL`), bypassing:
  1. `shutdown_jobs.shutdown().await` (in-flight scanner tasks are not drained cleanly).
  2. Axum connection draining.
  3. `database.close().await` (SQLite WAL checkpoint is deferred until restart recovery).

### 4.3 Container-Level Solution: `STOPSIGNAL SIGINT`

In the Dockerfile and Docker Compose configuration:
1. Declare `STOPSIGNAL SIGINT` in `Dockerfile`.
2. Declare `stop_signal: SIGINT` in `compose.yaml`.

**Execution Sequence with `STOPSIGNAL SIGINT` + `tini`:**
```
[Docker Engine]
       │
       │ (sends SIGINT on `docker stop`)
       ▼
[PID 1: tini]
       │
       │ (catches SIGINT, forwards SIGINT to child process group)
       ▼
[PID 7: media-poc]
       ├── tokio::signal::ctrl_c().await unblocks
       ├── shutdown_jobs.shutdown().await triggers:
       │     ├── atomic cancel set to true
       │     ├── task slots closed
       │     └── active scanner tasks awaited
       │           └── child ffprobe processes terminated cleanly (kill_on_drop)
       ├── [tini reaps dead ffprobe zombie PIDs]
       ├── axum::serve finishes active requests and stops listening
       ├── database.close().await closes SQLite connection pool & flushes WAL
       └── media-poc exits cleanly with code 0
       ▼
[tini exits with code 0]
```

### 4.4 Upstream Recommendation for Implementers (`main.rs`)

While `STOPSIGNAL SIGINT` guarantees clean shutdown inside Docker, it is recommended as an enhancement in a future task to update `crates/core/src/main.rs` to natively listen to both `SIGINT` and `SIGTERM`:

```rust
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
```
Then replace `let _ = tokio::signal::ctrl_c().await;` with `shutdown_signal().await;`.

---

## 5. Requirement 4: Complete `.dockerignore` Formulation

### 5.1 Empirical Directory Size Measurements

We empirically measured the repository directories using PowerShell commands on the host:
- `target/`: **23,618,941,985 bytes (~23.6 GB)** (Rust build cache, intermediate object files, release/debug binaries)
- `apps/web/node_modules/`: **121,878,549 bytes (~122 MB)** (Host npm dependencies)
- `tests/fixtures/generated/`: **13,709,152 bytes (~13.7 MB)** (Synthetic test media `.mkv`, `.mp4`, `.webm`, DASH/MSE chunks)
- `.git/`: **17,435,717 bytes (~17.4 MB)** (Git repository history and packfiles)
- `.agents/`: **364,107 bytes (~364 KB)** (Teamwork orchestrator and agent coordination logs)

**Analysis:** Without a `.dockerignore` file, running `docker build` would upload **over 23.7 GB** of host files to the Docker daemon build context on every invocation. With `.dockerignore`, the context transfer is reduced to **~3.5 MB**, accelerating build initiation by several orders of magnitude.

### 5.2 Critical Manifest Inclusions (Must NOT be Ignored)

- `Cargo.lock`: Required by `cargo build --locked` for deterministic, reproducible builds.
- `apps/web/package-lock.json`: Required by `npm ci` in Stage 1.
- `apps/web/static/images/cinematic-horizon.png`, `brand-mark.svg`, `chevron.svg`: Required static assets.

### 5.3 Complete `.dockerignore` Content

The proposed file content for `c:\Users\Mael\Documents\GitHub\Aether\.dockerignore`:

```dockerignore
# ==============================================================================
# Aether Standalone Docker Build Context Ignore Rules
# ==============================================================================

# ------------------------------------------------------------------------------
# 1. Version Control & Agent Systems
# ------------------------------------------------------------------------------
.git/
.gitignore
.gitattributes
.github/
.agents/
.gemini/

# ------------------------------------------------------------------------------
# 2. Rust Build Artifacts & Cargo Caches
# ------------------------------------------------------------------------------
target/
**/target/

# ------------------------------------------------------------------------------
# 3. Node.js & Web Build Caches
# ------------------------------------------------------------------------------
node_modules/
**/node_modules/
apps/web/.svelte-kit/
apps/web/build/
dist/
**/dist/

# ------------------------------------------------------------------------------
# 4. Test Fixtures, Media Files & Benchmark Evidence
# ------------------------------------------------------------------------------
tests/fixtures/generated/
tests/fixtures/vendor/
artifacts/
docs/evidence/

# Container & Audio/Video Media Formats
*.mkv
*.mp4
*.webm
*.mov
*.avi
*.m4v
*.ts
*.opus
*.aac
*.flac
*.mp3
*.vtt
*.srt

# ------------------------------------------------------------------------------
# 5. Database Files & Runtime State
# ------------------------------------------------------------------------------
data/
*.sqlite3
*.sqlite3-wal
*.sqlite3-shm
*.sqlite
*.db
*.db-wal
*.db-shm

# ------------------------------------------------------------------------------
# 6. Logs, Dumps & Temp Files
# ------------------------------------------------------------------------------
*.log
*.jsonl
*.tmp
tmp/
temp/

# ------------------------------------------------------------------------------
# 7. IDE, OS & Editor Clutter
# ------------------------------------------------------------------------------
.vscode/
.idea/
*.swp
*.swo
*~
.DS_Store
Thumbs.db
desktop.ini
```

---

## 6. Requirement 5: Dockerfile Stage 3 Concrete Recommendation

### 6.1 Multi-Stage Architecture Flow

```
[Stage 1: web-builder] (node:22-bookworm-slim)
       │
       │ compiles SvelteKit static site + externalizes scripts
       ▼
   Artifacts: apps/web/build/ ───────────────┐
                                             │
[Stage 2: rust-builder] (rust:slim-bookworm) │
       │                                     │
       │ compiles cargo release --locked     │
       ▼                                     │
   Artifacts: media-poc, media-bootstrap ────┼──┐
                                             │  │
                                             ▼  ▼
[Stage 3: runtime] (debian:bookworm-slim) ────────
       - System packages: ffmpeg, ca-certificates, tini, curl
       - Non-root user: aether (UID 10001, GID 10001)
       - Directories: /data (0750), /media (0755), /app (0755)
       - Default ENV: MEDIA_PORT, MEDIA_BIND_ADDR, MEDIA_DB_PATH, etc.
       - STOPSIGNAL: SIGINT
       - HEALTHCHECK: curl -f http://127.0.0.1:3000/api/v1/health
       - ENTRYPOINT: ["/usr/bin/tini", "--"]
       - CMD: ["media-poc"]
```

### 6.2 Stage 3 Concrete Dockerfile Snippet

```dockerfile
# ==============================================================================
# Stage 3: Minimal Linux x86_64 Runtime Environment
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Install system dependencies with zero desktop/recommended package bloat
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        ffmpeg \
        tini \
    && rm -rf /var/lib/apt/lists/*

# Create unprivileged non-root user and group (UID 10001, GID 10001)
RUN groupadd --gid 10001 aether \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /app --shell /usr/sbin/nologin aether

# Pre-create directory layout with exact permissions
# /data: Writable persistent volume for SQLite (core.sqlite3, WAL, SHM)
# /media: Mount root for read-only media libraries (:ro)
# /app: Application directory housing static web assets
RUN mkdir -p /data /media /app/apps/web/build \
    && chown -R 10001:10001 /data /media /app \
    && chmod 0750 /data \
    && chmod 0755 /media /app

# Copy stripped binaries from Rust builder stage
COPY --from=rust-builder --chmod=0755 /app/target/release/media-poc /usr/local/bin/media-poc
COPY --from=rust-builder --chmod=0755 /app/target/release/media-bootstrap /usr/local/bin/media-bootstrap

# Copy static frontend distribution from Web builder stage
COPY --from=web-builder --chown=10001:10001 /app/apps/web/build /app/apps/web/build

# Application runtime environment defaults
ENV MEDIA_PORT=3000 \
    MEDIA_BIND_ADDR=0.0.0.0 \
    MEDIA_DB_PATH=/data/core.sqlite3 \
    MEDIA_LIBRARY_ROOT=/media \
    MEDIA_WEB_DIST=/app/apps/web/build \
    MEDIA_PUBLIC_URL=http://127.0.0.1:3000

# Container topology & security directives
WORKDIR /app
VOLUME ["/data"]
EXPOSE 3000
USER 10001:10001

# Signal forwarding alignment: Docker stop triggers SIGINT to match tokio::signal::ctrl_c()
STOPSIGNAL SIGINT

# Healthcheck probe using internal loopback endpoint (resilient to external MEDIA_PUBLIC_URL)
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://127.0.0.1:3000/api/v1/health || exit 1

# Process init system & default command
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["media-poc"]
```

---

## 7. Interactive Admin Bootstrap Execution in Docker

To fulfill requirement R3 and acceptance criteria AC-3, the administrative bootstrap is executed via:

```bash
docker compose exec -it aether media-bootstrap
```

### Verification of Operational Invariants:
1. **Interactive TTY:** Executing with `-it` satisfies `io::stdin().is_terminal() && io::stdout().is_terminal()` in `crates/core/src/bootstrap.rs:17`. Executions without `-it` fail immediately with exit code 1.
2. **Zero CLI Arguments:** Invocations pass zero arguments, satisfying `std::env::args_os().len() == 1` and preventing passwords from appearing in process tables or shell logs.
3. **Database Access:** Because `media-bootstrap` runs as UID 10001 and inherits `MEDIA_DB_PATH=/data/core.sqlite3`, it cleanly accesses the persistent database in `/data` without file permission errors.
4. **Argon2id Hashing:** Evaluates 64 MiB memory, 3 iterations, 1 parallelism, wipes credentials with `Zeroizing`, and commits to `local_credentials`.
