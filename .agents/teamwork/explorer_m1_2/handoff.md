# Handoff Report: Milestone 1 Static Asset Serving and `MEDIA_WEB_DIST` Support

**Agent:** Milestone 1 Explorer 2 (`explorer_m1_2`)  
**Target Recipient:** Orchestrator (`e84c8bab-f4f1-4a93-8220-b8cbda41be06`) & Milestone 1 Implementer  
**Timestamp:** 2026-10-16T12:43:00Z  
**Type:** Hard Handoff (Investigation Complete)  

---

## 1. Observation

1. **Static Mount Point in Router:**  
   In `crates/core/src/lib.rs:150-156`:
   ```rust
   fn public_routes(mode: &'static str) -> Router {
       Router::new()
           .nest_service(
               "/app",
               tower_http::services::ServeDir::new("apps/web/build")
                   .append_index_html_on_directories(true),
           )
   ```
   The path `"apps/web/build"` is hardcoded as a string literal.

2. **Integration into App Router & Security Layer:**  
   In `crates/core/src/lib.rs:111-118`:
   ```rust
   Ok(local_security(
       public_routes(mode)
           .merge(media)
           .merge(scans::router(auth.clone(), root.clone(), jobs).await?)
           .merge(libraries::router(auth.clone(), root))
           .merge(auth::router(auth)),
       origin,
   ))
   ```
   `public_routes` is merged into the app router and protected by `local_security`, which applies:
   - `Host` header check (`req.headers().get(header::HOST) == host.as_deref()`)
   - `Origin` check (`req.headers().get(header::ORIGIN) == origin.as_bytes()`)
   - `sec-fetch-site != "cross-site"`
   - Security headers: `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`, and strict `Content-Security-Policy`.

3. **Frontend Build Configuration and Output:**  
   - `apps/web/vite.config.ts:4`:
     ```ts
     export default defineConfig({ plugins: [sveltekit({ adapter: adapter(), paths: { base: '/app' }, csp: { mode: 'hash', directives: { 'script-src': ['self'] } } })] ...
     ```
   - Directory listing of `apps/web/build/` shows prerendered directories containing `index.html`:
     `index.html`, `login/index.html`, `admin/libraries/index.html`, `films/index.html`, `series/index.html`, `settings/index.html`, `search/index.html`, `index-bootstrap-0.js`, and `_app/immutable/`.

4. **Existing Test Coverage Audit:**  
   - `grep_search` across `crates/core/tests/` for `"/app"` returned **0 results**.
   - `grep_search` across `crates/core/tests/` for `ServeDir` found only `browser_auth.rs:32`:
     ```rust
     // Static file service resolves from repository root.
     let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
         .join("../..")
         .canonicalize()
         .unwrap();
     // Run this test alone: ServeDir uses the process working directory.
     std::env::set_current_dir(&root).unwrap();
     ```
   - All browser tests (`browser_auth.rs`, `browser_scanner.rs`, `browser_nfo.rs`, `browser_posters.rs`) are marked `#[ignore = "..."]`.
   - **Zero** active non-ignored tests in the repository test static asset serving under `/app`.

5. **Path Resolution Patterns in Codebase:**  
   In `crates/core/src/identity.rs:177-187`:
   ```rust
   pub fn configured_path(media_root: &Path) -> Result<std::path::PathBuf, std::io::Error> {
       let path = std::path::PathBuf::from(
           std::env::var_os("MEDIA_DB_PATH").unwrap_or_else(|| "data/core.sqlite3".into()),
       );
       let parent = path
           .parent()
           .filter(|p| !p.as_os_str().is_empty())
           .unwrap_or(Path::new("."));
       let parent = parent.canonicalize()?;
       let resolved = if path.exists() {
           path.canonicalize()?
       } else { ... };
   ```
   Optional canonicalization when path exists is the established pattern.

---

## 2. Logic Chain

