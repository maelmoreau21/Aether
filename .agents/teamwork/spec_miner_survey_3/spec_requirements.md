# Aether All-in-One Docker Distribution Specification Report
**Milestone:** Survey & Specification Mining — Docker, Compose & Deployment Architecture  
**Author:** Docker & Deployment Spec Miner  
**Date:** 2026-10-16  
**Status:** Authoritative Specification Extraction Complete  

---

## 1. Executive Summary & Authoritative Hierarchy

This specification defines the exhaustive explicit and implicit requirements for packaging **Aether** as a production-grade, self-hosted, all-in-one Docker distribution. It addresses the standalone architecture targeted for both local standalone deployments and NAS deployments (specifically the **Synology DS1522+** powered by the AMD Ryzen R1600 x86_64 processor without an integrated GPU).

### Document Hierarchy of Authority
1. **`AGENTS.md`**: Mandatory non-negotiable rules. Enforces:
   - Exactly two roles: `administrator` and `user` (no guest/invited roles, no implicit admin).
   - Zero hardcoded or default credentials/passwords.
   - Read-only (`:ro`) media volumes; zero modification to user files or existing Jellyfin installations.
   - No mandatory video transcoding (especially on DS1522+ without iGPU).
   - Standalone first: single process, SQLite with SQLx, no permanent Node.js service, no second Rust server daemon, no mandatory PostgreSQL or Valkey.
   - Brand neutral codebase (`MEDIA_*` environment variables, constant brand in `crates/shared/src/brand.rs`).
2. **`ORIGINAL_REQUEST.md`**: Mission requirements R1–R4 and acceptance criteria.
3. **`ARCHITECTURE.md`**: Component boundaries (Axum Core, local Media Node, SvelteKit Web, media parsing, and SQLite database).
4. **`MEMORY.md`**: Current real codebase status (migrations 0001–0006, local sessions, library permissions, scanner, NFO, local posters).
5. **`docs/SECURITY.md`**: Threat model, Host/Origin/Sec-Fetch-Site controls, CSRF tokens, Argon2id parameters, session lifetimes, TOCTOU residual limitations.
6. **`docs/SPECIFICATION.md` & `docs/DEPLOYMENT.md`**: Packaging prerequisites, runtime invariants, and operational workflows.
7. **Existing Source Code & Tests**: Authoritative implementations in `crates/core/src/` (`main.rs`, `bootstrap.rs`, `lib.rs`, `identity.rs`, `node.rs`, `scans.rs`, `media.rs`) and `apps/web/` (`package.json`, `vite.config.ts`, `scripts/externalize-bootstrap.mjs`).

---

## 2. Features Discovered

