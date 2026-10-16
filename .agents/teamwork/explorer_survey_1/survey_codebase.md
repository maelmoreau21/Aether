# Codebase Architecture Survey: Aether Docker Standalone Distribution

**Date**: 2026-10-16  
**Investigator**: Codebase Architecture Explorer  
**Scope**: Authoritative Rust workspace (`crates/core`, `crates/shared`), SvelteKit frontend (`apps/web`), toolchain scripts, media scanner, and Docker context requirements.

---

## Executive Summary

1. **Rust Workspace & Binaries**: The Cargo workspace consists of a single workspace member `crates/core` (package name `media-poc`, edition `2024`, `rust-version = "1.99"`). It produces two binaries:
   - `media-poc` (`src/main.rs`): the unified standalone server combining Core (auth, SQLite, catalog, permissions) and local Media Node (scanning, probing, images, NFO, streaming).
   - `media-bootstrap` (`src/bootstrap.rs`): interactive CLI tool for initializing the first `administrator` account.
2. **HTTP & Static Asset Serving**: Axum currently serves static assets from `"apps/web/build"` under the `/app` route prefix via `tower_http::services::ServeDir::new("apps/web/build").append_index_html_on_directories(true)`. The path is relative to the process working directory.
3. **Frontend Build & SvelteKit**: `apps/web` is configured with `@sveltejs/adapter-static` (v4.0.0), `base: '/app'`, `prerender = true`, and `trailingSlash = 'always'`. The build outputs static HTML/CSS/JS into `apps/web/build/`. A post-build script `scripts/externalize-bootstrap.mjs` externalizes inline scripts into separate `.js` files to strictly satisfy Axum's Content Security Policy. No permanent Node.js runtime is needed in production.
4. **ffprobe Integration**: `ffprobe` is invoked via `tokio::process::Command::new("ffprobe")` in `crates/core/src/media.rs:42`. It resolves strictly through system `PATH` without any environment variable override. Execution is constrained by strict demuxer/protocol whitelists, a 10-second timeout, and a 1 MiB output ceiling. Missing `ffprobe` returns `ProbeError::Io`, causing the scanner to abort publication to prevent inventory invalidation.
5. **Build and Test Toolchain**: All suites pass cleanly:
   - Rust: `cargo fmt --check`, `cargo clippy --locked --all-features --all-targets -- -D warnings`, `cargo test --locked --all-features` (50 passed, 5 ignored).
   - Frontend: `npm run check` (0 errors, 0 warnings), `npm run lint` (clean), `npm test` (13 passed), `npm run build` (successful static build).
   - Scripts: Node statistic and seek policy tests (10 passed).
6. **Docker Context Exclusions**: Target build directory `target/`, `node_modules/`, `apps/web/node_modules/`, `apps/web/.svelte-kit/`, `apps/web/build/`, generated media fixtures `tests/fixtures/generated/`, test evidence `artifacts/` and `docs/evidence/`, `.git/`, `.agents/`, and local SQLite databases `data/` must be excluded in `.dockerignore`.

---

## Detailed Investigation

### 1. Rust Workspace Structure, Binaries, and Standalone Execution

#### Workspace Layout
- **Root `Cargo.toml`** (`Cargo.toml:1-9`):
  ```toml
  [workspace]
  members = ["crates/core"]
  resolver = "3"

  [workspace.package]
  edition = "2024"
  rust-version = "1.99"
  ```
- **`crates/shared`**: Currently contains only `crates/shared/src/brand.rs` (`pub const DEFAULT_NAME: &str = "Aether";`). It does not have its own `Cargo.toml`. Instead, `crates/core/src/lib.rs:3-4` imports it directly via:
  ```rust
  #[path = "../../shared/src/brand.rs"]
  mod brand;
  ```
