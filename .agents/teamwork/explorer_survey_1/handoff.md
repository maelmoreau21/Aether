# Handoff Report: Codebase Architecture Survey (Explorer Survey 1)

**Agent**: Codebase Architecture Explorer (`explorer_survey_1`)  
**Target Recipient**: Orchestrator (`e84c8bab-f4f1-4a93-8220-b8cbda41be06`)  
**Date**: 2026-10-16  
**Artifact**: `c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_survey_1\survey_codebase.md`

---

## 1. Observation

- **Cargo Workspace Structure & Binaries**:
  - `Cargo.toml:1-9` defines `[workspace] members = ["crates/core"]` with edition `2024`, `rust-version = "1.99"`, `resolver = "3"`.
  - `crates/shared` contains `crates/shared/src/brand.rs` (defining `pub const DEFAULT_NAME: &str = "Aether";`). It has no `Cargo.toml` and is referenced via `#[path = "../../shared/src/brand.rs"]` in `crates/core/src/lib.rs:3-4`.
  - `crates/core/Cargo.toml:1-46` sets package name `media-poc`.
  - Binaries produced:
    1. `media-poc` (`crates/core/src/main.rs:7-46`): default daemon.
    2. `media-bootstrap` (`crates/core/src/bootstrap.rs:8-46`, declared in `crates/core/Cargo.toml:32-34`).
  - Standalone mode: `crates/core/src/main.rs:8-45` unifies Axum HTTP server, SQLite `IdentityStore` (`identity.rs:61-99`), local Media Node `Inventory::local` (`node.rs:231-237`), `ScanJobs` runner (`scans.rs:19-56`), and routing (`lib.rs:82-119`) in a single process.
- **HTTP and Static File Serving**:
  - `crates/core/src/lib.rs:150-167`: Axum serves static frontend files under `/app` via `tower_http::services::ServeDir::new("apps/web/build").append_index_html_on_directories(true)`.
  - Content Security Policy in `crates/core/src/lib.rs:191`:
    `default-src 'none'; script-src 'self'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; frame-ancestors 'none'`
  - Network binding in `crates/core/src/main.rs:26`: `TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))`.
- **Frontend Build & Configuration**:
  - `apps/web/package.json:1-28`: `"media-web"`, devDependencies include `@sveltejs/adapter-static: 4.0.0`, `@sveltejs/kit: 3.1.0`, `svelte: 5.57.2`, `vite: 8.3.4`.
  - `apps/web/vite.config.ts:1-5`: Configures `sveltekit({ adapter: adapter(), paths: { base: '/app' }, csp: { mode: 'hash', directives: { 'script-src': ['self'] } } })`.
  - `apps/web/src/routes/+layout.ts:1-3`: `export const prerender = true; export const trailingSlash = 'always';`.
  - `apps/web/package.json:8`: `"build": "vite build && node scripts/externalize-bootstrap.mjs"`.
  - `apps/web/scripts/externalize-bootstrap.mjs:1-17`: Moves inline `<script>` tags from `apps/web/build/**/*.html` to `<filename>-bootstrap-0.js` files to comply with Axum's CSP.
- **ffprobe Discovery & Execution**:
  - `crates/core/src/media.rs:42`: `Command::new("ffprobe")` executes directly from system `PATH`. No environment variables (`FFPROBE_PATH` or `MEDIA_*`) are read.
  - Strict security flags: `-protocol_whitelist file`, `-format_whitelist matroska,webm,mov`, `-max_alloc 67108864`, `-probesize 5000000`, `-analyzeduration 5000000`.
  - Execution timeout: 10 seconds (`media.rs:74`), output capped at 1 MiB (`media.rs:72, 77`).
  - Scanner error handling in `crates/core/src/node/scanner.rs:373-374`: `Some(Err(media::ProbeError::Io(error))) => return Err(error)` immediately halts scan transaction if `ffprobe` is not found, preventing inventory invalidation.
- **Toolchain Verification**:
  - `cargo fmt --check`: Exited with code 0.
  - `cargo clippy --locked --all-features --all-targets -- -D warnings`: Exited with code 0.
  - `cargo test --locked --all-features`: Exited with code 0 (50 passed, 5 ignored).
  - `npm run check` (in `apps/web`): Exited with code 0 (0 errors, 0 warnings).
  - `npm run lint` (in `apps/web`): Exited with code 0 (clean).
  - `npm test` (in `apps/web`): Exited with code 0 (13 passed).
  - `npm run build` (in `apps/web`): Exited with code 0 (static build in `apps/web/build/`).
  - `node scripts/player-comparison-stats.test.cjs; node scripts/safe-margin-stats.test.cjs; node --test scripts/audio-seek-policy.test.cjs`: Exited with code 0 (10 passed).
