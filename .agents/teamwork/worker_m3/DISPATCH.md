## 2026-10-16T13:55:09Z

You are the Milestone 3 Implementation Worker for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m3\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\orchestrator_1\PROJECT.md.

Read the reference reports:
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\spec_miner_survey_3\spec_requirements.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_survey_2\survey_security_config.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m2\handoff.md

Exclusive Write Ownership:
You have exclusive write ownership of:
- compose.yaml (at repository root: c:\Users\Mael\Documents\GitHub\Aether\compose.yaml)
- docker-compose.yml (at repository root: c:\Users\Mael\Documents\GitHub\Aether\docker-compose.yml)

Task Description:
Implement Milestone 3: Synology DS1522+ Docker Compose & Operational Hardening.
1. Author `compose.yaml` (and `docker-compose.yml`) at repository root:
   - Configured specifically for Synology DS1522+ (AMD Ryzen R1600 x86_64, no iGPU, no `/dev/dri`) and local standalone self-hosting.
   - Service `aether`:
     - `image: aether:latest` (with `build: { context: . }`)
     - `container_name: aether`
     - Ports: default strictly to host loopback `"127.0.0.1:3000:3000"` (with comments explaining LAN / reverse-proxy configuration).
     - Volumes:
       - `aether_data:/data` (named persistent volume)
       - `${MEDIA_LIBRARY_DIR:-./tests/fixtures/generated}:/media:ro` (strict read-only media mount)
     - Environment:
       - `MEDIA_PORT=3000`
       - `MEDIA_BIND_ADDR=0.0.0.0`
       - `MEDIA_PUBLIC_URL=http://127.0.0.1:3000`
       - `MEDIA_DB_PATH=/data/core.sqlite3`
       - `MEDIA_LIBRARY_ROOT=/media`
       - `MEDIA_WEB_DIST=/app/apps/web/build`
     - Security hardening:
       - `cap_drop: ["ALL"]`
       - `security_opt: ["no-new-privileges:true"]`
       - `stop_signal: SIGINT`
       - `stop_grace_period: 15s`
       - `restart: unless-stopped`
     - Healthcheck:
       - `test: ["CMD-SHELL", "curl -f http://127.0.0.1:3000/api/v1/health || exit 1"]`
       - `interval: 30s`
       - `timeout: 5s`
       - `retries: 3`
       - `start_period: 10s`
   - Named volume declaration: `volumes: aether_data:`
2. Verify Compose configuration and operational workflows:
   - Run `docker compose config` to validate syntax and resolution.
   - Test starting the container with `docker compose up -d` against a temporary data directory or volume.
   - Verify healthcheck status: `docker compose ps` shows `(healthy)`.
   - Test admin bootstrap workflow:
     - Verify non-interactive execution `docker compose exec aether media-bootstrap` fails closed with exit code 1 (`bootstrap requires a local interactive terminal and accepts no arguments`).
     - Verify interactive bootstrap via pseudo-TTY works without default passwords.
   - Verify database migrations and restart resilience:
     - Verify SQLite database `/data/core.sqlite3` is initialized with WAL mode.
     - Verify stopping (`docker compose down`) and restarting (`docker compose up -d`) preserves database contents and safely recovers any interrupted scans.
   - Clean up any test containers/volumes after verification.
3. Run workspace regressions:
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test`

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Write your completion and handoff report to:
`c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m3\handoff.md`
Report the exact commands executed and results, and send a message back when complete.
