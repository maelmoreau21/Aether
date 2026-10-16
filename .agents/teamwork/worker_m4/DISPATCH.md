## 2026-10-16T14:08:27Z
You are the Milestone 4 Implementation & E2E Verification Worker for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m4\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\orchestrator_1\PROJECT.md.

Read the previous milestones' handoffs:
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m1\handoff.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m2\handoff.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m3\handoff.md

Exclusive Write Ownership:
You have exclusive write ownership of:
- README.md (at repository root)
- MEMORY.md (at repository root)
- docs/DEPLOYMENT_DOCKER.md (new file)

Task Description:
Perform Milestone 4: End-to-End Standalone Runtime Verification, Operational Documentation & Metrics Recording.
1. Perform complete End-to-End verification of Acceptance Criteria AC-1 through AC-11 against the container runtime:
   - AC-1: Verify `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass across the Rust workspace without regressions.
   - AC-2: In `apps/web`: verify `npm run check`, `npm run build`, and `npm test` pass without regressions.
   - AC-3: Verify `.dockerignore` excludes `.git`, `target`, `node_modules`, media fixtures, test logs from context (< 5 MB context transfer).
   - AC-4: Verify `docker build -t aether:latest .` produces non-root image (uid/gid 10001:10001) with `ffprobe` in `PATH` and zero permanent Node.js process.
   - AC-5: Start service via Compose (`docker compose up -d`), verify SQLite migrations in persistent volume and healthy container status (`docker compose ps`).
   - AC-6: Anonymous requests to protected API/catalog/stream endpoints are rejected (401/403), and Host/Origin/CSRF protections remain active when bound in Docker.
   - AC-7: Documented Docker administrator bootstrap initializes first `administrator` account cleanly via pseudo-TTY, and subsequent login succeeds.
   - AC-8: Scan read-only (`:ro`) synthetic media library inside container using `ffprobe`, verify catalog and posters/artwork are populated, and verify NO files in the `:ro` mount are modified.
   - AC-9: MP4 playback and HTTP Range seek requests (`206 Partial Content`, `416 Range Not Satisfiable`) succeed against running container.
   - AC-10: Stopping (`docker compose down`) and restarting (`docker compose up -d`) against the same persistent SQLite volume preserves users, sessions/catalog, and playback state, and cleanly recovers scan state.
   - AC-11: Measure and record actual runtime metrics:
     - Image size (MB).
     - Container startup duration to reach healthy (seconds).
     - Container memory RSS at idle (MB).
     - Container memory RSS during media library scanning (MB).
2. Update documentation:
   - Update `README.md` with standalone Docker deployment guide, Synology DS1522+ instructions, admin bootstrap command, configuration environment variables, reverse-proxy guide, and measured metrics.
   - Update `MEMORY.md`: record completed Docker standalone milestone (P1), architectural decisions (3-stage build, non-root 10001:10001, tini signal forwarding with STOPSIGNAL SIGINT, loopback default binding, route-decoupled healthcheck), and measured metrics.
   - Create `docs/DEPLOYMENT_DOCKER.md`: complete operational deployment guide for standalone self-hosting and Synology DS1522+ NAS via Container Manager.
3. Clean up all test containers and volumes after tests.
4. Mandatory integrity warning:
> DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.
5. Write completion report to:
`c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m4\handoff.md`
Report the exact commands executed and results, and send a message back when complete.