- **`crates/core/Cargo.toml`** (`crates/core/Cargo.toml:1-46`):
  - Package name: `media-poc` (v0.1.0).
  - Default binary: `default-run = "media-poc"`.
  - Dependencies: `axum = "0.8.9"`, `tokio = "1.53.3"`, `tower = "0.5"`, `tower-http = "0.7.1"`, `sqlx = "0.9.0"` (with features `runtime-tokio`, `sqlite-bundled`, `migrate`, `macros`), `argon2 = "0.6.0"`, `rpassword = "7.5.4"`, `zeroize = "1"`, `uuid = "1"`, `sha2 = "0.10.9"`, `quick-xml = "0.42.0"`, `image = "=0.25.10"`.
  - Feature: `fixture-bench = []`.

#### Produced Binaries
Two binaries are defined and compiled:
1. **`media-poc`** (entry point `crates/core/src/main.rs:7-45`):
   - The unified standalone service daemon.
   - Hosts Axum HTTP API, Core identity/sessions/catalog, and the local Media Node scanner/streamer in a single process.
2. **`media-bootstrap`** (entry point `crates/core/src/bootstrap.rs:8-46`):
   - Defined in `crates/core/Cargo.toml:32-34`:
     ```toml
     [[bin]]
     name = "media-bootstrap"
     path = "src/bootstrap.rs"
     ```
   - Local administrator CLI bootstrap tool. Requires interactive terminal (`io::stdin().is_terminal() && io::stdout().is_terminal()`, `bootstrap.rs:17`) and accepts no CLI arguments to prevent password leakage in process tables or history.
   - Computes Argon2id hash (64 MiB, 3 passes, 1 lane) and inserts the first `administrator` into `users` and `local_credentials` in SQLite, permanently recording completion in `bootstrap_state` (`identity.rs:114-172`).

#### Standalone Mode & Unified Process Wiring
The backend already operates as a unified standalone binary combining Core and local Media Node:
- In `crates/core/src/main.rs:8-45`:
  - **Media Root**: Reads `MEDIA_LIBRARY_ROOT` (`main.rs:9`). If unset, falls back to `MEDIA_FIXTURE_DIR` or `"tests/fixtures/generated"` (`main.rs:10-12`).
  - **SQLite Database**: Locates database path via `identity::configured_path(&root)` (`main.rs:16`, `identity.rs:177-203`). Defaults to `"data/core.sqlite3"`. Validates that the database path resides outside the media root to prevent data leakage. Initializes SQLite pool with WAL journal mode, busy timeouts, and runs embedded migrations (`sqlx::migrate!("./migrations")`) inside an immediate transaction (`identity.rs:61-99`).
  - **Inventory Initialization**:
    ```rust
    let inventory = if library_root.is_some() {
        Inventory::local(root).await?
    } else {
        Inventory::load(root).await?
    };
    ```
    When `MEDIA_LIBRARY_ROOT` is configured, `Inventory::local(root)` starts in standalone mode without loading or probing any experimental fixtures (`node.rs:231-237`).
  - **Scan Orchestrator**: Creates `Arc<ScanJobs>` (`main.rs:32`), coordinating background scans with concurrency limits (`Semaphore::new(1)`), cancellation tokens, and restart cleanup (`scans.rs:19-56, 79-80`).
  - **Routing & Execution**: Calls `secure_app_with_jobs(inventory, port, auth, jobs)` (`crates/core/src/lib.rs:82-119`), which unifies:
    - Public static routes (`public_routes(mode)`)
    - Authenticated media and streaming routes (`media_routes(inventory)` + `permissions::require_media`)
    - Scanner admin routes (`scans::router`)
    - Library configuration routes (`libraries::router`)
    - Authentication routes (`auth::router`)
  - **Graceful Shutdown**: Listens for `tokio::signal::ctrl_c()` (`main.rs:38-41`) and invokes `jobs.shutdown()`. (Note for Docker: SIGTERM handling should be considered in addition to ctrl_c/SIGINT).

---

### 2. HTTP Endpoint and Static Asset Serving

