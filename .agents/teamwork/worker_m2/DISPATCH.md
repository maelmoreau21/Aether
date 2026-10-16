## 2026-10-16T13:42:50Z
You are the Milestone 2 Implementation Worker for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m2\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\orchestrator_1\PROJECT.md.

Read the synthesis reports from the Explorers:
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_1\analysis.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_2\analysis.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_3\analysis.md

Exclusive Write Ownership:
You have exclusive write ownership of:
- Dockerfile (at repository root: c:\Users\Mael\Documents\GitHub\Aether\Dockerfile)
- .dockerignore (at repository root: c:\Users\Mael\Documents\GitHub\Aether\.dockerignore)

Task Description:
Implement Milestone 2: Container Packaging (Dockerfile & .dockerignore).
1. Author `.dockerignore` at repository root:
   - Exclude `.git/`, `.github/`, `.agents/`, `target/`, `node_modules/`, `apps/web/node_modules/`, `apps/web/.svelte-kit/`, `apps/web/build/`, `tests/fixtures/generated/`, `artifacts/`, `docs/evidence/`, `data/`, `*.sqlite3*`, `*.log`, `.DS_Store`, `Thumbs.db`.
   - Ensure context transfer is minimal (< 5 MB instead of > 23.7 GB).
2. Author multi-stage `Dockerfile` at repository root:
   - Stage 1 (`web-builder`): `node:22-bookworm-slim`, install dependencies (`npm ci --no-audit --no-fund`), compile static web via `npm run build` and `externalize-bootstrap.mjs`.
   - Stage 2 (`rust-builder`): `rust:slim-bookworm` (provides rustc 1.99.1), scaffold dummy manifests + explicit tests `tests/p0.rs` and `tests/catalog.rs` for dependency caching, copy `crates/`, compile release binaries `media-poc` and `media-bootstrap` (`cargo build --release --locked --bin media-poc --bin media-bootstrap`), and strip binaries with `strip --strip-all`.
   - Stage 3 (`runtime`): `debian:bookworm-slim`, install `ca-certificates`, `curl`, `ffmpeg` (for `ffprobe`), `tini`. Establish user `aether` (UID/GID 10001:10001). Create `/data` (0750 mode, owned by aether:aether) and `/media` (0755 mode). Copy stripped binaries to `/usr/local/bin/` and static web assets to `/app/apps/web/build`. Set default environment variables (`MEDIA_PORT=3000`, `MEDIA_BIND_ADDR=0.0.0.0`, `MEDIA_DB_PATH=/data/core.sqlite3`, `MEDIA_LIBRARY_ROOT=/media`, `MEDIA_WEB_DIST=/app/apps/web/build`), `USER aether:aether`, `WORKDIR /app`, `EXPOSE 3000`, `STOPSIGNAL SIGINT`, healthcheck probe on `http://127.0.0.1:3000/api/v1/health`, and entrypoint `["tini", "--", "media-poc"]`.
3. Verify the container build and runtime properties:
   - Run `docker build -t aether:latest .`
   - Run `docker run --rm aether:latest id` and confirm non-root user `10001:10001`.
   - Run `docker run --rm aether:latest ffprobe -version` and confirm `ffprobe` is functional in PATH.
   - Run `docker run --rm aether:latest which node` and confirm Node.js is NOT installed in runtime.
   - Run `docker run --rm aether:latest media-bootstrap --help` and verify binary runs cleanly.
4. Verify workspace regressions:
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test`
   - In `apps/web`: `npm run check`, `npm test`

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Write your completion and handoff report to:
`c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m2\handoff.md`
Report the exact commands executed and results, and send a message back when complete.
