# Execution Plan: Aether All-in-One Standalone Docker Distribution

## Objective
Deliver a secure, functional all-in-one Docker standalone distribution for Aether (`aether` image and Docker Compose configuration) packaging compiled SvelteKit frontend, Rust/Axum Core + local Media Node, persistent SQLite, and `ffprobe` in a single non-root container without permanent Node.js runtime, second Rust server process, or mandatory PostgreSQL.

## Phase 0: Survey & Technical Exploration
- Spawn 3 Explorers / Spec Miners in parallel:
  - **Explorer 1**: Codebase survey (Rust workspace structure, binaries, how frontend is currently served/integrated or built, axum static asset serving capability, entrypoints).
  - **Explorer 2**: Security & Runtime Configuration survey (Docker non-root permissions, environment variables `MEDIA_*`, Host/Origin/CSRF validation, SQLite migrations, admin bootstrap workflow).
  - **Spec Miner / Explorer 3**: Requirements & Acceptance Criteria mapping against existing documentation (`AGENTS.md`, `ARCHITECTURE.md`, `MEMORY.md`, `docs/SECURITY.md`, `docs/SPECIFICATION.md`, Synology DS1522+ requirements, `ffprobe` integration).
- Synthesize findings into `PROJECT.md` (Feature Inventory, Architecture, Code Layout, Interfaces).

## Phase 1: Milestone Decomposition & Track Setup
- Validate Feature Inventory cross-check.
- Setup E2E Testing track or dual-track verification.

## Phase 2: Milestone Iterations (Packaging, Security/Config, Bootstrap/Healthcheck, Compose/E2E Verification)
- Milestone 1: Multi-stage Dockerfile, .dockerignore, Single binary / Static asset integration.
- Milestone 2: Configuration, network binding, Host/Origin/CSRF hardening.
- Milestone 3: Initial admin bootstrap workflow, restart resilience, SQLite migrations, healthcheck.
- Milestone 4: Synology DS1522+ Docker Compose, Documentation, E2E Test Suite verification & metrics recording.

## Phase 3: Final Verification & Reporting
- Full build and test verification (cargo test, clippy, fmt, frontend build/test, docker build & run tests).
- Gate checks (Reviewer, Challenger, Forensic Auditor).
- Update README.md, MEMORY.md, and docs.
- Final completion handoff report to parent.