#### Static Web Serving via Axum
In `crates/core/src/lib.rs:150-167`:
```rust
fn public_routes(mode: &'static str) -> Router {
    Router::new()
        .nest_service(
            "/app",
            tower_http::services::ServeDir::new("apps/web/build")
                .append_index_html_on_directories(true),
        )
        .route(
            "/api/v1/config",
            get(move || async move {
                Json(serde_json::json!({"name": brand::DEFAULT_NAME, "mode":mode}))
            }),
        )
        .route(
            "/api/v1/health",
            get(move || async move { Json(serde_json::json!({"status":"ok", "mode":mode})) }),
        )
}
```
- **Finding**: Axum **already** serves the static frontend from `"apps/web/build"` under `/app` using `tower_http::services::ServeDir` with `append_index_html_on_directories(true)`.
- **Directory Resolution**: `"apps/web/build"` is currently relative to the current working directory of the process. In a containerized environment, the working directory must contain `apps/web/build` or the path must be configurable.

#### Complete Endpoint Inventory
| Method | Path | Auth / RBAC | Module | Purpose |
|---|---|---|---|---|
| `GET` | `/` | Anonymous | `lib.rs:29` | Embedded test bench HTML (`bench.html`) |
| `GET` | `/app/*` | Anonymous (Web assets) | `lib.rs:152-156` | SvelteKit static application bundle |
| `GET` | `/api/v1/config` | Anonymous | `lib.rs:158-162` | Public system metadata (`{ name, mode }`) |
| `GET` | `/api/v1/health` | Anonymous | `lib.rs:163-166` | Service liveness probe (`{ status: "ok", mode }`) |
| `POST` | `/api/v1/auth/login` | Anonymous | `auth.rs:155, 167` | Authenticates login/password; issues HttpOnly session cookie |
| `POST` | `/api/v1/auth/logout` | Authenticated | `auth.rs:157, 189` | Revokes session and clears cookie |
| `GET` | `/api/v1/auth/me` | Authenticated | `auth.rs:156, 206` | Returns current user info and anti-CSRF token |
| `GET` | `/api/v1/catalog` | Auth + Library Grant | `catalog.rs:21` | Paginated catalog search and browse |
| `GET` | `/api/v1/catalog/{id}` | Auth + Library Grant | `catalog.rs:60` | Detailed Work/Edition/Version/Source metadata |
| `GET` | `/api/v1/media/{id}` | Auth + Library Grant | `lib.rs:101, 130` | Playback descriptor for media player |
| `GET` | `/api/v1/assets/{id}/content` | Auth + Library Grant | `node.rs:435` | Stream audio/video content (64 KiB chunked `ServeFile`, HTTP 206/416 Range) |
| `GET` | `/api/v1/assets/{id}/image` | Auth + Library Grant | `lib.rs:102, 121` | Stream validated poster image |
| `GET` | `/api/v1/admin/libraries` | Administrator | `libraries.rs:92, 114` | List registered media libraries |
| `POST` | `/api/v1/admin/libraries` | Administrator | `libraries.rs:93, 126` | Create new library (validates canonical path) |
| `PATCH` | `/api/v1/admin/libraries/{id}` | Administrator | `libraries.rs:94, 150` | Update library name, enabled status, user grants |
| `POST` | `/api/v1/admin/libraries/{id}/scan` | Administrator | `scans.rs:81, 131` | Trigger background library scan (HTTP 202 / 409 Conflict) |
| `GET` | `/api/v1/admin/libraries/{id}/scan` | Administrator | `scans.rs:82, 157` | Query library scan job progress and status |

#### Security Middleware & Headers
Defined in `crates/core/src/lib.rs:169-193`:
- **Host & Origin Validation**:
  - `Host` header must match authority of `MEDIA_PUBLIC_URL`.
  - `sec-fetch-site` must not be `"cross-site"`.
  - `Origin` header (when present) must strictly match `MEDIA_PUBLIC_URL`.
- **Security Headers Injected**:
  - `X-Content-Type-Options: nosniff`
  - `Cache-Control: no-store`
  - `Content-Security-Policy: default-src 'none'; script-src 'self'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; frame-ancestors 'none'`
- **Cookie Settings** (`crates/core/src/auth.rs:146-151`):
  - Cookie name: `media_session`.
  - Flags: `Path=/; HttpOnly; SameSite=Strict; Max-Age=43200` (12 hours). Includes `; Secure` if `MEDIA_PUBLIC_URL` scheme is `https`.