| # | Category | Feature | Description | Inputs | Outputs | Error Behavior | Discovered Via |
|---|----------|---------|-------------|--------|---------|----------------|----------------|
| 1 | Multi-Stage Build | Node.js Frontend Compilation | Compiles SvelteKit frontend to static assets with adapter-static and externalizes inline bootstrap scripts for CSP compliance. | `apps/web/` sources, `package.json`, `package-lock.json` | `apps/web/build/` directory with static HTML/JS/CSS | Build failure on TS check error, lint error, or missing bootstrap tag | `apps/web/package.json`, `apps/web/scripts/externalize-bootstrap.mjs` |
| 2 | Multi-Stage Build | Rust Workspace Compilation | Compiles Axum Core, embedded SQLite migrations, and CLI bootstrap into release binaries. | `Cargo.toml`, `Cargo.lock`, `crates/` sources | Stripped binaries: `media-poc` (server) and `media-bootstrap` (CLI) | Cargo compilation or Clippy error | `Cargo.toml`, `crates/core/Cargo.toml` |
| 3 | Container Runtime | Minimal Linux x86_64 Runtime | Packages compiled binaries and web build into a minimal Linux x86_64 runtime image with `ffprobe` installed. | Compiled artifacts from Stages 1 & 2, Debian slim / Alpine base | Standalone `aether` container image | Image build aborts if packages or artifacts are missing | `ORIGINAL_REQUEST.md` § R1 |
| 4 | Container Runtime | Non-Root Unprivileged Execution | Container runs as non-root user (e.g., UID/GID 10001:10001 or 1000:1000) with no privileged capabilities. | `USER` directive in Dockerfile | Linux unprivileged process context | Container fails to write if data volume permissions are misconfigured | `AGENTS.md` § 7, `ORIGINAL_REQUEST.md` § R1 |
| 5 | Container Runtime | Signal Forwarding & PID 1 Init | Graceful shutdown handling via init system (tini) forwarding SIGINT/SIGTERM to Axum server. | OS signals (SIGTERM, SIGINT) | Orderly shutdown of Axum listener and in-flight scan tasks | Abrupt kill if timeout exceeded (default 10s Docker stop timeout) | `ORIGINAL_REQUEST.md` § R1, `crates/core/src/main.rs` |
| 6 | Container Runtime | Single-Process Core + Node Architecture | The single `media-poc` executable hosts Axum Core, API, SvelteKit static serving, and local Media Node. | Command `media-poc` | Unified HTTP service on port 3000 | Process aborts if port or database is unavailable | `AGENTS.md` § 3, `ARCHITECTURE.md` § 2 |
| 7 | Container Runtime | System `ffprobe` Dependency | Media scanner invokes `ffprobe` as an external subprocess for format, stream, and codec probing. | Media file path on filesystem | JSON probe output (bounded to 1 MiB) | Probe error / timeout (10s) fails individual file scan without crashing server | `crates/core/src/media.rs`, `node/scanner.rs` |
| 8 | Build Context | Strict `.dockerignore` Context Hygiene | Excludes development artifacts, large caches, media fixtures, evidence, and database files from build context. | Docker build context patterns | Minimal Docker transfer context (< 50 MB) | Build fails if required source files are accidentally ignored | `ORIGINAL_REQUEST.md` § R1 |
| 9 | Configuration | `MEDIA_PORT` | Configures the HTTP server listening port. | Port string (e.g. `"3000"`) | Bound TCP socket | ParseIntError causes process exit | `crates/core/src/main.rs:13` |
| 10 | Configuration | `MEDIA_PUBLIC_URL` | Configures exact public origin for Host/Origin checking and HTTPS session cookie `Secure` flag. | URL string (e.g. `"http://127.0.0.1:3000"` or `"https://aether.nas.lan"`) | Origin validation rule; Secure cookie flag if `https://` | Requests with mismatched `Host` or cross-site `Origin` return `403 Forbidden` | `crates/core/src/main.rs:29`, `lib.rs:170`, `auth.rs` |
| 11 | Configuration | `MEDIA_DB_PATH` | Path to persistent SQLite database file. Default `data/core.sqlite3`. | Absolute or relative file path | Initialized SQLite database with WAL mode | IO error if parent dir missing; error if inside media root | `crates/core/src/identity.rs:177` |
| 12 | Configuration | `MEDIA_LIBRARY_ROOT` | Absolute operator path to allowed media directory root. Disables fixture mode. | Absolute filesystem path (e.g. `"/media"`) | Root of local Media Node inventory | IO error if path is relative or does not exist | `crates/core/src/main.rs:9`, `node.rs:231` |
| 13 | Configuration | Network Binding Inside Docker | Ability to bind to container network interface while preserving Host/Origin boundaries. | Bind address (0.0.0.0 inside container vs host mapping) | Accessible HTTP endpoint inside Docker network | `Connection refused` if only bound to container loopback | `ORIGINAL_REQUEST.md` § R2 |
| 14 | Security | Media Volume Read-Only Isolation | Media directory mounted strictly `:ro`; scanner and player perform read-only access. | Docker volume mount `:ro` | Read-only file descriptors | Write operations rejected by OS filesystem | `AGENTS.md` § 1, 3, 6, `ORIGINAL_REQUEST.md` § R2 |
| 15 | Security | Database Volume Directory Separation | Database path must be strictly outside `MEDIA_LIBRARY_ROOT`. Verified at startup. | `MEDIA_DB_PATH` and `MEDIA_LIBRARY_ROOT` | Resolved canonical paths | Fails startup: `"database must be outside the media directory"` | `crates/core/src/identity.rs:198` |
| 16 | Security | Host & Origin Validation Middleware | Enforces that incoming `Host` and `Origin` headers match configured `public_origin` / `authority`. | HTTP request headers | Passed to inner router | Returns `403 Forbidden` on mismatch or `sec-fetch-site: cross-site` | `crates/core/src/lib.rs:175-188` |
| 17 | Security | Content Security Policy & Security Headers | Imposes strict CSP, `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`, `frame-ancestors 'none'`. | Outgoing HTTP responses | Injected HTTP security headers | Browser blocks unauthorized inline scripts/frames | `crates/core/src/lib.rs:189-193` |
| 18 | Security | Session Cookie Hardening | `media_session` cookie configured with `HttpOnly`, `SameSite=Strict`, `Path=/`, and conditional `Secure`. | User authentication | Set-Cookie response header | Browser blocks script access and cross-site transmission | `crates/core/src/auth.rs`, `docs/LOCAL_SESSIONS.md` |
| 19 | Security | Synchronizer CSRF Token Defense | State-changing API endpoints require valid `x-csrf-token` derived from user session. | `x-csrf-token` header + session | Execution of mutation | Returns `403 Forbidden` if missing or invalid | `crates/core/src/auth.rs:105`, `scans.rs:95` |
| 20 | Security | Zero Hardcoded Admin / Default Secrets | No default admin login or password in code, Dockerfile, or Compose configuration. | Environment / CLI | Clean initial database | Requires explicit bootstrap execution before login | `AGENTS.md` § 3, `ORIGINAL_REQUEST.md` § R2 |
| 21 | Operations | Interactive CLI Admin Bootstrap | First administrator created securely via interactive CLI command in running container. | Interactive TTY (`-it`), Login, Password, Confirm Password | Inserted administrator record and singleton lock | Fails with exit 1 if no TTY, args present, or already initialized | `crates/core/src/bootstrap.rs` |
| 22 | Operations | Container Healthcheck Endpoint | Exposes `/api/v1/health` reporting service readiness without leaking sensitive internals. | HTTP GET to `/api/v1/health` with valid Host header | `200 OK` with JSON `{"status":"ok", "mode":mode}` | Docker marks container unhealthy on failure | `crates/core/src/lib.rs:164`, `ORIGINAL_REQUEST.md` § R3 |
| 23 | Operations | Automatic SQLite Migrations | Migrations 0001–0006 executed automatically within an atomic transaction at startup. | SQLite connection on startup | Schema up-to-date in persistent volume | Startup aborts if migration fails | `crates/core/src/identity.rs:95` |
| 24 | Operations | Interrupted Scan Auto-Recovery | Any scan job left in `queued` or `running` state is automatically marked `error` on startup. | Database query in `scans::router()` | Database updated; status becomes `error` | No scan jobs remain permanently stuck in running state | `crates/core/src/scans.rs:80` |
| 25 | Operations | Graceful Scan Task Shutdown | `ScanJobs::shutdown()` sets cancellation atomic flag, closes semaphore, and awaits tasks on SIGINT/SIGTERM. | OS signal or server shutdown | Cancellation propagated to active scanner loop | Child ffprobe killed via `kill_on_drop(true)` | `crates/core/src/scans.rs:44`, `media.rs:66` |
| 26 | Deployment | Host Loopback Binding Default | Docker Compose binds host port `127.0.0.1:3000:3000` by default to prevent accidental LAN exposure. | Docker Compose port specification | Accessible only on host loopback | Not accessible externally until operator explicitly reconfigures | `ORIGINAL_REQUEST.md` § R4 |
| 27 | Deployment | Synology DS1522+ Compatibility | AMD Ryzen R1600 x86_64 target, no iGPU hardware acceleration configured, low RAM/CPU footprint. | Docker Compose on Synology Container Manager | Running container on DSM | Fails to start if invalid hardware devices (e.g. `/dev/dri`) are passed | `ORIGINAL_REQUEST.md` § R4, `MEMORY.md` § 4 |
| 28 | Deployment | Upgrade Path Safety | Image update preserves SQLite volume; startup runs incremental migrations without data loss. | Newer image version mounted to existing `/data` | Seamless migration to latest schema | Unaltered existing users, sessions, and catalog | `ORIGINAL_REQUEST.md` § R3 |

