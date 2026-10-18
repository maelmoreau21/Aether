# Progress

- Last visited: 2026-10-16T12:28:50Z
- Status: Investigation complete, drafting comprehensive survey report and handoff
- Current step: Writing survey_security_config.md and handoff.md

## Completed Investigations
1. **Configuration & MEDIA_* Environment Variables**: Audited all variables across crates and scripts. Detailed parsing, validation, fallbacks, and paths.
2. **Network Binding & Security**: Traced TCP binding (`Ipv4Addr::LOCALHOST`), `local_security` Host/Origin middleware, CSRF tokens, lack of CORS, and Docker loopback / reverse-proxy requirements.
3. **SQLite Migrations & Persistence**: Analyzed embedded `sqlx::migrate!`, WAL journal mode, busy timeouts, migrations 0001-0006, database isolation checks, and restart tests.
4. **Local Administrator Bootstrap**: Inspected `media-bootstrap` binary, strict TTY enforcement, interactive zeroized credentials, Argon2id hashing, and `bootstrap_state` singleton table.
5. **Read-Only Media Mounts & Scanner**: Verified 100% read-only operations on media mounts, internal SQLite-only cache tables (`inventory_assets`, `inventory_images`), companion poster streaming without extraction to disk, and interrupted scan recovery via `scan_jobs` status updates.
6. **Healthcheck Endpoint**: Analyzed `/api/v1/health`, static JSON response, lack of DB probe, and the critical Host-check conflict when running Docker healthchecks with external `MEDIA_PUBLIC_URL`.
