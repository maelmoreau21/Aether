# Handoff Report — Docker & Deployment Specification Mining

**Agent:** `spec_miner_survey_3`  
**Parent / Caller ID:** `e84c8bab-f4f1-4a93-8220-b8cbda41be06`  
**Handoff Type:** Hard (Task Complete)  
**Date:** 2026-10-16  

---

## 1. Observation

Direct observations from source code, project documentation, manifests, and system configurations:

1. **Bootstrap Binary Invariants (`crates/core/src/bootstrap.rs:17-21`):**
   ```rust
   if std::env::args_os().len() != 1 || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
       return Err(
           "bootstrap requires a local interactive terminal and accepts no arguments".into(),
       );
   }
   ```
   Verbatim check requires `is_terminal()` on both stdin and stdout and accepts zero arguments. Executing without an interactive TTY (e.g. `docker exec` without `-it`) returns exit code 1.
2. **Database Path & Media Root Separation (`crates/core/src/identity.rs:185, 195-201`):**
   ```rust
   let parent = parent.canonicalize()?;
   ...
   if let Ok(root) = media_root.canonicalize()
       && resolved.starts_with(root)
   {
       return Err(std::io::Error::other(
           "database must be outside the media directory",
       ));
   }
   ```
   The parent directory of `MEDIA_DB_PATH` must exist before process startup (or `canonicalize()` fails), and the resolved database file must NOT be inside `MEDIA_LIBRARY_ROOT`.
3. **Scan Recovery & Job Invariants (`crates/core/src/scans.rs:44-55, 79-80`):**
   ```rust
   // A stopped process cannot resume a probe. Keep its last inventory and report interruption.
   sqlx::query("UPDATE scan_jobs SET status='error',errors_json='[\"Scan interrompu par un redémarrage\"]' WHERE status IN ('queued','running')").execute(&auth.store.pool).await?;
   ```
   At startup, any incomplete jobs left in `queued` or `running` state are marked as `error`. `ScanJobs::shutdown()` closes the task semaphore and sets the atomic cancellation flag.
4. **External `ffprobe` Invocation (`crates/core/src/media.rs:42-67`):**
   ```rust
   let mut child = Command::new("ffprobe")
       .args([
           "-v", "error", "-protocol_whitelist", "file", "-format_whitelist", "matroska,webm,mov",
           "-max_alloc", "67108864", "-probesize", "5000000", "-analyzeduration", "5000000",
           "-show_streams", "-show_format", "-show_data", "-of", "json",
       ])
       .kill_on_drop(true)
       .spawn()?;
   ```
   The scanner requires `ffprobe` in standard `$PATH`.
5. **Static Frontend Serving & CSP (`crates/core/src/lib.rs:152-156` & `apps/web/scripts/externalize-bootstrap.mjs`):**
   ```rust
   .nest_service(
       "/app",
       tower_http::services::ServeDir::new("apps/web/build")
           .append_index_html_on_directories(true),
   )
   ```
   Axum serves pre-compiled static assets from `apps/web/build`. In `externalize-bootstrap.mjs`, all inline `<script>` tags are extracted to external JS files so that Axum's strict CSP (`script-src 'self'`) succeeds without `'unsafe-inline'`.
6. **Host & Origin Security Controls (`crates/core/src/lib.rs:180-185`):**
   ```rust
   if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != host.as_deref()
       || req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site")
       || req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != origin.as_bytes())
   {
       return StatusCode::FORBIDDEN.into_response();
   }
   ```
   All requests (including `/api/v1/health`) are evaluated against `host` and `origin` derived from `MEDIA_PUBLIC_URL`.
7. **Synology DS1522+ Platform Target (`MEMORY.md:100` & `ORIGINAL_REQUEST.md:32`):**
   AMD Ryzen R1600 (x86_64, Zen 1 microarchitecture, 2 cores / 4 threads, no iGPU). Device node `/dev/dri` does not exist on this NAS.
8. **Repository State:**
   No `Dockerfile` or `compose.yaml` exists currently in the repository root.

---

## 2. Logic Chain

1. **Multi-Stage Build Rationale:**
   - From Obs 5, frontend requires Node.js, npm, and `scripts/externalize-bootstrap.mjs` to build, but once built, Axum serves static files directly.
   - From Obs 4, runtime requires `ffprobe` from ffmpeg, but does NOT require Node.js or Rust build tools.
   - Therefore, a 3-stage Dockerfile (Stage 1: Node.js for SvelteKit static build, Stage 2: Rust for cargo release build, Stage 3: minimal Debian slim runtime with ffmpeg and tini) achieves the smallest attack surface, minimal image size, and eliminates Node.js from runtime.