---

## 3. Edge Cases & Boundary Conditions

| # | Feature | Input / Condition | Observed / Documented Behavior |
|---|---------|-------------------|--------------------------------|
| 1 | Admin Bootstrap | Non-interactive execution (e.g., piped input or `docker exec` without `-it`) | `bootstrap.rs:17` checks `!io::stdin().is_terminal() \|\| !io::stdout().is_terminal()`. Errors immediately: `"bootstrap requires a local interactive terminal and accepts no arguments"`. Exit code 1. |
| 2 | Admin Bootstrap | CLI arguments supplied (e.g., `media-bootstrap --admin foo`) | `bootstrap.rs:17` checks `std::env::args_os().len() != 1`. Rejects all arguments to prevent credentials from leaking into process tables or logs. |
| 3 | Admin Bootstrap | Secondary invocation after initial bootstrap | `identity.rs:150` checks `bootstrap_state` singleton table. Rejects with `IdentityError::AlreadyInitialized`. |
| 4 | Admin Bootstrap | Password confirmation mismatch | Prompts for confirmation; if `*password != *confirmation`, rejects with `"password confirmation does not match"`. Memory wiped via `Zeroizing`. |
| 5 | Startup / Config | Missing parent directory for `MEDIA_DB_PATH` | `identity.rs:185` calls `parent.canonicalize()?`. Fails with `std::io::Error (NotFound)` if directory does not exist on disk prior to startup. |
| 6 | Startup / Config | `MEDIA_DB_PATH` points inside `MEDIA_LIBRARY_ROOT` | `identity.rs:196` checks `resolved.starts_with(root)`. Rejects startup with `"database must be outside the media directory"`. |
| 7 | Startup / Config | `MEDIA_LIBRARY_ROOT` does not exist | `node.rs:137` calls `tokio::fs::canonicalize(allowed_root).await?`. Startup aborts if path does not exist on filesystem. |
| 8 | Startup / Config | `MEDIA_LIBRARY_ROOT` contains relative components (`..`) | `node.rs:127` checks `root.components().any(|part| part == Component::ParentDir)`. Aborts with `InvalidInput: "library roots must be absolute without parent components"`. |
| 9 | Network / Host | Healthcheck without matching `Host` header | `lib.rs:180` checks `req.headers().get(HOST) == host`. Returns `403 Forbidden`. Docker healthcheck command must pass matching `Host` header if `MEDIA_PUBLIC_URL` differs. |
| 10 | Network / Host | Access from reverse proxy with mismatched `Host` | Reverse proxy must be configured to pass original `Host` (e.g. `Host: $http_host`). If forwarded host doesn't match `MEDIA_PUBLIC_URL`, server returns `403 Forbidden`. |
| 11 | Network / CSRF | Cross-site request or login CSRF | `lib.rs:181` checks `sec-fetch-site == "cross-site"` and `header::ORIGIN != origin`. Requests are immediately rejected with `403 Forbidden`. |
| 12 | Database / SQLite | Concurrent startup race on PRAGMA journal_mode=WAL | `identity.rs:79-88` implements retry loop for SQLite errors 5 (`SQLITE_BUSY`) and 6 (`SQLITE_LOCKED`) with exponential backoff (up to 2 retries, 15s timeout). |
| 13 | Operations / Restart | Container stopped while library scan was running | On subsequent startup, `scans::router()` runs: `UPDATE scan_jobs SET status='error', errors_json='["Scan interrompu par un redémarrage"]' WHERE status IN ('queued','running')`. |
| 14 | Operations / Shutdown | SIGTERM sent to container by Docker daemon | In Linux containers without tini or native SIGTERM handling, PID 1 ignores SIGTERM. With tini or dual SIGINT/SIGTERM listener, Axum triggers `shutdown_jobs.shutdown().await`. |
| 15 | Media Scanner | `ffprobe` unavailable in container `$PATH` | If `ffprobe` is missing, `Command::new("ffprobe").spawn()` returns `NotFound`. `scanner.rs:373` catches IO error and aborts publication so inventory is not wiped. |
| 16 | Media Scanner | Media file corrupt or unreadable | `ffprobe` fails or times out (10s). Error is recorded in scan error list; scanner continues with remaining files without blocking. |
| 17 | Media Streaming | HTTP Range request beyond file bounds | Streaming handler returns `416 Range Not Satisfiable` with `content-range: bytes */<length>`. Valid ranges return `206 Partial Content`. |
| 18 | Synology Hardware | Hardware transcoding / GPU passthrough requested | AMD Ryzen R1600 lacks iGPU. Specifying `/dev/dri` in Compose causes container startup failure because device node does not exist on host. |

