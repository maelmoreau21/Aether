## 2026-10-16T12:21:59Z
You are the Codebase Architecture Explorer for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_survey_1\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\AGENTS.md, c:\Users\Mael\Documents\GitHub\Aether\ARCHITECTURE.md, and c:\Users\Mael\Documents\GitHub\Aether\MEMORY.md.

Investigate the authoritative codebase (Rust workspace and SvelteKit frontend) to answer:
1. How is the Rust workspace structured? What binaries are produced? Is there already a standalone mode or unified binary combining Core and Media Node, or how are they run together in a single process?
2. How does the backend serve HTTP endpoints and static assets? Does Axum currently serve static files from apps/web/build or similar via tower_http::services::ServeDir, or is that needed?
3. How is the frontend (apps/web) built? What SvelteKit adapter is configured (@sveltejs/adapter-static vs @sveltejs/adapter-node)? What is the build output directory? Are there prerender/fallback settings?
4. How is ffprobe discovered, configured, and executed by the scanner/media inspection module? What path or environment variables are used?
5. What are the existing build and test commands across Rust and Web (cargo test, cargo clippy, pnpm test, etc.)?
6. What files and directories currently exist in the repository that must be excluded from Docker context in .dockerignore?

Do NOT modify or write any source code or project files.
Write your comprehensive survey report with verified evidence (file paths and line numbers) to:
c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_survey_1\survey_codebase.md
Also write your handoff report to c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_survey_1\handoff.md and send a message back to the orchestrator when finished.
