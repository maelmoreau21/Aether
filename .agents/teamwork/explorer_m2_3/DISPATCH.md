## 2026-10-16T13:27:18Z
You are Milestone 2 Explorer 3 for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_3\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\orchestrator_1\PROJECT.md.

Task:
Investigate Milestone 2: Runtime Image Assembly, Non-Root Hardening, and .dockerignore.
1. Determine the minimal Linux x86_64 runtime image (e.g. debian:bookworm-slim) and exact system packages required (ffmpeg for ffprobe, ca-certificates, tini for PID 1 signal forwarding, curl for healthcheck).
2. Define non-root user setup: UID 10001, GID 10001 (user/group `aether`), home directory `/app`, directory ownership for `/data` and `/media`.
3. Verify signal handling: verify tini properly catches SIGTERM and forwards it to `media-poc` to trigger graceful shutdown.
4. Formulate the complete `.dockerignore` file content excluding .git, target, node_modules, generated fixtures, test artifacts, database files, and temporary files.
5. Provide a detailed, concrete recommendation and Dockerfile Stage 3 snippet. Do NOT modify source files directly.
Write your analysis and recommendations to c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_3\analysis.md and handoff report to c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m2_3\handoff.md. Send a message when complete.
