# Analysis: Static Asset Serving and `MEDIA_WEB_DIST` Support

**Author:** Milestone 1 Explorer 2  
**Date:** 2026-10-16  
**Component:** `crates/core` (`lib.rs`, `main.rs`) & SvelteKit Static Distribution  

---

## Executive Summary

In Milestone 1, Aether adapts its Axum Core daemon (`media-poc`) for standalone containerization. Currently, static frontend assets are hardcoded to relative path `"apps/web/build"` in `crates/core/src/lib.rs:154`:
```rust
fn public_routes(mode: &'static str) -> Router {
    Router::new()
        .nest_service(
            "/app",
            tower_http::services::ServeDir::new("apps/web/build")
                .append_index_html_on_directories(true),
        )
        ...
```
This hardcoded relative path causes failures if the process working directory is not the workspace root (e.g. running inside Docker with a custom `WORKDIR`, running from crate directories, or when container operators mount static assets to custom or absolute paths like `/app/apps/web/build`).

This investigation resolves the configuration, path resolution, fallback, and testing strategy for `MEDIA_WEB_DIST`.

---

## 1. Inspection of Existing Route Setup (`crates/core/src/lib.rs:150-167`)

### 1.1 Current Architecture
In `crates/core/src/lib.rs`:
- Lines 150–167 define `public_routes(mode: &'static str) -> Router`.
- The route `/app` is mounted via `Router::nest_service("/app", tower_http::services::ServeDir::new("apps/web/build").append_index_html_on_directories(true))`.
- `public_routes` is merged into the root router inside `secure_app_with_jobs(...)` at line 112, and wrapped by `local_security(...)` at line 111.
- `local_security` applies:
  - `Host` header enforcement matching `origin.authority()`.
  - `Origin` header validation against `origin`.
  - `sec-fetch-site: cross-site` rejection (403 Forbidden).
  - Security response headers:
    - `Content-Security-Policy: default-src 'none'; script-src 'self'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; frame-ancestors 'none'`
    - `Cache-Control: no-store`
    - `X-Content-Type-Options: nosniff`

### 1.2 Frontend Build Artifact Structure
SvelteKit is built via `@sveltejs/adapter-static` with base path `/app` (`apps/web/vite.config.ts:4`).
The output directory `apps/web/build/` contains prerendered directories containing `index.html`:
- `index.html` (for `/app/`)
- `login/index.html` (for `/app/login/`)
- `admin/libraries/index.html` (for `/app/admin/libraries/`)
- `films/index.html` (for `/app/films/`)
- `series/index.html` (for `/app/series/`)
- `settings/index.html` (for `/app/settings/`)
- `search/index.html` (for `/app/search/`)
- `_app/immutable/...` (JS/CSS bundles)
- `index-bootstrap-0.js` (CSP-externalized inline script)

Because all routes are prerendered as directories containing `index.html`, `tower_http::services::ServeDir::new(...).append_index_html_on_directories(true)` serves every page correctly when requests end in `/` (e.g. `/app/`, `/app/login/`). For directories requested without trailing slash (e.g. `/app/login`), `ServeDir` issues an automatic redirect to `/app/login/`.

---

## 2. Configuration Design for `MEDIA_WEB_DIST`

### 2.1 Specification & Invariants
- **Variable Name:** `MEDIA_WEB_DIST` (conforms to `MEDIA_*` prefix rule in `AGENTS.md` and `PROJECT.md`).
- **Default Value:** `"apps/web/build"`.
- **Condition for Default:** If `MEDIA_WEB_DIST` is:
  1. Not set (`std::env::var` returns `Err(VarError::NotPresent)`).
  2. Empty string (`""`).
  3. Whitespace-only (`"   "`).
- **Custom Value Support:** Any non-empty trimmed string is accepted, supporting:
  - Absolute container paths: `/app/apps/web/build`, `/usr/share/aether/web`, `/data/web`.
  - Relative paths: `apps/web/build`, `../../apps/web/build`, `./dist`.

### 2.2 Extraction Function
```rust
/// Resolve the static web distribution path from `MEDIA_WEB_DIST`.
/// Defaults to "apps/web/build" if unset or empty/whitespace.
pub fn web_dist_path() -> std::path::PathBuf {
    match std::env::var("MEDIA_WEB_DIST") {
        Ok(val) if !val.trim().is_empty() => std::path::PathBuf::from(val.trim()),
        _ => std::path::PathBuf::from("apps/web/build"),
    }
}
```

---

## 3. Path Resolution, Canonicalization, Fallback & Edge Cases

