# Empirical Security Challenge Report: Milestone 1 (Healthcheck & Local Security Layer)

**Challenger**: Challenger 2 (Milestone 1)  
**Date**: 2026-10-16  
**Target Code**: `crates/core/src/lib.rs`, `crates/core/src/main.rs`, `crates/core/src/auth.rs`, `crates/core/src/libraries.rs`, `crates/core/src/scans.rs`  
**Test Suite**: `crates/core/tests/challenger_security_challenge.rs`  
**Verdict**: **APPROVE**

---

## 1. Challenge Summary

**Overall risk assessment**: **LOW**

The security boundaries implemented in Milestone 1 decouple `/api/v1/health` from `local_security` while maintaining strict fail-closed boundary enforcement on 100% of protected routes. CSRF protection, constant-time token comparison, Origin matching, and `Sec-Fetch-Site` restrictions on mutating endpoints remain intact and unaffected.

---

## 2. Challenges & Attack Hypotheses Tested

### [High] Challenge 1: Healthcheck Route Leaks Internal State or Sensitive Credentials Under Host Permutations
- **Assumption challenged**: Separating `/api/v1/health` from `local_security` might allow attackers to probe internal system state or leak configuration via custom/malformed Host headers or query parameters.
- **Attack scenario**: Attacker sends requests to `/api/v1/health` with external domains, missing `Host`, arbitrary IP/ports, IPv6 loopback, giant 16KB headers, query parameters (`?token=secret`), or mutating methods (POST/PUT/DELETE).
- **Result**: **PASS**.
  - All valid GET requests return HTTP 200 OK with headers `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.
  - The payload is strictly minimal: `{"status":"ok","mode":"standalone-local"}` (exactly 2 JSON keys).
  - No database paths, secrets, passwords, usernames, tokens, or stack traces are present.
  - Query parameters are ignored and not reflected.
  - Mutating methods (POST, PUT, DELETE, PATCH) are strictly rejected with HTTP 405 Method Not Allowed.

### [Critical] Challenge 2: Local Security Boundary Bypass on Protected Routes via DNS Rebinding, Suffix/UserInfo Tricks, or Header Spoofing
- **Assumption challenged**: `local_security` might be bypassed by DNS rebinding, subdomain/suffix confusion (`127.0.0.1:3000.attacker.com`), userinfo (`127.0.0.1:3000@attacker.com`), `X-Forwarded-Host` injection, or trailing slash normalization.
- **Attack scenario**: Attacker uses a browser or forged HTTP client targeting `/app/`, `/api/v1/catalog`, `/api/v1/config`, `/api/v1/auth/login`, `/api/v1/admin/libraries`, `/api/v1/assets`, or `/api/v1/admin/libraries/{id}/scan` with:
  1. Mismatched Host (`evil.attacker.com`, `evil.attacker.com:3000`)
  2. Suffix domain (`127.0.0.1:3000.attacker.com`)
  3. Userinfo domain (`127.0.0.1:3000@attacker.com`)
  4. Host port mismatch (`127.0.0.1:9999`) or missing port (`127.0.0.1`)
  5. Localhost alias mismatch (`localhost:3000` when origin authority is `127.0.0.1:3000`)
  6. Missing `Host` header
  7. Spoofed `X-Forwarded-Host: 127.0.0.1:3000` and `X-Forwarded-For: 127.0.0.1`
  8. Cross-site `Sec-Fetch-Site: cross-site`
  9. Invalid `Origin` (`http://evil.attacker.com`, `https://evil.attacker.com`, `http://127.0.0.1:3001`, `https://127.0.0.1:3000`, or `http://127.0.0.1:3000/`)
  10. Streaming Range request on `/api/v1/assets/{id}/content` with mismatched Host
  11. CORS preflight (`OPTIONS /api/v1/catalog`) with evil Origin
- **Result**: **PASS**.
  - **100% of tested protected routes strictly rejected every single attack vector with HTTP 403 Forbidden**.
  - `local_security` checks exact string equality against authority extracted from canonical origin. Subdomain suffixes, userinfo, and port mismatches fail exact equality.
  - Reverse proxy headers (`X-Forwarded-Host`, `X-Forwarded-For`) are completely ignored by `local_security`, defeating proxy header injection attacks.