---

## 4. R1: All-in-One Multi-Stage Container & `.dockerignore`

### 4.1 Multi-Stage Build Architecture
The build requires three distinct stages in a single `Dockerfile` to produce an image of minimal size (< 150 MB) containing zero build tools:

```
+----------------------------------------------------------------------------------+
| Stage 1: Frontend Builder (node:22-bookworm-slim or node:22-alpine)              |
| - Copies apps/web/package*.json                                                  |
| - Runs `npm ci`                                                                  |
| - Copies apps/web/ sources                                                       |
| - Runs `npm run build` (vite build + scripts/externalize-bootstrap.mjs)          |
| - Output: apps/web/build/                                                        |
+----------------------------------------------------------------------------------+
                                        | (Artifacts: static web assets)
                                        v
+----------------------------------------------------------------------------------+
| Stage 2: Rust Cargo Builder (rust:1-slim-bookworm)                               |
| - Installs pkg-config, libssl-dev, etc.                                          |
| - Copies Cargo.toml, Cargo.lock, crates/                                         |
| - Runs `cargo build --release --locked --bin media-poc --bin media-bootstrap`   |
| - Strips binaries (`strip target/release/media-poc media-bootstrap`)             |
| - Output: target/release/media-poc, target/release/media-bootstrap               |
+----------------------------------------------------------------------------------+
                                        | (Artifacts: compiled binaries)
                                        v
+----------------------------------------------------------------------------------+
| Stage 3: Minimal Linux x86_64 Runtime (debian:bookworm-slim)                     |
| - Installs runtime packages: ffmpeg (for ffprobe), ca-certificates, tini         |
| - Creates unprivileged non-root user/group (UID 10001 / GID 10001)               |
| - Creates directories: /data, /cache, /media, /app                               |
| - Copies binaries to /usr/local/bin/                                             |
| - Copies frontend assets to /app/apps/web/build/                                 |
| - Sets WORKDIR /app, USER 10001:10001                                            |
| - ENTRYPOINT ["/usr/bin/tini", "--"]                                             |
| - CMD ["media-poc"]                                                              |
+----------------------------------------------------------------------------------+
```