### 3.1 `canonicalize()` vs `exists()` vs Raw PathBuf Analysis

| Strategy | Behavior if Path Exists | Behavior if Path Missing | Symlink & Traversal Safety | Impact on Test Suites |
|---|---|---|---|---|
| **A. Strict `canonicalize()?`** | Resolves to canonical absolute path. | **Fails startup with `io::Error`** (crashes process). | Resolves symlinks. | **BREAKS CI/TESTS** when tests run before `npm run build`. |
| **B. Raw `PathBuf` only** | Retains path as passed. | Does not fail startup; 404 on HTTP requests. | Relies on OS cwd. | Fragile if cwd differs or changes at runtime. |
| **C. Best-Effort `canonicalize()` with Fallback** (*Recommended*) | Resolves to canonical absolute path. | **Succeeds**, falls back to uncanonicalized `raw`. | Resolves symlinks; safe. | **ZERO REGRESSIONS**: tests without frontend pass cleanly. |

### 3.2 Recommended Resolution Logic
```rust
pub fn configured_web_dist() -> std::path::PathBuf {
    let raw = web_dist_path();
    raw.canonicalize().unwrap_or(raw)
}
```
**Why this is optimal:**
1. **Absolute path stability:** When the directory exists (e.g. `/app/apps/web/build` in Docker, or `c:\...\apps\web\build` locally), `canonicalize()` locks in an absolute path at startup. Subsequent changes to the process working directory will not break asset serving.
2. **Resilience during testing:** If `apps/web/build` has not been compiled yet (or during non-web unit tests), `canonicalize()` returns `Err(NotFound)`. `unwrap_or(raw)` keeps the uncanonicalized `PathBuf`. The server boots successfully; non-app endpoints (`/api/v1/...`, healthcheck, catalog) function normally, and `/app` simply returns HTTP `404 Not Found`.
3. **Symlink resolution:** Resolves symlinks safely to prevent circular reference bugs.

### 3.3 Traversal & Security Protection
- `tower_http::services::ServeDir` has built-in path sanitization:
  - Percent-decodes paths safely.
  - Automatically rejects any path segment containing `..` (`Component::ParentDir`) with `404 Not Found`.
  - HTTP requests cannot traverse outside the canonicalized root directory.
- `local_security` wraps `/app`:
  - Enforces `Host`, `Origin`, and rejects cross-site fetch requests.
  - Applies strict CSP (`default-src 'none'; script-src 'self' ...`).
  - Sets `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`.

### 3.4 Operational Warning in `main.rs`
In `crates/core/src/main.rs`, when running the standalone binary:
```rust
let web_dist = media_poc::configured_web_dist();
if !web_dist.exists() {
    eprintln!(
        "Warning: Static web assets not found at '{}'. /app routes will return 404.",
        web_dist.display()
    );
}
```
This alerts container operators immediately in `docker logs` if they forgot to mount or copy the frontend assets, without crashing the daemon or triggering Docker restart loops.

---

## 4. Assessment of Existing Tests for Static Serving under `/app`

### 4.1 Exhaustive Audit of Test Suites
A complete audit of `crates/core/tests/` and `scripts/` revealed:
1. **Unit and Integration Tests (`crates/core/tests/*.rs`):**
   - **ZERO** active (non-ignored) tests send HTTP requests to `/app` or test `ServeDir`.
   - The test suites focus on `identity`, `permissions`, `catalog`, `scanner_flow`, `nfo_flow`, `posters_flow`, `p0` (AV1/Opus playback), and `core_restart`.
2. **End-to-End Playwright Tests (`crates/core/tests/browser_*.rs`):**
   - Four browser test files exist:
     - `browser_auth.rs`
     - `browser_scanner.rs`
     - `browser_nfo.rs`
     - `browser_posters.rs`
   - All four are annotated with `#[ignore = "..."]` (requires built Web assets, generated fixtures, and installed Chrome/Playwright).
   - In `browser_auth.rs:28-33`, the test author explicitly documented the working directory workaround:
     ```rust
     // Static file service resolves from repository root.
     let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
         .join("../..")
         .canonicalize()
         .unwrap();
     // Run this test alone: ServeDir uses the process working directory.
     std::env::set_current_dir(&root).unwrap();
     ```
   - In `browser_scanner.rs:8`, the child process `media-poc` is explicitly launched with `.current_dir(root)`.

### 4.2 Gaps Identified
- No automated regression tests verify that `/app/` serves `index.html`.
- No tests verify `MEDIA_WEB_DIST` configuration overriding.
- No tests verify HTTP headers (CSP, Cache-Control, MIME types) on static assets.
- No tests verify 404 behavior when an asset is missing.