- **Network Binding Note**:
  - `main.rs:26` currently binds to `SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)`. Inside a container, binding to `0.0.0.0` is required for port exposure while preserving `MEDIA_PUBLIC_URL` origin validation.

---

### 3. Frontend (apps/web) Build and SvelteKit Configuration

#### Package & Configuration
- **Package Manifest** (`apps/web/package.json:1-28`):
  - Name: `"media-web"`, `"private": true`, `"type": "module"`.
  - Node version: Tested on Node.js 24 (`.github/workflows/ci.yml:111`).
  - Key devDependencies:
    - `@sveltejs/adapter-static`: `4.0.0`
    - `@sveltejs/kit`: `3.1.0`
    - `svelte`: `5.57.2`
    - `vite`: `8.3.4`
    - `typescript`: `^6.0.3`
    - `vitest`: `^5.0.5`
- **Vite & SvelteKit Configuration** (`apps/web/vite.config.ts:1-5`):
  ```typescript
  import adapter from '@sveltejs/adapter-static';
  import { sveltekit } from '@sveltejs/kit/vite';
  import { defineConfig } from 'vitest/config';

  export default defineConfig({
    plugins: [
      sveltekit({
        adapter: adapter(),
        paths: { base: '/app' },
        csp: { mode: 'hash', directives: { 'script-src': ['self'] } }
      })
    ],
    test: { include: ['src/**/*.test.ts'], environment: 'node' }
  });
  ```
- **Prerendering & Routing** (`apps/web/src/routes/+layout.ts:1-3`):
  ```typescript
  export const prerender = true;
  export const trailingSlash = 'always';
  ```
  - `prerender = true`: Instructs `@sveltejs/adapter-static` to prerender all static pages during `npm run build`.
  - `trailingSlash = 'always'`: Generates directory-based HTML files (`build/index.html`, `build/login/index.html`, `build/admin/libraries/index.html`, etc.), aligning with Axum's `append_index_html_on_directories(true)`.

#### Build Process and Output
- **Build Script** (`apps/web/package.json:8`):
  `"build": "vite build && node scripts/externalize-bootstrap.mjs"`
- **Output Directory**: `apps/web/build/`
- **Contents of `apps/web/build/`**:
  - `index.html` and `index-bootstrap-0.js`
  - `_app/` (immutable JS/CSS chunks and asset manifests)
  - `admin/libraries/index.html` (and externalized bootstrap)
  - `films/index.html`
  - `series/index.html`
  - `search/index.html`
  - `login/index.html`
  - `settings/index.html`
  - `images/` (branding SVG and local artwork)
- **Bootstrap Externalization** (`apps/web/scripts/externalize-bootstrap.mjs:1-17`):
  - Traverses `apps/web/build/**/*.html`.
  - Extracts inline `<script>...</script>` injected by SvelteKit and writes them to separate `<filename>-bootstrap-0.js` files.
  - Rewrites HTML script tags to `<script src="./*-bootstrap-0.js"></script>`.
  - Ensures Axum's CSP (`script-src 'self'`) executes without `'unsafe-inline'` or CSP errors.
- **Zero Runtime Node.js Requirement**: The compiled output is 100% static files. No permanent Node.js server or `@sveltejs/adapter-node` is used at runtime.

---

### 4. ffprobe Discovery, Configuration, and Execution

#### Invocation Details
- **Location**: `crates/core/src/media.rs:41-94` (`pub async fn probe(path: &Path) -> Result<Probe, ProbeError>`).
- **Process Spawning** (`media.rs:42`):
  ```rust
  let mut child = Command::new("ffprobe")
      .args([
          "-v", "error",
          "-protocol_whitelist", "file",
          "-format_whitelist", "matroska,webm,mov",
          "-max_alloc", "67108864",
          "-probesize", "5000000",
          "-analyzeduration", "5000000",
          "-show_streams",
          "-show_format",
          "-show_data",
          "-of", "json",
      ])
      .arg(path)
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .kill_on_drop(true)
      .spawn()?;
  ```