### 4.2 Non-Root User Specification
- **UID / GID:** `10001:10001` (named `aether`). Avoids conflicting with typical host UID `1000` or DSM admin `1026`.
- **Directory Ownership:**
  - `/app`: Owned by `root:root`, read-only to `aether` (`0755`).
  - `/data`: Owned by `10001:10001`, permissions `0700` or `0750` (writable for SQLite database and WAL/SHM files).
  - `/cache`: Owned by `10001:10001`, permissions `0750`.
  - `/media`: Mount target for media volumes. Read permission required (`0555` or `0755` on host).

### 4.3 Signal Handling & Graceful Shutdown
- **Problem Statement:** In a Docker container, the primary process runs as PID 1. By default, Linux kernels do not apply default signal actions (like termination) to PID 1. Furthermore, `docker stop` sends `SIGTERM` followed by `SIGKILL` after 10 seconds. In standard Rust Tokio applications, `tokio::signal::ctrl_c()` listens specifically for `SIGINT`, not `SIGTERM`.
- **Specification:**
  1. Use `tini` as container `ENTRYPOINT` (`/usr/bin/tini --`). `tini` acts as a proper PID 1, reaps zombie child processes (important since `ffprobe` subprocesses are spawned repeatedly), and translates/forwards signals.
  2. The server binary graceful shutdown routine must respond to both `SIGINT` (Ctrl+C) and `SIGTERM` (Docker stop) signals, ensuring `ScanJobs::shutdown()` is invoked before process exit.

### 4.4 Process Model Invariants
- **No Permanent Node.js Process:** Node.js exists strictly in Stage 1. The runtime container does NOT have Node.js, npm, or any JS runtime installed. Axum's `tower_http::services::ServeDir` serves all pre-compiled assets.
- **Single Server Process:** The runtime consists of exactly one persistent process (`media-poc`). It runs Tokio multi-threaded runtime, Axum HTTP listener, SQLite connection pool, and the internal Media Node scanner/streamer. No secondary server, relay daemon, or background worker process.
- **`ffprobe` Availability:** Provided by the Debian `ffmpeg` package. Verified available at `/usr/bin/ffprobe` (in standard `$PATH`).

### 4.5 `.dockerignore` Complete Pattern Inventory
The `.dockerignore` file must exclude all host development artifacts, large binaries, media files, and sensitive data to optimize build context transfer:

```dockerignore
# Version Control & Metadata
.git
.gitignore
.gitattributes
.github
.agents
.gemini

# Build Artifacts & Caches
target/
**/target/
node_modules/
**/node_modules/
apps/web/.svelte-kit/
apps/web/build/
dist/

# Test Artifacts, Fixtures & Large Media
tests/fixtures/generated/
tests/fixtures/vendor/
*.mkv
*.mp4
*.webm
*.vtt
docs/evidence/
artifacts/

# Database Files & Persistent State
data/
*.sqlite3
*.sqlite3-wal
*.sqlite3-shm
*.db
*.db-wal
*.db-shm

# Development / OS / Temp Files
*.log
*.jsonl
*.tmp
*.swp
*~
.DS_Store
Thumbs.db
.vscode/
.idea/
tmp/
```

---

## 5. R2: Startup Configuration, Network Binding & Security Hardening

### 5.1 Validated `MEDIA_*` Environment Variables

| Variable | Type | Default Value | Required | Purpose & Constraints |
|---|---|---|---|---|
| `MEDIA_PORT` | `u16` | `3000` | No | Internal port Axum binds to inside the container. Must be a valid 16-bit integer (1..65535). |
| `MEDIA_PUBLIC_URL` | String | `http://127.0.0.1:3000` | No | Exact public origin used to validate `Host` and `Origin` headers, and set session cookie `Secure` attribute if `https://`. Must NOT have trailing slash. |
| `MEDIA_DB_PATH` | Path | `data/core.sqlite3` | No | Path to SQLite database file. In container, recommended `/data/core.sqlite3`. Parent directory must exist; must NOT reside inside `MEDIA_LIBRARY_ROOT`. |
| `MEDIA_LIBRARY_ROOT` | Path | Unset (uses fixtures) | **Yes (Prod)** | Absolute root directory containing media libraries (e.g. `/media`). Must be an existing directory without `..` components. Disables fixture bench mode and enables real scanner. |
| `MEDIA_BIND_ADDR` / Network Binding | String | `0.0.0.0` (in Docker) | No | Interface IP for Axum listener inside container. `0.0.0.0` required inside Docker to accept bridged traffic from Docker host port forwarder. |

