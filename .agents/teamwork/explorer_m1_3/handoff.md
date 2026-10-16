# Milestone 1 Handoff Report: Resilient Healthcheck and Security Layer

## 1. Observation

1. **Routing and Wrapping in `crates/core/src/lib.rs` (lines 111-118, 150-167, 169-188)**:
   - `public_routes` declares `/api/v1/health`:
     ```rust
     .route(
         "/api/v1/health",
         get(move || async move { Json(serde_json::json!({"status":"ok", "mode":mode})) }),
     )
     ```
   - In `secure_app_with_jobs`, `public_routes(mode)` is merged with `media`, `scans`, `libraries`, and `auth`, and passed into `local_security`:
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
   - `local_security` extracts the authority from `origin` into `host`:
     ```rust
     let host = origin
         .parse::<axum::http::Uri>()
         .ok()
         .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
     ```
   - In the middleware layer (lines 179-185):
     ```rust
     if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != host.as_deref()
         || req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site")
         || req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != origin.as_bytes())
     {
         return StatusCode::FORBIDDEN.into_response();
     }
     ```

2. **Configuration in `crates/core/src/main.rs` (lines 28-30)**:
   - `MEDIA_PUBLIC_URL` defines `public_origin`:
     ```rust
     let public_origin =
         std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
     let auth = AuthState::new(database.clone(), public_origin).await?;
     ```
   - When `MEDIA_PUBLIC_URL="https://nas.example.com"`, `origin` is `"https://nas.example.com"` and `host` is `"nas.example.com"`.

3. **Internal Container Probe Behavior**:
   - Docker container health checks or internal probes execute `curl http://127.0.0.1:3000/api/v1/health`.
   - The probe sends `Host: 127.0.0.1:3000`.
   - In `local_security`, `req.headers().get(header::HOST).and_then(|h| h.to_str().ok())` (`"127.0.0.1:3000"`) != `host.as_deref()` (`"nas.example.com"`).
   - This comparison evaluates to `true`, immediately executing `return StatusCode::FORBIDDEN.into_response();`.
   - The health probe receives `403 Forbidden` and Docker marks the container `unhealthy`.

4. **Security Protection on Mutating & Protected Endpoints**:
   - `crates/core/src/auth.rs`: `login` (line 172) and `logout` (line 120) enforce `check_origin(&headers)` and `require_csrf`.
   - `crates/core/src/scans.rs`: line 95 enforces `state.auth.require_csrf(headers, &identity)`.
   - `crates/core/src/libraries.rs`: admin handlers enforce `require_csrf`.
   - `crates/core/src/permissions.rs`: `require_media` enforces session and RBAC before any media/catalog access.
   - `crates/core/tests/permissions.rs`: lines 265-281 verify that requests with mismatched `Host` receive `403 Forbidden`.

## 2. Logic Chain

1. Step 1 (from Observation 1 & 2): `local_security` enforces that the `Host` HTTP header must equal `origin.authority()`.
2. Step 2 (from Observation 3): When `MEDIA_PUBLIC_URL` is set to an external domain (e.g., `https://nas.example.com`), `origin.authority()` is `nas.example.com`.
3. Step 3 (from Observation 3): A container internal healthcheck probe (e.g., `curl http://127.0.0.1:3000/api/v1/health`) connects directly to loopback and issues `Host: 127.0.0.1:3000`.
4. Step 4 (from Observation 1 & 3): Because `/api/v1/health` is currently wrapped inside `local_security`, the `Host` check fails (`"127.0.0.1:3000" != "nas.example.com"`), returning `403 Forbidden`.
5. Step 5 (from Observation 4): `/api/v1/health` is an unauthenticated, read-only readiness probe returning only `{"status":"ok", "mode":mode}`. It contains no secrets, user data, or mutable state.
6. Step 6: Decoupling `/api/v1/health` into a separate Axum `health_routes` router merged outside `local_security` allows internal probes to succeed regardless of the incoming `Host` header (supporting loopback `127.0.0.1`, `localhost`, container bridge IPs, or external FQDNs).
7. Step 7 (from Observation 1 & 4): Because `local_security` continues to wrap all other routes (`/app`, `/api/v1/config`, `/api/v1/auth/*`, `/api/v1/catalog/*`, `/api/v1/assets/*`, `/api/v1/admin/*`), all Host, Origin, CSRF, and CSP protections remain fully enforced on all sensitive, catalog, and streaming endpoints.

## 3. Caveats

- **No Caves / In-Scope Scope**: The investigation is read-only; no production files were modified directly.
- **Alternative Interpretations Considered**:
  - *Option B (Middleware conditional bypass)*: Adding `if req.uri().path() == "/api/v1/health"` with loopback string matching inside `local_security`. Rejected because it creates an Axum anti-pattern, incurs per-request string comparison overhead on every server route, and breaks if Docker probes via container IP or alias.
  - *Option C (Hybrid cross-site block)*: Attaching `sec-fetch-site != cross-site` to `health_routes`. Documented as an optional defense-in-depth enhancement, though standard CORS SOP already prevents external browser reading of the unauthenticated health payload.

## 4. Conclusion

The recommended, cleanest, and safest fix is **Architectural Route Separation**:
1. Move `/api/v1/health` out of `public_routes` into a standalone `health_routes(mode: &'static str) -> Router` decorated with `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.
2. In `crates/core/src/lib.rs` (`secure_app_with_jobs` and `app`), merge `health_routes(mode)` at the top level with `local_security(...)`.
3. This completely resolves the Docker healthcheck 403 failure under any `MEDIA_PUBLIC_URL` setting while keeping all security invariants (Host validation, Origin enforcement, CSRF tokens, session integrity, and CSP) 100% active on all protected routes.

## 5. Verification Method

1. **Codebase Inspection**:
   - Inspect `crates/core/src/lib.rs` at lines 111-118, 143-148, 150-167, and 169-188.
2. **Automated Test Command**:
   - Run `cargo test --test permissions` and `cargo test --test p0`.
3. **Targeted Verification Scenario**:
   - Initialize `AuthState` with `https://media.example.invalid`.
   - Issue `GET /api/v1/health` with `Host: 127.0.0.1:3000` -> Expect `200 OK`.
   - Issue `GET /api/v1/health` with `Host: media.example.invalid` -> Expect `200 OK`.
   - Issue `GET /api/v1/catalog` with `Host: 127.0.0.1:3000` -> Expect `403 Forbidden`.
   - Issue `POST /api/v1/auth/login` with `Host: 127.0.0.1:3000` -> Expect `403 Forbidden`.
4. **Invalidation Conditions**:
   - If `/api/v1/catalog` or `/api/v1/auth/login` accepts a mismatched `Host`, the security invariant has been violated.
   - If `GET /api/v1/health` with `Host: 127.0.0.1:3000` returns `403 Forbidden` when `MEDIA_PUBLIC_URL` is set to an external origin, the healthcheck resilience requirement has not been met.