- **Docker Context Exclusions**:
  - Exclusions required: `.git/`, `.github/`, `.agents/`, `target/`, `node_modules/`, `apps/web/node_modules/`, `apps/web/.svelte-kit/`, `apps/web/build/`, `tests/fixtures/generated/`, `artifacts/`, `docs/evidence/`, `data/`, `*.sqlite3*`, `*.log`, `.DS_Store`, `Thumbs.db`.

---

## 2. Logic Chain

1. **Standalone Capability**:
   - `crates/core/src/main.rs` already unifies Axum HTTP, SQLite persistence, Media Node inventory/scanning, and streaming into `media-poc`.
   - Therefore, no separate Core or Media Node container or second server process is needed.
2. **Static Asset Delivery**:
   - `apps/web` builds into purely static HTML, CSS, and JS files via `@sveltejs/adapter-static` with prerendering.
   - Axum's `public_routes` (`lib.rs:154`) already mounts `tower_http::services::ServeDir::new("apps/web/build")` under `/app`.
   - Therefore, no Node.js runtime process is required in the container.
3. **Container Build Strategy**:
   - A multi-stage Docker build can compile the frontend in a Node stage (`npm ci && npm run build`), compile the Rust binaries (`media-poc`, `media-bootstrap`) in a Cargo stage (`cargo build --release --locked`), and copy both into a minimal Linux runtime image.
4. **ffprobe Requirement**:
   - Because `crates/core/src/media.rs:42` calls `Command::new("ffprobe")` and `scanner.rs:374` aborts if `ffprobe` is not in `PATH`, the runtime container image MUST have `ffprobe` installed and available in `PATH`.
5. **Network Binding & Security Hardening**:
   - `main.rs:26` currently binds to `127.0.0.1`. Inside Docker, this will prevent access from outside the container unless bound to `0.0.0.0`.
   - However, `lib.rs:180-184` and `auth.rs:76-80` validate `Host` and `Origin` against `MEDIA_PUBLIC_URL`. Supporting `0.0.0.0` network binding inside the container must be accompanied by keeping `MEDIA_PUBLIC_URL` validation intact.

---

## 3. Caveats

- **Operating System and Docker Daemon Availability**: Investigation was performed on Windows. Actual Docker daemon presence or version on the host was not tested during this survey step as this was a read-only investigation.
- **ServeDir Working Directory Sensitivity**: `ServeDir::new("apps/web/build")` uses a relative path. In the container, the working directory must be set so that `apps/web/build` exists at that relative location, or the path must be configured appropriately.
- **Terminal Requirement for Bootstrap**: `media-bootstrap` requires interactive standard I/O (`is_terminal()`). Automated headless Docker execution without TTY allocation (`-t` / `tty: true`) will be rejected by design.

---

## 4. Conclusion

The Aether codebase is architecturally prepared for an all-in-one standalone Docker image:
1. `media-poc` already integrates Core and local Media Node in a single Rust process.
2. The frontend is fully static and already served by Axum via `ServeDir` under `/app`.
3. `media-bootstrap` is an independent binary handling initial local administrator setup.
4. All existing unit, integration, and linter suites pass with zero regressions.
5. The container requirements identified will directly inform the Dockerfile, Compose file, and `.dockerignore` creation without modifying core product architectures.

---

## 5. Verification Method

To independently verify these findings:
1. **Rust Test Suite**:
   ```bash
   cargo fmt --check
   cargo clippy --locked --all-features --all-targets -- -D warnings
   cargo test --locked --all-features
   ```
2. **Frontend Check, Test, and Build**:
   ```bash
   cd apps/web
   npm run check
   npm run lint
   npm test
   npm run build
   ```
3. **Script Tests**:
   ```bash
   node scripts/player-comparison-stats.test.cjs
   node scripts/safe-margin-stats.test.cjs
   node --test scripts/audio-seek-policy.test.cjs
   ```
4. **Inspect Key Source Locations**:
   - Rust binaries: `crates/core/Cargo.toml:32-34`, `crates/core/src/main.rs:7-46`, `crates/core/src/bootstrap.rs:8-46`.
   - Static file serving: `crates/core/src/lib.rs:150-167`.
   - SvelteKit adapter: `apps/web/vite.config.ts:1-5`, `apps/web/src/routes/+layout.ts:1-3`.
   - ffprobe invocation: `crates/core/src/media.rs:41-94`, `crates/core/src/node/scanner.rs:348-383`.