### 5.2 Network Binding Inside Docker vs Host Exposure
1. **Container Internal Interface:** Must listen on `0.0.0.0:<port>` inside Docker network namespace so container port forwarding succeeds.
2. **Host Port Binding:** By default in Docker Compose, the port mapping MUST be explicitly bound to the host loopback:
   ```yaml
   ports:
     - "127.0.0.1:3000:3000"
   ```
   This prevents unauthorized access from local area network (LAN) devices or WAN before an administrator has configured access controls.
3. **LAN or Reverse-Proxy Exposure Workflow:**
   - To expose on host LAN: Administrator changes Compose binding to `3000:3000` (or `"0.0.0.0:3000:3000"`).
   - `MEDIA_PUBLIC_URL` must be set to the exact origin used by clients (e.g., `http://192.168.1.100:3000` or `https://aether.myhome.net`).
   - If behind a reverse proxy (e.g., Nginx, Caddy, Synology Reverse Proxy):
     - Proxy passes request with `Host: $http_host` (or configured host).
     - Proxy terminates TLS; Core sees incoming HTTP with preserved Host matching `MEDIA_PUBLIC_URL`.

### 5.3 Security Headers & Origin Controls
- **Host Header Validation:** Strict equality check `req.headers().get(HOST) == configured_host`.
- **Origin Validation:** If present, `req.headers().get(ORIGIN) == configured_origin`.
- **Cross-Site Defense:** Requests containing `sec-fetch-site: cross-site` are denied (`403 Forbidden`).
- **CSP Directive:** Strict content security policy:
  `default-src 'none'; script-src 'self'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-...'; frame-ancestors 'none'`
- **Cookie Security:** Cookie `media_session` flags: `HttpOnly`, `SameSite=Strict`, `Path=/`, `Secure` (when `MEDIA_PUBLIC_URL` starts with `https://`).

### 5.4 TOCTOU Hardening Status Transparency
- **Current Architecture:** File access verifies that canonicalized paths reside within authorized library roots before opening.
- **Residual Risk:** Canonicalization followed by open is non-atomic against a hostile local OS actor who can alter symlinks/junctions in the directory.
- **Operational Requirement:** Storage must be controlled by the trusted operator via proper OS permissions/ACLs. This limitation is explicitly documented and NOT falsely claimed as resolved.

---

## 6. R3: Secure Administrator Bootstrap, Restart Resilience & Healthcheck

### 6.1 Administrator Bootstrap Workflow
Bootstrap creates the first user with role `administrator`. It uses a dedicated CLI binary (`media-bootstrap`) rather than an insecure HTTP setup endpoint.

#### Docker Execution Command:
```bash
docker compose exec -it aether media-bootstrap
```

#### Security & Operational Constraints:
1. **Interactive TTY Required:** The binary enforces `io::stdin().is_terminal() && io::stdout().is_terminal()`. Commands run without `-it` or via automated non-interactive pipes fail immediately with exit code 1.
2. **Zero CLI Arguments:** Enforces `std::env::args_os().len() == 1`. Passing credentials via command line flags (e.g. `--password`) is rejected to prevent exposure in `docker ps`, `ps aux`, shell history, and container logs.
3. **Argon2id Hash Parameters:**
   - Memory cost: `65536 KiB` (64 MiB)
   - Time iterations: `t=3`
   - Parallelism: `p=1`
   - Salt: 16 cryptographically secure random bytes from OS CSPRNG.
   - Hash representation: PHC string stored in `local_credentials.password_hash`.
4. **Memory Hygiene:** Clear-text passwords and confirmation strings are wrapped in `zeroize::Zeroizing<String>` to scrub memory upon drop.
5. **Singleton Lock:** Executed within an atomic transaction (`BEGIN IMMEDIATE`). Checks `bootstrap_state`. Once initialized, subsequent invocations fail with `IdentityError::AlreadyInitialized`.

### 6.2 Container Restart Resilience & Migrations
1. **SQLite Storage Integrity:**
   - WAL journal mode (`PRAGMA journal_mode = WAL`) enables concurrent readers alongside writers.
   - Busy timeout set to 15 seconds (`PRAGMA busy_timeout = 15000`).
   - SQLite files stored in dedicated persistent volume `/data`.
2. **Automatic Migrations:**
   - Embedded into binary via `sqlx::migrate!("./migrations")`.
   - Executed inside an immediate transaction on every startup.
   - Safe and idempotent across container restarts.