### [Critical] Challenge 3: CSRF & Origin Protections on Mutating Operations
- **Assumption challenged**: Decoupling `/api/v1/health` or refactoring router assembly might degrade CSRF token validation on mutating endpoints (`/api/v1/auth/login`, `/api/v1/auth/logout`, `/api/v1/admin/libraries`, `/api/v1/admin/libraries/{id}/scan`).
- **Attack scenario**:
  - Login without `Origin`, with `sec-fetch-site: cross-site`, or with untrusted `Origin`.
  - Logout without `x-csrf-token`, with wrong 64-char hex token, with short/malformed token, with cross-site `Sec-Fetch-Site`, or with untrusted `Origin`.
  - Admin library creation and updates without `x-csrf-token` or with invalid token.
  - Triggering library scans without `x-csrf-token` or with invalid token.
- **Result**: **PASS**.
  - `login` strictly requires `Origin == state.origin` and `sec-fetch-site != "cross-site"`, returning 403 Forbidden otherwise.
  - `logout` requires valid 64-char SHA-256 CSRF token matching the session identity via constant-time comparison. Missing, short, or mismatched tokens immediately return 403 Forbidden. Valid logout revokes the session in SQLite, and subsequent authenticated calls return 401 Unauthorized.
  - `admin/libraries` and `admin/libraries/{id}/scan` require administrator role and enforce `state.auth.require_csrf(...)`, returning 403 Forbidden on missing or invalid tokens.

### [Medium] Challenge 4: Reverse Proxy Origin (`MEDIA_PUBLIC_URL`) Isolation
- **Assumption challenged**: When `MEDIA_PUBLIC_URL` is set to an external domain (e.g. `https://nas.example.invalid`), Docker container internal loopback probes might mistakenly be allowed to access protected routes, or external requests might be blocked from health checks.
- **Attack scenario**: Configure origin to `https://nas.example.invalid`:
  1. Internal healthcheck probe with `Host: 127.0.0.1:3000` -> Expected 200 OK.
  2. Protected routes (`/app/`, `/api/v1/catalog`, `/api/v1/config`, `/api/v1/admin/libraries`) with loopback `Host: 127.0.0.1:3000` -> Expected 403 Forbidden.
  3. Reverse proxy traffic with `Host: nas.example.invalid` -> Expected 200 OK.
- **Result**: **PASS**.
  - Loopback health probe succeeds (200 OK).
  - All protected routes fail-closed with 403 Forbidden for internal loopback Host.
  - External reverse proxy traffic matching the configured authority succeeds normally.

---

## 3. Stress Test Results Matrix

