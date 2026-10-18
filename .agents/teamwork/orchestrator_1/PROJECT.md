# Project: Aether All-in-One Standalone Docker Distribution

## Architecture
- **Single Process Model**: Packages the unified `media-poc` daemon (combining Axum HTTP, Core identity/auth, local Media Node inventory/scanner, and SvelteKit static asset serving) into an unprivileged Linux container.
- **Multi-Stage Build Pipeline**:
  - Stage 1 (`web-builder`): Node.js environment builds SvelteKit frontend via `@sveltejs/adapter-static` and executes `scripts/externalize-bootstrap.mjs` for CSP compliance.
  - Stage 2 (`rust-builder`): Rust cargo environment builds release binaries `media-poc` and `media-bootstrap` (BuildKit caching with dummy manifests, stripped symbols).
  - Stage 3 (`runtime`): Minimal Debian bookworm-slim image containing `ffprobe`, `ca-certificates`, `tini`, and non-root user `aether` (`10001:10001`).
- **Storage & Volume Topology**:
  - Media libraries mounted strictly read-only (`:ro`).
  - Persistent application state and SQLite database mounted on dedicated writable volume (`/data`).
  - No disk cache writes in media directories; scan/probe/image metadata cached in SQLite.
- **Network & Security Invariants**:
  - Process binds to `0.0.0.0` inside container via `MEDIA_BIND_ADDR`.
  - Docker Compose defaults to host loopback binding `127.0.0.1:3000:3000`.
  - `Host`, `Origin`, and CSRF validation strictly enforced against `MEDIA_PUBLIC_URL`.
  - Zero default admin credentials; bootstrap via interactive TTY `docker compose exec -it aether media-bootstrap`.
  - Internal healthcheck on `/api/v1/health` resilient to external `MEDIA_PUBLIC_URL` setting.

## Feature Inventory
| # | Feature | Description | Milestone | Source | Status |
|---|---------|-------------|-----------|--------|--------|
| 1 | Runtime Config Flexibility | Support `MEDIA_BIND_ADDR` and `MEDIA_WEB_DIST` with safe defaults | M1 | Survey (Explorer 2) | DONE |
| 2 | Resilient Healthcheck | Ensure `/api/v1/health` allows container health probes regardless of `MEDIA_PUBLIC_URL` | M1 | Survey (Explorer 2) | DONE |
| 3 | Static Asset Location Support | Ensure ServeDir resolves static assets in container WORKDIR | M1 | Survey (Explorer 1) | DONE |
| 4 | `.dockerignore` Context Filter | Comprehensive exclusion of `.git`, `target`, `node_modules`, media fixtures, test logs | M2 | Survey (Spec Miner 3) | DONE |
| 5 | Multi-Stage `Dockerfile` | 3-stage build producing minimal non-root `aether` image with `ffprobe` & `tini` | M2 | Survey (Spec Miner 3) | DONE |
| 6 | Synology DS1522+ Compose | `compose.yaml` with loopback binding, `:ro` media mounts, `/data` persistent volume, healthcheck | M3 | Survey (Spec Miner 3) | DONE |
| 7 | Secure Admin Bootstrap Workflow | Interactive TTY execution of `media-bootstrap` via Docker | M3 | Survey (Spec Miner 3) | DONE |
| 8 | Scan Interruption Recovery | Verification of SQLite recovery of interrupted scans on container restart | M3 | Survey (Explorer 2) | DONE |
| 9 | E2E Standalone Runtime Verification | Validate AC-1 through AC-10 against container runtime (auth, scan, range seek, restarts) | M4 | ORIGINAL_REQUEST § AC | DONE |
| 10 | Operational Documentation & Metrics | Update README.md, MEMORY.md, deployment guide with commands & real/blocker metrics | M4 | ORIGINAL_REQUEST § R4 | DONE |

## Milestones
| # | Name | Scope | Dependencies | Status | Outputs |
|---|------|-------|-------------|--------|---------|
| 1 | Backend Containerization Adaptations | Add `MEDIA_BIND_ADDR`, `MEDIA_WEB_DIST`, healthcheck loopback probe handling | none | DONE | crates/core/src/{main.rs, lib.rs}, static_web.rs, healthcheck_resilience.rs |
| 2 | Container Packaging (`Dockerfile` & `.dockerignore`) | Multi-stage build, non-root user (10001:10001), tini, ffprobe, static web copy | M1 | DONE | Dockerfile, .dockerignore, aether:latest (213 MB) |
| 3 | Compose & Operational Hardening | Synology DS1522+ `compose.yaml`, secure bootstrap workflow, healthcheck test | M2 | DONE | compose.yaml, docker-compose.yml |
| 4 | E2E Verification & Documentation | Complete test suites, runtime verification (AC-1..AC-11), metrics, docs update | M3 | DONE | README.md, MEMORY.md, docs/DEPLOYMENT_DOCKER.md |

## Interface Contracts
### Environment Variables
- `MEDIA_PORT`: Port number (default `3000`).
- `MEDIA_BIND_ADDR`: IPv4 address to listen on (default `127.0.0.1`, set to `0.0.0.0` in container).
- `MEDIA_PUBLIC_URL`: Canonical origin for `Host` & `Origin` checks (e.g. `http://127.0.0.1:3000`).
- `MEDIA_DB_PATH`: SQLite database path (default `data/core.sqlite3`, `/data/core.sqlite3` in container).
- `MEDIA_LIBRARY_ROOT`: Media mount root (default `tests/fixtures/generated`, `/media` in container).
- `MEDIA_WEB_DIST`: Path to static web build (default `apps/web/build`).

### Container Endpoints & Commands
- Primary daemon: `tini -- media-poc`
- Bootstrap CLI: `docker compose exec -it aether media-bootstrap`
- Healthcheck: `/api/v1/health` (HTTP GET, returns 200 OK with `{"status":"ok","mode":"standalone"}`)

## Code Layout
- `crates/core/src/main.rs`: Network binding with `MEDIA_BIND_ADDR`.
- `crates/core/src/lib.rs`: Static asset directory configuration and healthcheck route handling.
- `Dockerfile`: Multi-stage image build definition at repository root.
- `.dockerignore`: Build context exclusion rules at repository root.
- `compose.yaml`: Synology DS1522+ and local standalone Compose file at repository root.
- `docs/DEPLOYMENT_DOCKER.md`: Detailed deployment guide.
- `README.md`, `MEMORY.md`: Updated operational documentation and records.