3. **Interrupted Scan Recovery:**
   - On startup, `scans::router()` automatically executes:
     `UPDATE scan_jobs SET status='error', errors_json='["Scan interrompu par un redémarrage"]' WHERE status IN ('queued','running')`
   - Prevents stale lockups without deleting catalog entries discovered prior to interruption.

### 6.3 Container Healthcheck Specification
- **Endpoint:** `/api/v1/health`
- **Response Format:** HTTP `200 OK`, `Content-Type: application/json`
  `{"status":"ok", "mode":"standalone-local"}`
- **Security Middleware Consideration:** Because `/api/v1/health` is wrapped in `local_security`, healthcheck requests must include a `Host` header matching the authority of `MEDIA_PUBLIC_URL` (default `127.0.0.1:3000`).
- **Docker Healthcheck Definition in Compose:**
  ```yaml
  healthcheck:
    test: ["CMD-SHELL", "curl -f -H 'Host: 127.0.0.1:3000' http://127.0.0.1:3000/api/v1/health || exit 1"]
    interval: 30s
    timeout: 5s
    retries: 3
    start_period: 10s
  ```

---

## 7. R4: Synology DS1522+ Docker Compose & Operational Safety

### 7.1 Synology DS1522+ Hardware Profile & Constraints
- **Processor:** AMD Ryzen Embedded R1600 (x86_64, Zen 1 microarchitecture, 2 cores / 4 threads, 2.6 GHz base / 3.1 GHz boost).
- **GPU:** **None.** (No integrated GPU, no AMD VCN/VCE, no Intel QuickSync).
- **Implication for Transcoding:** Transcoding 4K/1080p AV1 in real time on this CPU is completely infeasible. Aether's architectural requirement of **no mandatory transcoding** (direct stream of AV1+Opus or audio remux only) is essential for this target.
- **Compose Device Directives:** Must NOT include `devices: ["/dev/dri:/dev/dri"]`. Passing `/dev/dri` on DS1522+ will cause container startup to fail because the hardware node does not exist on DSM.

### 7.2 Docker Compose Configuration (`compose.yaml`)

```yaml
services:
  aether:
    image: aether:latest
    container_name: aether
    restart: unless-stopped
    user: "10001:10001"
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    ports:
      # Default: bound strictly to host loopback interface.
      # To expose on LAN or reverse proxy, configure host IP and MEDIA_PUBLIC_URL accordingly.
      - "127.0.0.1:3000:3000"
    environment:
      - MEDIA_PORT=3000
      - MEDIA_PUBLIC_URL=http://127.0.0.1:3000
      - MEDIA_DB_PATH=/data/core.sqlite3
      - MEDIA_LIBRARY_ROOT=/media
    volumes:
      # Persistent application database volume
      - aether_data:/data
      # Read-only media library mounts (e.g. Synology shared folders)
      - /volume1/video/Films:/media/films:ro
    healthcheck:
      test: ["CMD-SHELL", "curl -f -H 'Host: 127.0.0.1:3000' http://127.0.0.1:3000/api/v1/health || exit 1"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s

volumes:
  aether_data:
    name: aether_data
```

### 7.3 Operational Safety Rules (Strict Non-Negotiables)
1. **Never Push to Git or Publish Docker Images:** No `docker push`, no git pushes to remote origins.
2. **Never Deploy to Remote NAS:** No automated SSH/SCP/API deployment to user's physical Synology NAS. Deliver configurations and instructions locally.
3. **Never Modify Media Files:** Media mounts are strictly `:ro`. No tagging, no renaming, no sidecar file generation in media directories.

---

## 8. Acceptance Criteria Checklist & Verification Test Plan Mapping

This section maps every requirement from `ORIGINAL_REQUEST.md` to a verifiable test case, execution command, and expected output.