| # | Test Scenario | Expected Behavior | Actual Behavior | Result |
|---|---------------|-------------------|-----------------|--------|
| 1 | `GET /api/v1/health` with `Host: 127.0.0.1:3000` | 200 OK, nosniff, no-store, 2 JSON keys | 200 OK, nosniff, no-store, status=ok, mode=standalone-local | PASS |
| 2 | `GET /api/v1/health` with `Host: localhost:3000` | 200 OK | 200 OK | PASS |
| 3 | `GET /api/v1/health` with `Host: nas.example.invalid` | 200 OK | 200 OK | PASS |
| 4 | `GET /api/v1/health` with `Host: evil.attacker.com` | 200 OK | 200 OK | PASS |
| 5 | `GET /api/v1/health` with missing `Host` | 200 OK | 200 OK | PASS |
| 6 | `GET /api/v1/health` with `Host: 192.168.1.100:9999` | 200 OK | 200 OK | PASS |
| 7 | `GET /api/v1/health` with 16KB giant `Host` | 200 OK | 200 OK | PASS |
| 8 | `GET /api/v1/health?debug=true&token=secret` | 200 OK, query not reflected | 200 OK, query ignored | PASS |
| 9 | `POST /api/v1/health` | 405 Method Not Allowed | 405 Method Not Allowed | PASS |
| 10 | `GET /app/` with mismatched Host | 403 Forbidden | 403 Forbidden | PASS |
| 11 | `GET /api/v1/catalog` with mismatched Host | 403 Forbidden | 403 Forbidden | PASS |
| 12 | `GET /api/v1/config` with mismatched Host | 403 Forbidden | 403 Forbidden | PASS |
| 13 | `POST /api/v1/auth/login` with mismatched Host | 403 Forbidden | 403 Forbidden | PASS |
| 14 | `GET /api/v1/admin/libraries` with mismatched Host | 403 Forbidden | 403 Forbidden | PASS |
| 15 | `POST /api/v1/admin/libraries/test/scan` with mismatched Host | 403 Forbidden | 403 Forbidden | PASS |
| 16 | Protected routes with `127.0.0.1:3000.attacker.com` | 403 Forbidden | 403 Forbidden | PASS |
| 17 | Protected routes with `127.0.0.1:3000@attacker.com` | 403 Forbidden | 403 Forbidden | PASS |
| 18 | Protected routes with `Host: evil.com` + `X-Forwarded-Host: 127.0.0.1:3000` | 403 Forbidden | 403 Forbidden | PASS |
| 19 | Protected routes with `sec-fetch-site: cross-site` | 403 Forbidden | 403 Forbidden | PASS |
| 20 | Protected routes with `Origin: http://evil.attacker.com` | 403 Forbidden | 403 Forbidden | PASS |
| 21 | Protected routes with `Origin: http://127.0.0.1:3000/` (trailing slash) | 403 Forbidden | 403 Forbidden | PASS |
| 22 | `GET /api/v1/assets/test/content` (Range) with bad Host | 403 Forbidden | 403 Forbidden | PASS |
| 23 | `OPTIONS /api/v1/catalog` with evil Origin | 403 Forbidden | 403 Forbidden | PASS |
| 24 | `POST /api/v1/auth/login` without Origin | 403 Forbidden | 403 Forbidden | PASS |
| 25 | `POST /api/v1/auth/login` with `sec-fetch-site: cross-site` | 403 Forbidden | 403 Forbidden | PASS |
| 26 | `POST /api/v1/auth/logout` without CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 27 | `POST /api/v1/auth/logout` with wrong 64-char CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 28 | `POST /api/v1/auth/logout` with malformed length CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 29 | `POST /api/v1/auth/logout` with cross-site `Sec-Fetch-Site` | 403 Forbidden | 403 Forbidden | PASS |
| 30 | `POST /api/v1/auth/logout` with evil Origin | 403 Forbidden | 403 Forbidden | PASS |
| 31 | `POST /api/v1/admin/libraries` without CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 32 | `POST /api/v1/admin/libraries` with bad CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 33 | `PUT /api/v1/admin/libraries/{id}` without CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 34 | `POST /api/v1/admin/libraries/{id}/scan` without CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 35 | `POST /api/v1/admin/libraries/{id}/scan` with bad CSRF token | 403 Forbidden | 403 Forbidden | PASS |
| 36 | Valid mutating PUT library + valid CSRF + matching Origin | 200 OK | 200 OK | PASS |
| 37 | Valid logout + valid CSRF + matching Origin | 204 No Content | 204 No Content | PASS |
| 38 | Post-logout request with invalidated cookie | 401 Unauthorized | 401 Unauthorized | PASS |
| 39 | Reverse proxy mode: internal probe `/api/v1/health` with `Host: 127.0.0.1:3000` | 200 OK | 200 OK | PASS |
| 40 | Reverse proxy mode: internal probe protected routes with `Host: 127.0.0.1:3000` | 403 Forbidden | 403 Forbidden | PASS |
| 41 | Reverse proxy mode: external traffic with matching `Host: nas.example.invalid` | 200 OK | 200 OK | PASS |

Total stress tests executed: **41 scenarios**  
Total passed: **41** (100% pass rate)

---

## 4. Unchallenged Areas

- **Docker daemon network namespace bridging**: Physical packet capture inside Docker Linux network namespaces was not run directly here (Docker containerization belongs to Milestones 2 & 3). However, all HTTP level socket and header handling was verified empirically under Axum/Tower.
- **Browser Playwright E2E**: Playwright browser tests were ignored as per project build defaults (require built Web assets and Chrome binary); verified at HTTP layer via Axum oneshot service execution with exact browser headers (`Sec-Fetch-Site`, `Origin`, `Host`, `Cookie`).

---

## 5. Verdict

**APPROVE**. The healthcheck and local security layer meets and exceeds all Milestone 1 security requirements.