---

## 5. Safe Implementation Strategy and Code Diff Proposal

### 5.1 Proposed Changes to `crates/core/src/lib.rs`

```diff
--- a/crates/core/src/lib.rs
+++ b/crates/core/src/lib.rs
@@ -147,12 +147,23 @@ pub fn app(inventory: node::Inventory, port: u16) -> Router {
     )
 }
 
+/// Resolve the static web distribution directory from `MEDIA_WEB_DIST`.
+/// Defaults to `"apps/web/build"` if unset or empty/whitespace.
+/// Best-effort canonicalizes if the directory exists on disk.
+pub fn configured_web_dist() -> std::path::PathBuf {
+    let raw = match std::env::var("MEDIA_WEB_DIST") {
+        Ok(val) if !val.trim().is_empty() => std::path::PathBuf::from(val.trim()),
+        _ => std::path::PathBuf::from("apps/web/build"),
+    };
+    raw.canonicalize().unwrap_or(raw)
+}
+
 fn public_routes(mode: &'static str) -> Router {
     Router::new()
         .nest_service(
             "/app",
-            tower_http::services::ServeDir::new("apps/web/build")
+            tower_http::services::ServeDir::new(configured_web_dist())
                 .append_index_html_on_directories(true),
         )
         .route(
```

### 5.2 Proposed Changes to `crates/core/src/main.rs`

```diff
--- a/crates/core/src/main.rs
+++ b/crates/core/src/main.rs
@@ -30,6 +30,13 @@ async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
     let auth = AuthState::new(database.clone(), public_origin).await?;
     println!("Authenticated local Core: http://127.0.0.1:{port}/app/");
+    let web_dist = media_poc::configured_web_dist();
+    if !web_dist.exists() {
+        eprintln!(
+            "Warning: Static web assets not found at '{}'. /app routes will return 404.",
+            web_dist.display()
+        );
+    }
     let jobs = std::sync::Arc::new(ScanJobs::default());
     let shutdown_jobs = jobs.clone();
     axum::serve(
```

### 5.3 Proposed New Integration Test: `crates/core/tests/static_web.rs`

```rust
//! Integration test for static asset serving under /app and MEDIA_WEB_DIST support.
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use media_poc::{
    auth::AuthState, configured_web_dist, identity::IdentityStore, node::Inventory, secure_app,
};
use tower::ServiceExt;
use zeroize::Zeroizing;

#[tokio::test]
async fn static_web_serves_custom_dist_and_applies_security_headers() {
    let temp = tempfile::tempdir().unwrap();
    let web_dist = temp.path().join("custom_dist");
    std::fs::create_dir_all(web_dist.join("login")).unwrap();
    std::fs::write(
        web_dist.join("index.html"),
        "<!doctype html><html><body>Aether Test Index</body></html>",
    )
    .unwrap();
    std::fs::write(
        web_dist.join("login").join("index.html"),
        "<!doctype html><html><body>Aether Test Login</body></html>",
    )
    .unwrap();

    // Verify configured_web_dist with explicit MEDIA_WEB_DIST
    unsafe {
        std::env::set_var("MEDIA_WEB_DIST", &web_dist);
    }
    assert_eq!(configured_web_dist(), web_dist.canonicalize().unwrap());

    let db_path = temp.path().join("test_static.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    store
        .bootstrap("test-admin", Zeroizing::new("disposable test pass".into()))
        .await
        .unwrap();

    let origin = "http://127.0.0.1:3000".to_string();
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::load(media_dir).await.unwrap();

    let router = secure_app(inventory, 3000, auth).await.unwrap();

    // 1. Request to /app/ -> returns 200 OK with custom index.html content
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    assert_eq!(
        res.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert!(
        res.headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("default-src 'none'")
    );

    // 2. Request to /app/login/ -> returns 200 OK
    let req = Request::builder()
        .uri("/app/login/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 3. Request to missing file -> returns 404 Not Found
    let req = Request::builder()
        .uri("/app/nonexistent.js")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    store.close().await;
}
```

---

## 6. Verification Plan
- Unit verification: `cargo check` and `cargo test` across all existing targets.
- Environmental verification: Ensure `MEDIA_WEB_DIST=""` or unset defaults strictly to `"apps/web/build"`.
- Container layout verification: In Milestone 2 (`Dockerfile`), configure `ENV MEDIA_WEB_DIST=/app/apps/web/build` or rely on `WORKDIR /app` + default `"apps/web/build"`.
