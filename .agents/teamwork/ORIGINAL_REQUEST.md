# Original User Request

## 2026-10-16T12:19:23Z

Deliver the first functional all-in-one Docker standalone distribution of Aether (`aether` image and Docker Compose configuration), packaging the compiled SvelteKit frontend, the Rust/Axum Core, the existing local Media Node, persistent SQLite, and `ffprobe` in a single non-root container without a permanent Node.js runtime, a second Rust server process, or mandatory PostgreSQL.

Working directory: C:/Users/Mael/Documents/GitHub/Aether
Integrity mode: development

## Reference Material & Mandatory Context

Before making changes, inspect the actual codebase, manifests, uncommitted changes, and tests, and read:
- `AGENTS.md`
- `ARCHITECTURE.md`
- `MEMORY.md`
- `docs/SECURITY.md`
- `docs/SPECIFICATION.md`

Do not break existing scanners, the media player, permissions, or SQLite migrations. Do not build separate Core/Node images, PostgreSQL support, OIDC, multi-Node, or the Series scanner, and do not redesign the frontend.

## Requirements

### R1. All-in-One Multi-Stage Container & `.dockerignore`
Provide an optimized multi-stage `Dockerfile` (compiling the SvelteKit web frontend and the Rust workspace, then assembling a minimal x86_64 Linux runtime image with `ffprobe` installed) that produces a single `aether` image serving both the compiled frontend and the backend/local Media Node in one process. The container must run as an unprivileged non-root user, handle signals cleanly, and perform graceful shutdown. Include a `.dockerignore` that excludes generated media fixtures, test evidence, `.git`, build caches, and unnecessary local files.

### R2. Startup Configuration, Network Binding & Security Hardening
Support validated `MEDIA_*` environment variables at startup with strict separation between read-only media library mounts and writable persistent application data directories. Support network binding inside Docker while preserving `Host`, `Origin`, session cookie, and CSRF security controls. Do not include any default administrator credentials or secrets in the image or Compose files, and do not document TOCTOU hardening as resolved if it is not.

### R3. Secure Administrator Bootstrap, Restart Resilience & Healthcheck
Provide a documented Docker workflow/command to perform the initial local administrator bootstrap securely. Ensure reliable startup and restart behavior across container restarts, automatic SQLite migrations, and clean recovery/cleanup of interrupted library scans. Expose and configure a container healthcheck that accurately reflects service readiness without leaking sensitive internal details.

### R4. Synology DS1522+ Docker Compose, Documentation & Operational Safety
Provide a Docker Compose configuration suitable for Synology DS1522+ and local standalone self-hosting, mounting media libraries as `:ro`, persisting writable data/cache in dedicated volumes, requiring no privileged container permissions, and binding to the host loopback interface (`127.0.0.1`) by default (with explicit configuration and validation documented for LAN or reverse-proxy HTTPS exposure). Update `README.md`, `MEMORY.md`, and the deployment documentation with concrete commands and measured results. Never push to Git, publish Docker images, deploy to a remote NAS, or modify user media files.

## Acceptance Criteria

### Code Quality & Existing Test Suites
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass across the Rust workspace without regressions.
- [ ] Frontend strict TypeScript/Svelte checks, build, and existing unit tests pass without regressions.

### Container Build & Static Verification
- [ ] `.dockerignore` excludes `.git`, `target/`, `node_modules/`, generated media fixtures, and test artifacts from the Docker build context.
- [ ] `docker build` succeeds and produces the local `aether` image running as a non-root user with `ffprobe` available in `PATH` and no permanent Node.js runtime process.

### End-to-End Standalone Runtime Verification
- [ ] Starting the service via Docker Compose initializes SQLite migrations in the persistent data volume and reports a healthy container status.
- [ ] Anonymous requests to protected API/catalog/stream endpoints are rejected (`401`/`403`), and `Host`/`Origin`/CSRF protections remain active when bound in Docker.
- [ ] The documented Docker administrator bootstrap command initializes the first `administrator` account cleanly, and subsequent login succeeds.
- [ ] Scanning a read-only (`:ro`) synthetic media library succeeds inside the container using `ffprobe`, populates the catalog and posters/artwork, and does not modify any file in the `:ro` mount.
- [ ] MP4 playback and HTTP `Range` seek requests (`206 Partial Content`, invalid range `416`) succeed against the running container.
- [ ] Stopping and restarting the container against the same persistent SQLite volume preserves users, sessions/catalog, and playback state, and cleanly handles any scan state interrupted by shutdown.
- [ ] If Docker is available in the environment, actual measurements (image size, idle RSS, scan RSS, startup time) are recorded in the final report and documentation; if any host environment limitation blocks execution, the exact blocker and unverified steps are reported transparently without fabricated metrics.