1. **Step 1 (Working Directory Vulnerability):** Observation 1 shows that `ServeDir::new("apps/web/build")` uses a relative path. In Tower-HTTP, relative paths are resolved against the current working directory (`std::env::current_dir()`) of the running process at request time.
2. **Step 2 (Container and Subdirectory Failure):** If `media-poc` is launched with a working directory other than the workspace root (e.g. `/` in Docker, or `crates/core` when running cargo), relative `"apps/web/build"` fails to find the assets. Observation 4 verifies that existing browser tests had to forcefully execute `std::env::set_current_dir(&root)` or `.current_dir(root)` as a workaround.
3. **Step 3 (Configuration Contract):** According to `PROJECT.md` line 49 and `ORIGINAL_REQUEST.md`, `MEDIA_WEB_DIST` must be supported, defaulting to `"apps/web/build"` when unset or empty/whitespace.
4. **Step 4 (Path Resolution & Canonicalization Strategy):**
   - If `canonicalize()` were mandatory (`?`), startup would fail if the web assets haven't been built yet (e.g. fresh git clone or backend CI testing), breaking existing `cargo test` runs.
   - If `canonicalize()` is completely skipped, relative paths remain sensitive to working directory changes at runtime.
   - Therefore, a **best-effort canonicalization** (`raw.canonicalize().unwrap_or(raw)`) is optimal: if the path exists, it locks into an absolute path regardless of future cwd modifications; if it does not exist, it safely falls back to `raw` without crashing the daemon.
5. **Step 5 (404 Fallback & Security Invariants):** When an asset or the dist directory does not exist, `ServeDir` naturally returns HTTP `404 Not Found`. `ServeDir`'s built-in path sanitizer prevents directory traversal (`..` sequences). All `/app` requests remain wrapped by `local_security`, preserving CSP, Host/Origin validation, and anti-sniffing headers.
6. **Step 6 (Testing Gap Resolution):** Observation 4 established that no regular automated tests cover `/app`. A dedicated integration test (`tests/static_web.rs`) setting a custom `MEDIA_WEB_DIST` in a temp directory provides hermetic, zero-dependency test coverage without requiring Node.js or Playwright.

---

## 3. Caveats

1. **No Source Modifications Made:** As an Explorer agent, no source files in `crates/core/` have been directly edited. All implementation proposals are provided as diffs in `analysis.md` and this handoff.
2. **Interaction with Healthcheck Probe (Explorer 3):** `/api/v1/health` and `/app` are both currently in `public_routes`. Explorer 3 is investigating exempting or isolating `/api/v1/health` from `local_security` for container healthcheck probes. Static asset serving under `/app` MUST remain protected by `local_security`.
3. **SvelteKit Dynamic Routing:** SvelteKit in this project is pre-rendered via `@sveltejs/adapter-static` with explicit subdirectories (`login/index.html`, etc.). It does not currently require single-page app (SPA) fallback routing (e.g. serving `index.html` on 404), and `ServeDir`'s `append_index_html_on_directories(true)` matches the pre-rendered layout.

---

## 4. Conclusion

1. **Implementation is straightforward, low-risk, and backward-compatible:**
   - Define `pub fn configured_web_dist() -> std::path::PathBuf` in `crates/core/src/lib.rs`.
   - Update `public_routes` in `crates/core/src/lib.rs:154` to use `tower_http::services::ServeDir::new(configured_web_dist()).append_index_html_on_directories(true)`.
   - In `crates/core/src/main.rs`, log a non-fatal warning if `!configured_web_dist().exists()`.
2. **Environment Variable Rules:**
   - Default: `"apps/web/build"`.
   - When `MEDIA_WEB_DIST` is unset, `""`, or whitespace: use default.
   - When `MEDIA_WEB_DIST` is set: use trimmed value, best-effort canonicalized.
3. **Container Compatibility:**
   - Supports `ENV MEDIA_WEB_DIST=/app/apps/web/build` in Dockerfile or custom volume mount paths in `compose.yaml`.
4. **Testing Recommendation:**
   - Add automated test `crates/core/tests/static_web.rs` verifying `/app/` static serving, custom `MEDIA_WEB_DIST`, 404 handling, and security headers.

---

## 5. Verification Method

To independently verify this assessment:

1. **Check Hardcoded Route:**
   Inspect `crates/core/src/lib.rs` line 154 to confirm `ServeDir::new("apps/web/build")`.
2. **Verify Absence of Unit Tests for `/app`:**
   Run:
   ```bash
   git grep "/app" crates/core/tests/
   ```
   Observe that only commented or ignored Playwright tests exist.
3. **Verify Existing Workspace Tests Pass:**
   Run from repository root:
   ```bash
   cargo test --test identity
   cargo test --test permissions
   ```
   Both commands succeed cleanly.
4. **Proposed Implementation Verification:**
   After the implementer applies the diff in `analysis.md`:
   - Run `cargo test --test static_web` to verify `MEDIA_WEB_DIST` override and HTTP 200/404 responses.
   - Run `cargo test` to verify no regressions across the workspace.