- **Discovery Mechanism**: Invoked directly as `"ffprobe"`. It relies entirely on standard OS resolution through the `PATH` environment variable. **No environment variables** (e.g. `FFPROBE_PATH`) are defined or checked in the codebase.

#### Security & Sandboxing Constraints
- **Protocol Whitelist**: `-protocol_whitelist file` prevents network access, pipe traversal, or SSRF attacks embedded in media headers.
- **Format Whitelist**: `-format_whitelist matroska,webm,mov` restricts parsing strictly to targeted container formats.
- **Memory Ceiling**: `-max_alloc 67108864` (64 MiB limit inside ffprobe).
- **Probing Limits**: `-probesize 5000000` (5 MB) and `-analyzeduration 5000000` (5 s / 5 MB) prevent resource exhaustion on pathological inputs.
- **Output Size Ceiling**: The stdout reader is capped at `1_048_577` bytes (`media.rs:72`). If ffprobe emits more than 1 MiB, it aborts with `ProbeError::TooLarge` (`media.rs:77`).
- **Timeout**: Enforced at 10 seconds via `tokio::time::timeout(Duration::from_secs(10), ...)` (`media.rs:74`).
- **Cleanup**: `kill_on_drop(true)` ensures no orphan zombie ffprobe processes linger on timeout or drop.
- **Path Redaction**: `result.format.remove("filename")` (`media.rs:89`) strips local absolute file paths from the returned probe structure before it can reach API descriptors.

#### Integration in Media Scanner
- **Location**: `crates/core/src/node/scanner.rs:348-383`.
- **Cache Optimization**: If a file's size and `modified_ns` match SQLite cache (`CachedFile`), ffprobe is not spawned (`report.reused += 1`, `scanner.rs:352-353`).
- **Fail-Safe Error Handling** (`scanner.rs:373-374`):
  ```rust
  // Abort publication so an unavailable ffprobe cannot invalidate an inventory.
  Some(Err(media::ProbeError::Io(error))) => return Err(error),
  ```
  If `ffprobe` binary is missing from `PATH` or cannot be executed, `media::probe` returns `ProbeError::Io(error)`. The scanner immediately aborts the scan transaction. Existing catalog items and database records are preserved rather than being marked unavailable or corrupt.

---

### 5. Existing Build and Test Commands

#### Rust Workspace Commands
Execute from repository root (`c:\Users\Mael\Documents\GitHub\Aether`):
- **Code Formatting Check**:
  ```bash
  cargo fmt --check
  ```
- **Strict Clippy Linting**:
  ```bash
  cargo clippy --locked --all-features --all-targets -- -D warnings
  ```
- **Automated Integration & Unit Tests**:
  ```bash
  cargo test --locked --all-features
  ```
  - Result: **50 passed, 5 ignored**.
  - Ignored tests are explicit browser headless suites and a subprocess worker helper:
    - `browser_auth::browser_login_catalog_player_logout`
    - `browser_nfo::browser_nfo_persists_refreshes_and_plays_after_core_restart`
    - `browser_posters::browser_posters_persist_refresh_and_fallback_after_core_restart`
    - `browser_scanner::browser_scanner_persists_and_plays_after_core_restart`
    - `identity::bootstrap_process_worker`
- **Optional Browser Test Execution** (requires built frontend and Playwright Chrome):
  ```bash
  cargo test --locked --test browser_auth -- --ignored --nocapture
  cargo test --locked --test browser_scanner -- --ignored --nocapture
  cargo test --locked --test browser_nfo -- --ignored --nocapture
  cargo test --locked --test browser_posters -- --ignored --nocapture
  ```
- **Security Audit**:
  ```bash
  cargo audit
  ```

#### Web Frontend Commands
Execute from `apps/web/`:
- **Clean Dependency Installation**:
  ```bash
  npm ci
  ```
- **TypeScript & Svelte Diagnostics**:
  ```bash
  npm run check
  ```
  Executes `svelte-kit sync && svelte-check --tsconfig ./tsconfig.json`. Result: **0 errors, 0 warnings**.
- **ESLint**:
  ```bash
  npm run lint
  ```
  Executes `eslint src`. Result: **Clean**.
