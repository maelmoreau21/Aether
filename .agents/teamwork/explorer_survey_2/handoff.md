# Handoff Report : Security, Config, Database, and Admin Bootstrap Survey

> Agent: Security and Config Explorer (`explorer_survey_2`)  
> Target: Orchestrator (`e84c8bab-f4f1-4a93-8220-b8cbda41be06`)  
> Working Directory: `c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_survey_2\`  
> Associated Comprehensive Report: `survey_security_config.md`

---

## 1. Observation

Direct code and schema inspections yielded the following factual observations:

### A. Configuration & `MEDIA_*` Environment Variables
- `crates/core/src/main.rs:9`: `let library_root = std::env::var("MEDIA_LIBRARY_ROOT").ok();`
- `crates/core/src/main.rs:10-12`: `let root = PathBuf::from(library_root.clone().unwrap_or_else(|| { std::env::var("MEDIA_FIXTURE_DIR").unwrap_or_else(|_| "tests/fixtures/generated".into()) }));`
- `crates/core/src/main.rs:13-15`: `let port: u16 = std::env::var("MEDIA_PORT").unwrap_or_else(|_| "3000".into()).parse()?;`
- `crates/core/src/main.rs:28-29`: `let public_origin = std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));`
- `crates/core/src/identity.rs:177-180`: `let path = std::path::PathBuf::from(std::env::var_os("MEDIA_DB_PATH").unwrap_or_else(|| "data/core.sqlite3".into()));`
- `crates/core/src/bootstrap.rs:22-26`: `let root = PathBuf::from(std::env::var_os("MEDIA_LIBRARY_ROOT").or_else(|| std::env::var_os("MEDIA_FIXTURE_DIR")).unwrap_or_else(|| "tests/fixtures/generated".into()));`
- `crates/core/src/node.rs:124-154` (`validate_library_root`): enforces `root.is_absolute() && allowed_root.is_absolute()`, rejects `Component::ParentDir`, canonicalizes and verifies directory existence.
- `crates/core/src/identity.rs:185`: `let parent = parent.canonicalize()?;` (returns error if DB parent folder does not exist).
- `crates/core/src/identity.rs:195-201`: `if let Ok(root) = media_root.canonicalize() && resolved.starts_with(root) { return Err(std::io::Error::other("database must be outside the media directory")); }`
- `crates/core/src/lib.rs:154`: `tower_http::services::ServeDir::new("apps/web/build")` (hardcoded relative path for SvelteKit static build).

### B. Network Binding, Host/Origin/CORS/CSRF & Reverse Proxies
- `crates/core/src/main.rs:25-27`: `let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;` — strictly binds to `127.0.0.1`.
- `crates/core/src/lib.rs:170-188`: `local_security` extracts authority from `MEDIA_PUBLIC_URL` and rejects requests where `Host` header does not match, where `sec-fetch-site == "cross-site"`, or where `Origin` is present and does not match `origin`.
- `crates/core/src/auth.rs:76-79`: rejects `origin` if `!(scheme == "https" || (scheme == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")))`. Direct plain HTTP across LAN is refused at startup.
- `crates/core/src/auth.rs:115-135` & `290-295`: `require_csrf` verifies `x-csrf-token` header matching SHA-256 HMAC of session token using constant-time comparison (`constant_equal`).
- `crates/core/src/auth.rs:149`: `; Secure` cookie attribute added if scheme is `https`.

### C. SQLite Migrations & Persistence
- `crates/core/src/identity.rs:92-96`:
  ```rust
  let mut conn = pool.acquire().await?;
  let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
  sqlx::migrate!("./migrations").run(&mut *tx).await?;
  tx.commit().await?;
  ```
  Migrations are embedded at compile time into the binary via `sqlx::migrate!`.
- `crates/core/src/identity.rs:62-67`: SQLite WAL journal mode (`SqliteJournalMode::Wal`), `busy_timeout(15s)`, `max_connections(4)`.
- 6 migrations in `crates/core/migrations/`: `0001_identity.sql`, `0002_sessions.sql`, `0003_libraries.sql`, `0004_inventory.sql`, `0005_film_editorial.sql`, `0006_local_posters.sql`.

### D. Local Administrator Bootstrap
- `crates/core/Cargo.toml:32-34`: binary target `[[bin]] name = "media-bootstrap" path = "src/bootstrap.rs"`.
- `crates/core/src/bootstrap.rs:17-21`:
  ```rust
  if std::env::args_os().len() != 1 || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
      return Err("bootstrap requires a local interactive terminal and accepts no arguments".into());
  }
  ```
- `crates/core/src/bootstrap.rs:34-39`: prompts for login, password, and password confirmation via `rpassword::prompt_password`, wrapped in `Zeroizing<String>`.
- `crates/core/src/identity.rs:132-141`: hashes password with Argon2id v19 (64 MiB, 3 passes, 1 lane, 32-byte salt).
- `crates/core/src/identity.rs:145-152, 167-169`: checks `bootstrap_state` and `users`, inserts admin user, sets `bootstrap_state(singleton) = 1`. Permanent closure of bootstrap.

### E. Media Scanner & Read-Only (`:ro`) Handling
- `crates/core/src/node/scanner.rs:90-120`: strictly validates symlinks and confinement (`confined_file`), rejects symlinks/reparse points (`linked()`).
- All media file accesses are read-only: `tokio::fs::symlink_metadata`, `tokio::fs::read_dir`, `std::fs::File::open`, `ffprobe` (`crates/core/src/media.rs:41-67`).
- Zero extracted files written to media directory: companion posters (`poster.jpg`, etc.) discovered in place, validated in memory (`crates/core/src/node/images.rs:35-105`), and served directly from original path via `ServeFile` (`crates/core/src/inventory.rs:505-558`).
- Scan caches reside in SQLite (`inventory_assets.probe_json`, `inventory_images`), not in a disk cache folder.
- `crates/core/src/scans.rs:79-80`: startup resets interrupted jobs to `status='error'`, errors_json=`["Scan interrompu par un redémarrage"]`.
- `crates/core/src/inventory.rs:86`: missing files only marked unavailable if `report.complete == true`; aborted scans preserve previous inventory.

### F. Healthcheck Endpoint
- `crates/core/src/lib.rs:163-167`: route `/api/v1/health` returns `{"status":"ok", "mode":mode}`. Route `/health` does NOT exist.
- Does not check SQLite database connection or pool readiness.
- Nested inside `local_security`: if `MEDIA_PUBLIC_URL` has an external domain (e.g. for reverse proxy), an internal container healthcheck via `curl http://127.0.0.1:3000/api/v1/health` will be rejected with `403 Forbidden` due to `Host` mismatch unless `Host` is explicitly passed in `curl` headers.

---

## 2. Logic Chain

1. **Network Binding & Docker Port Forwarding** (From Obs A, B):
   - `main.rs:26` binds to `Ipv4Addr::LOCALHOST` (127.0.0.1).
   - In Docker bridge networking, external packets arrive on the container's virtual ethernet interface (`eth0`, e.g. 172.17.0.2).
   - Listening only on `127.0.0.1` inside a container means packets forwarded by `docker-proxy` get `ECONNREFUSED`.
   - *Therefore*: Aether must be adapted or configured to bind to `0.0.0.0` inside Docker (e.g. via `MEDIA_BIND_ADDR=0.0.0.0`), while host security is maintained by binding to `127.0.0.1:3000:3000` in `docker-compose.yml`.

2. **Reverse Proxy and Remote Access Security** (From Obs B):
   - `AuthState::new` (`auth.rs:76-79`) rejects plain HTTP for any host other than loopback (`127.0.0.1`, `localhost`, `[::1]`).
   - `local_security` (`lib.rs:180`) strictly rejects requests whose `Host` header does not match `host` authority.
   - *Therefore*: Remote or LAN access must be mediated by a reverse proxy with HTTPS termination. The reverse proxy must forward `Host` and `Origin` headers unchanged (`proxy_set_header Host $host`), and `MEDIA_PUBLIC_URL` must match the public HTTPS URL.

3. **Storage Separation & Volume Persistence** (From Obs A, C, E):
   - `configured_path` (`identity.rs:195-201`) forbids SQLite from residing in the media directory.
   - The media scanner and image handlers perform zero disk writes to the media directory.
   - The probe and image caches reside entirely in SQLite tables.
   - *Therefore*: Media mounts in Docker can safely be mounted `:ro` without causing errors. Only the database directory (`MEDIA_DB_PATH`, e.g. `/data`) requires a persistent writable volume (`aether_data:/data`).

4. **Zero-Default-Credential Bootstrap in Docker** (From Obs D):
   - `media-bootstrap` requires `is_terminal() == true` on stdin/stdout and takes no arguments.
   - Docker supports allocating a pseudo-TTY via `docker compose exec -it aether media-bootstrap` or `docker compose run --rm -it aether media-bootstrap`.
   - *Therefore*: Administrator initialization can be executed cleanly without default passwords, environment variables, or hardcoded secrets in Dockerfiles or Compose files.

5. **Container Healthcheck Reliability** (From Obs B, F):
   - `/api/v1/health` currently lives inside `local_security`.
   - When an operator sets `MEDIA_PUBLIC_URL=https://nas.example.com`, standard `curl http://127.0.0.1:3000/api/v1/health` sends `Host: 127.0.0.1:3000`, which fails the `Host` equality check, returning `403 Forbidden`.
   - *Therefore*: To avoid false `unhealthy` container failures, the healthcheck probe must either send the matching `Host` header, or the healthcheck route in Core should be excluded from external Host restrictions.

---

## 3. Caveats

1. **Series Scanner Excluded**: As observed in `crates/core/src/scans.rs:141`, only libraries with `kind == 'films'` and `id NOT IN ('fixture-films', 'fixture-series')` can currently be scanned. Series scanning is out of scope for MVP.
2. **Readiness Verification Depth**: `/api/v1/health` is purely static. A container could report healthy even if the SQLite database file became corrupted after startup, until active DB probing is added to `/api/v1/health`.
3. **Frontend Path**: `tower_http::services::ServeDir::new("apps/web/build")` assumes execution from repository root or matching folder layout in the container.

---

## 4. Conclusion

1. **Configuration**: Five environment variables are active (`MEDIA_PORT`, `MEDIA_LIBRARY_ROOT`, `MEDIA_FIXTURE_DIR`, `MEDIA_DB_PATH`, `MEDIA_PUBLIC_URL`). Path validation strictly enforces absolute media paths and separates database storage from media storage.
2. **Network Binding**: Binding is currently hardcoded to `127.0.0.1`. For container bridge networking, Core needs support for binding to `0.0.0.0` (or `MEDIA_BIND_ADDR`), while Docker Compose enforces `127.0.0.1:3000:3000` on the host. Reverse proxy exposure requires `MEDIA_PUBLIC_URL=https://...` and `Host`/`Origin` forwarding.
3. **Database & Migrations**: SQLite migrations (0001-0006) are embedded in the binary and run safely at startup with `BEGIN IMMEDIATE` and WAL mode. Mount of a single persistent volume (`/data`) guarantees complete state preservation.
4. **Bootstrap**: `media-bootstrap` CLI operates via interactive TTY. Running it with `docker compose exec -it aether media-bootstrap` allows secure bootstrap without any default credentials.
5. **Scanner & Read-Only**: Scanner operations and companion poster delivery are 100% read-only. Media mounts should be mounted `:ro`. Scan crashes are cleanly recovered at restart, preserving existing inventory.
6. **Healthcheck**: Endpoint `/api/v1/health` exists but must be protected against `Host` check rejection when `MEDIA_PUBLIC_URL` is set to an external domain.

---

## 5. Verification Method

1. **Verify Codebase Compiles and Tests Pass**:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
2. **Verify Configuration & Restart Logic**:
   Inspect `crates/core/tests/core_restart.rs` and run:
   ```bash
   cargo test --test core_restart
   ```
3. **Verify Bootstrap Constraints**:
   Inspect `crates/core/tests/identity.rs` and run:
   ```bash
   cargo test --test identity cli_rejects_noninteractive_input_and_secret_arguments
   ```
4. **Verify Scanner Read-Only & Recovery**:
   Inspect `crates/core/src/scans.rs:79-80` and `crates/core/tests/scanner_flow.rs`:
   ```bash
   cargo test --test scanner_flow
   ```
5. **Verify Host/Origin Security Controls**:
   Inspect `crates/core/src/lib.rs:170-188` and `crates/core/tests/browser_auth.rs`.

---
*Report generated and self-verified by Security and Config Explorer (`explorer_survey_2`).*