2. **Process Model & Signal Handling:**
   - From Obs 3 and `AGENTS.md` § 3, standalone mode runs both Core and internal Media Node in a single process (`media-poc`).
   - From Obs 1 and Linux PID 1 behavior, signals sent by `docker stop` (`SIGTERM`) are ignored by PID 1 unless handled or reaped by an init system. `tini` must be the entrypoint, and the server listener should handle both `SIGTERM` and `SIGINT` to trigger `shutdown_jobs.shutdown().await`.
3. **Volume Mounts & Security Invariants:**
   - From Obs 2 and `AGENTS.md` § 3, `MEDIA_DB_PATH` parent directory must exist and must not be inside `MEDIA_LIBRARY_ROOT`. Thus `/data` must be a dedicated writable volume (`aether_data:/data`).
   - Media libraries must be mounted strictly read-only (`/volume1/video/Films:/media/films:ro`), guaranteeing that Aether cannot modify existing media or interfere with Jellyfin.
4. **Network & Healthcheck Configuration:**
   - From Obs 6, any incoming request with mismatched `Host` returns `403 Forbidden`.
   - Therefore, Docker healthcheck hitting `http://127.0.0.1:3000/api/v1/health` must include the header `-H "Host: 127.0.0.1:3000"` (or the configured `MEDIA_PUBLIC_URL` host).
   - In Compose, host port mapping defaults to `"127.0.0.1:3000:3000"`, preventing accidental LAN exposure without explicit admin intent.
5. **Operational Workflows:**
   - From Obs 1, admin bootstrap cannot be run headless. The documented operator workflow is `docker compose exec -it aether media-bootstrap`.
   - From Obs 3, container crashes or restarts gracefully recover scan state by setting interrupted jobs to `error` upon reboot, leaving existing catalog works intact.

---

## 3. Caveats

1. **TOCTOU File Open Limitation:** Symlink/junction confinement checks prior to file opening in `node/scanner.rs` and `node/images.rs` are non-atomic. OS-level operator permissions on the host media mounts are required against hostile local writers.
2. **Host Port Binding on Synology DSM:** Synology DSM Container Manager bridges ports. While binding to `127.0.0.1:3000:3000` protects host loopback on standard Linux, DSM users accessing via NAS IP need either direct host port binding (e.g. `3000:3000`) with appropriate `MEDIA_PUBLIC_URL` or a reverse proxy. Both options are documented in the specification.
3. **No GPU Acceleration:** Because the AMD Ryzen R1600 lacks an iGPU, any attempt to pass `/dev/dri` will cause container failure. No GPU configurations should be added.

---

## 4. Conclusion

All explicit and implicit requirements for Aether's all-in-one Docker standalone distribution (R1–R4) have been mined, analyzed, and synthesized into a comprehensive specification document at:
`c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\spec_miner_survey_3\spec_requirements.md`

The specification defines:
- The 3-stage `Dockerfile` architecture (Node SvelteKit build -> Cargo release build -> Debian slim x86_64 runtime with `ffprobe` and `tini`).
- The complete `.dockerignore` pattern inventory.
- The `compose.yaml` specification for local standalone and Synology DS1522+ deployments (unprivileged UID `10001:10001`, `:ro` media mounts, `/data` persistent volume, loopback port binding, and healthcheck).
- Operational workflows: interactive TTY bootstrap command, container restart resilience, scan interruption recovery, and forward upgrade migrations.
- Complete mapping of acceptance criteria AC-1 to AC-11.

---

## 5. Verification Method

To independently verify the facts and constraints reported:

1. **Verify Codebase Invariants:**
   - Inspect `crates/core/src/bootstrap.rs` lines 17–21 for TTY and argument checks.
   - Inspect `crates/core/src/identity.rs` lines 177–203 for `MEDIA_DB_PATH` canonicalization and parent/media isolation checks.
   - Inspect `crates/core/src/scans.rs` lines 78–84 for startup recovery of interrupted scan jobs.
   - Inspect `crates/core/src/media.rs` lines 42–67 for `ffprobe` command execution.
   - Inspect `crates/core/src/lib.rs` lines 152–193 for static asset serving and `Host`/`Origin` validation.
2. **Execute Current Workspace Verification Commands:**
   ```powershell
   cargo fmt --check
   cargo clippy --locked --all-features --all-targets -- -D warnings
   cargo test --locked --all-features
   ```
   Confirm that all 50 tests pass and 5 are ignored (browser/subprocess).
3. **Verify Frontend Build & Scripts:**
   ```powershell
   cd apps/web
   npm run check
   npm run lint
   npm test
   npm run build
   ```
   Confirm build succeeds and externalized bootstrap scripts are generated.
4. **Invalidation Conditions:**
   - If `media-bootstrap` can be executed non-interactively without error, Observation 1 is invalidated.
   - If `IdentityStore::open` permits SQLite files inside `MEDIA_LIBRARY_ROOT`, Observation 2 is invalidated.
   - If `scans::router` leaves `running` scan jobs intact after process reboot, Observation 3 is invalidated.