| Requirement ID | Acceptance Criterion | Verification Method / Command | Expected Observable Output | Failure Invalidation Condition |
|---|---|---|---|---|
| **AC-1** | Rust workspace passes format, clippy, and unit tests without regressions. | `cargo fmt --check`<br>`cargo clippy --all-targets --locked --all-features -- -D warnings`<br>`cargo test --locked --all-features` | Clean exit (code 0). 50 tests pass, 5 ignored (browser/subprocess). | Any formatting diff, clippy warning, or failing test. |
| **AC-2** | Frontend TypeScript check, linter, tests, and static build pass. | `cd apps/web`<br>`npm run check`<br>`npm run lint`<br>`npm test`<br>`npm run build` | 0 errors, 0 warnings. Vitest passes. Static build succeeds with externalized bootstrap scripts. | Type error, lint failure, Vitest failure, or missing bootstrap JS. |
| **AC-3** | `.dockerignore` excludes git, targets, node_modules, fixtures, evidence, and state. | Inspect `.dockerignore` patterns; execute context size check: `docker build --no-cache -t aether .` | Build context transfer is minimal (< 50 MB); excluded directories are not sent. | Presence of `.git`, `target`, `node_modules`, or `docs/evidence` in build context. |
| **AC-4** | Docker build produces single `aether` image running as non-root with `ffprobe` in PATH and no permanent Node.js. | `docker build -t aether .`<br>`docker run --rm aether whoami`<br>`docker run --rm aether which ffprobe`<br>`docker run --rm aether which node` | Non-root UID output (`10001` or `aether`).<br>`/usr/bin/ffprobe` found.<br>`node: not found` (exit code 1). | Root execution, missing `ffprobe`, or persistent Node.js in runtime. |
| **AC-5** | Service startup in Compose initializes SQLite migrations and reports healthy status. | `docker compose up -d`<br>`docker compose ps` | Status indicates `healthy`. SQLite database created in `/data` with migrations applied. | Status `unhealthy`, container crash, or unmigrated DB. |
| **AC-6** | Anonymous requests to protected endpoints return 401/403; Host/Origin/CSRF controls remain active. | `curl -i -H 'Host: 127.0.0.1:3000' http://127.0.0.1:3000/api/v1/catalog`<br>`curl -i -H 'Host: evil.com' http://127.0.0.1:3000/api/v1/health` | `HTTP/1.1 401 Unauthorized`<br>`HTTP/1.1 403 Forbidden` | Any bypass of auth or Host/Origin validation. |
| **AC-7** | Documented Docker bootstrap command creates first administrator; subsequent login succeeds. | `docker compose exec -it aether media-bootstrap`<br>`curl -i -X POST http://127.0.0.1:3000/api/v1/auth/login ...` | Prompt succeeds; admin record inserted in SQLite; login returns `200 OK` + session cookie. | Bootstrap fails, accepts non-TTY, or login fails. |
| **AC-8** | Read-only synthetic library scan succeeds via `ffprobe`, populates catalog, does not write to `:ro` mount. | Trigger scan via admin API POST `/api/v1/admin/libraries/{id}/scan`; poll status; compare media directory SHA256 before/after. | Scan completes with probed count > 0; catalog populated; media directory SHA256 hashes 100% identical. | Scan failure, or any modification/write to media files. |
| **AC-9** | MP4 playback and HTTP Range seek requests (206, 416) succeed against running container. | `curl -i -H 'Host: 127.0.0.1:3000' -H 'Range: bytes=0-1023' -H "Cookie: $COOKIE" .../content`<br>`curl -i -H 'Host: 127.0.0.1:3000' -H 'Range: bytes=99999999-99999999' .../content` | Returns `206 Partial Content` with `Content-Range: bytes 0-1023/...`.<br>Returns `416 Range Not Satisfiable`. | Returns `200 OK` (ignoring Range), returns incorrect chunk, or crashes. |
| **AC-10** | Container stop and restart preserves users, sessions, catalog, and clears interrupted scan state. | Start scan, immediately run `docker compose restart`, then query `/api/v1/auth/me` and scan status. | Session still valid (200 OK); catalog intact; interrupted scan marked `status: "error"`. | Lost accounts/sessions, corrupted DB, or scan permanently stuck in `running`. |
| **AC-11** | Transparent reporting of environment metrics and blockers without fabricated data. | Inspect Docker performance stats (`docker stats --no-stream`) or record host blockers transparently. | Real measured image size, RSS, startup time recorded; host limitations stated honestly. | Fabricated metrics or undisclosed failures. |

---

## 9. Conclusion & Implementation Checklist for Downstream Agents

Downstream implementation agents should execute the deployment in accordance with this specification:
1. **Create `.dockerignore`**: Enforce complete pattern inventory (§ 4.5).
2. **Create multi-stage `Dockerfile`**:
   - Stage 1: `apps/web` build (`npm ci && npm run build`).
   - Stage 2: Rust release build (`cargo build --release --locked --bin media-poc --bin media-bootstrap`).
   - Stage 3: `debian:bookworm-slim` with `ffmpeg`, `ca-certificates`, `tini`, non-root user `10001:10001`.
   - Copy web assets to `/app/apps/web/build` and binaries to `/usr/local/bin/`.
3. **Verify Signal Handling**: Ensure `main.rs` listens for both `SIGINT` and `SIGTERM` before terminating so graceful shutdown is honored.
4. **Create `compose.yaml`**: Pre-configured with `:ro` media volume, persistent data volume, loopback port binding `127.0.0.1:3000:3000`, healthcheck with matching Host header, and unprivileged user configuration.
5. **Update Documentation**: Update `README.md`, `MEMORY.md`, and `docs/DEPLOYMENT.md` with exact bootstrap commands, operational procedures, and real verification evidence.