- **Unit Tests (Vitest)**:
  ```bash
  npm test
  ```
  Executes `vitest run`. Result: **3 test files, 13 passed**.
- **Static Production Build**:
  ```bash
  npm run build
  ```
  Executes `vite build && node scripts/externalize-bootstrap.mjs`. Result: Produces `apps/web/build/` with externalized CSP bootstrap scripts.
- **Security Audit**:
  ```bash
  npm audit --audit-level=high
  ```

#### Toolchain and Statistical Validation Scripts
Execute from repository root:
- **Reader Comparison Metrics**:
  ```bash
  node scripts/player-comparison-stats.test.cjs
  ```
- **Safe-Margin Statistics**:
  ```bash
  node scripts/safe-margin-stats.test.cjs
  ```
- **Bounded Audio Seek Policy Tests**:
  ```bash
  node --test scripts/audio-seek-policy.test.cjs
  ```
  Result: **10 tests passed**.

---

### 6. Repository Files to Exclude from Docker Context (.dockerignore)

Based on disk inventory and `.gitignore`, the following directories and file patterns must be excluded from Docker context:

```dockerignore
# Version control
.git/
.gitignore

# CI & GitHub configurations
.github/

# Agent teamwork metadata and working directories
.agents/

# Rust build artifacts and caches
target/
Cargo.lock.bak

# Node.js dependencies and caches
node_modules/
apps/web/node_modules/
apps/web/.svelte-kit/
apps/web/build/

# Synthetic media fixtures (multi-megabyte AV1/Opus files)
tests/fixtures/generated/

# Test run artifacts, browser logs, screenshots, and evidence
artifacts/
docs/evidence/

# SQLite databases, transaction logs, and local application data
data/
*.sqlite3
*.sqlite3-wal
*.sqlite3-shm

# Log files and OS metadata
*.log
.DS_Store
Thumbs.db

# Documentation not needed for image assembly
ORIGINAL_REQUEST.md
PROMPT_DEMARRAGE_IA.md
```

---

## Architectural Insights for Container Packaging

1. **Multi-Stage Build Pipeline**:
   - Stage 1 (Frontend): `node:24-bookworm-slim` or `node:24-alpine`, runs `npm ci` and `npm run build` inside `apps/web/` to produce `apps/web/build/`.
   - Stage 2 (Backend): `rust:1.99-bookworm` or `rust:1.99-alpine` (or compatible stable toolchain), compiles `media-poc` and `media-bootstrap` in release mode (`cargo build --release --locked --bin media-poc --bin media-bootstrap`).
   - Stage 3 (Runtime): Minimal Linux runtime (e.g., `debian:bookworm-slim` or `alpine`) containing `ffprobe` (via `apt-get install -y --no-install-recommends ffmpeg` or Alpine package), non-root system user, the compiled binaries, and the static frontend directory.
2. **Network Binding in Docker**:
   - Currently, `main.rs:26` binds to `127.0.0.1`. Inside a container, binding to `0.0.0.0` is required for Docker port mapping to work.
   - At the same time, `Host` and `Origin` validation in `lib.rs:175-188` checks against `MEDIA_PUBLIC_URL`. When users access the container via `http://127.0.0.1:3000` or `http://localhost:3000` or reverse proxy HTTPS, `MEDIA_PUBLIC_URL` must match the incoming request headers.
3. **Data Volume Separation**:
   - Media root (`MEDIA_LIBRARY_ROOT`) must be mounted `:ro`.
   - SQLite data directory (`MEDIA_DB_PATH` parent, e.g., `/data/core.sqlite3`) must be mounted to a writable persistent volume owned by the non-root container UID.
   - `configured_path` (`crates/core/src/identity.rs:177-203`) enforces that `MEDIA_DB_PATH` does not reside within the media root.
4. **Bootstrap Execution**:
   - `media-bootstrap` requires an interactive terminal (`tty`).
   - In Docker Compose or Docker CLI, the bootstrap command must be run with `docker compose run --rm -it aether media-bootstrap` or `docker exec -it <container> media-bootstrap`.
