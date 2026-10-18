# Dispatch for Spec Miner 3 (Survey: Specifications & Docker/Compose Constraints)
Role: Specification Miner
Target: Extract all specifications and acceptance criteria from docs, AGENTS.md, MEMORY.md, and ORIGINAL_REQUEST.md.


## 2026-10-16T12:21:59Z
Sender: e84c8bab-f4f1-4a93-8220-b8cbda41be06
Priority: MESSAGE_PRIORITY_HIGH
Content:
You are the Docker and Deployment Spec Miner for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\spec_miner_survey_3\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\AGENTS.md, c:\Users\Mael\Documents\GitHub\Aether\ARCHITECTURE.md, c:\Users\Mael\Documents\GitHub\Aether\MEMORY.md, c:\Users\Mael\Documents\GitHub\Aether\docs\SECURITY.md, and c:\Users\Mael\Documents\GitHub\Aether\docs\SPECIFICATION.md.

Mine and extract all explicit and implicit requirements and constraints for the all-in-one Docker distribution:
1. Dockerfile requirements: multi-stage build (Node/SvelteKit build stage, Rust cargo build stage, minimal Linux x86_64 runtime stage with ffprobe), unprivileged non-root user (UID/GID), signal handling / graceful shutdown (tini or native), no permanent Node.js runtime, no second Rust server process.
2. .dockerignore requirements: complete list of paths/patterns to exclude (.git, target, node_modules, media fixtures, test logs/evidence, temp files).
3. Docker Compose requirements: configuration suitable for Synology DS1522+ (AMD Ryzen R1600 x86_64, no iGPU) and local standalone self-hosting. Volume mappings (:ro for media, persistent writable volumes for data/cache), port mappings (default 127.0.0.1 binding), environment variables, healthcheck definition.
4. Operational workflows: Initial admin bootstrap via Docker command, container restart resilience, scan interruption handling, upgrade path.
5. Complete acceptance criteria checklist and verification test plan mapping for all requirements R1-R4 in ORIGINAL_REQUEST.md.

Do NOT modify or write any source code or project files.
Write your comprehensive specification extraction report to:
c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\spec_miner_survey_3\spec_requirements.md
Also write your handoff report to c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\spec_miner_survey_3\handoff.md and send a message back to the orchestrator when finished.
