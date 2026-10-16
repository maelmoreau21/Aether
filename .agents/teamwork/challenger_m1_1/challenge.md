# Challenge Report: Milestone 1 Configuration & Static Web Stress Testing

## Challenge Summary

**Overall risk assessment**: LOW (All tested hypotheses and adversarial attack scenarios verified; implementation is resilient and fail-closed)
**Verdict**: **APPROVE**

---

## 1. MEDIA_BIND_ADDR Stress Testing & Permutations

### Tested Hypotheses & Attack Scenarios

| Scenario | Input | Expected Result | Actual Result | Status |
|---|---|---|---|---|
| Unset variable | `Err(VarError::NotPresent)` | Default to `127.0.0.1` (`Ipv4Addr::LOCALHOST`) | `127.0.0.1` | **PASS** |
| Empty string | `""` | Default to `127.0.0.1` | `127.0.0.1` | **PASS** |
| Whitespace only | `"   \t \r\n "` | Default to `127.0.0.1` | `127.0.0.1` | **PASS** |
| Standard loopback | `"127.0.0.1"` | Parse to `127.0.0.1`, bind listener | `127.0.0.1`, bound | **PASS** |
| All interfaces | `"0.0.0.0"` | Parse to `0.0.0.0`, bind listener | `0.0.0.0`, bound | **PASS** |
| Secondary loopback | `"127.0.0.2"` | Parse to `127.0.0.2`, bind listener | `127.0.0.2`, bound | **PASS** |
| Whitespace padding | `"  127.0.0.1  "` | Trim whitespace, parse to `127.0.0.1` | `127.0.0.1`, bound | **PASS** |
| Out of range octet | `"999.999.999.999"` | Fail fast, exit 1, error logged | Exit code 1, `invalid MEDIA_BIND_ADDR '999.999.999.999'` | **PASS** |
| Out of range single octet | `"256.0.0.1"` | Fail fast, exit 1, error logged | Exit code 1, `invalid MEDIA_BIND_ADDR '256.0.0.1'` | **PASS** |
| Hostname instead of IP | `"localhost"` | Refuse hostname (prevent DNS rebinding in bind), exit 1 | Exit code 1, `invalid MEDIA_BIND_ADDR 'localhost'` | **PASS** |
| IPv6 format | `"::1"` | Enforce IPv4 contract, exit 1 | Exit code 1, `invalid MEDIA_BIND_ADDR '::1'` | **PASS** |
| Embedded port | `"127.0.0.1:3000"` | Reject embedded port (separate from `MEDIA_PORT`), exit 1 | Exit code 1, `invalid MEDIA_BIND_ADDR '127.0.0.1:3000'` | **PASS** |
| Subnet CIDR notation | `"127.0.0.1/24"` | Reject CIDR mask, exit 1 | Exit code 1, `invalid MEDIA_BIND_ADDR '127.0.0.1/24'` | **PASS** |
| Negative numbers | `"-1.0.0.1"` | Reject negative, exit 1 | Exit code 1, `invalid MEDIA_BIND_ADDR '-1.0.0.1'` | **PASS** |
| Non-numeric string | `"invalid-ip-string"` | Reject non-numeric, exit 1 | Exit code 1, `invalid MEDIA_BIND_ADDR 'invalid-ip-string'` | **PASS** |

### Findings & Architecture Invariants
- `parse_bind_addr()?` is evaluated at lines 41 of `crates/core/src/main.rs`, **strictly prior** to database connection opening (`IdentityStore::open`) or filesystem library inventory scanning (`Inventory::local` / `Inventory::load`).
- This guarantees fail-fast execution: no persistent state is touched, no locks are acquired, and no partial processes linger if an invalid bind address is supplied.
- Tested TCP socket binding using `tokio::net::TcpListener::bind(SocketAddrV4::new(ip, 0))` on Windows with `127.0.0.1`, `0.0.0.0`, and `127.0.0.2` succeeded with 100% reliability.

---

## 2. MEDIA_WEB_DIST Stress Testing & Permutations

### Tested Hypotheses & Attack Scenarios

| Scenario | Input | Expected Result | Actual Result | Status |
|---|---|---|---|---|
| Unset variable | `None` | Default to `"apps/web/build"`, canonicalized if exists | Defaulted & canonicalized | **PASS** |
| Empty string | `Some("")` | Default to `"apps/web/build"`, canonicalized if exists | Defaulted & canonicalized | **PASS** |
| Whitespace only | `Some("   \t  ")` | Default to `"apps/web/build"`, canonicalized if exists | Defaulted & canonicalized | **PASS** |
| Non-existent directory | `Some("nonexistent/custom/path")` | Fall back to raw PathBuf without panic or crash | Returned raw PathBuf | **PASS** |
| Existing directory | `Some("custom_dir")` | Canonicalized absolute path | Canonicalized absolute path | **PASS** |
| Directory with spaces | `Some("dist with spaces in folder name")` | Preserve internal spaces, canonicalized | Resolved & canonicalized | **PASS** |
| Directory with outer padding | `Some("  custom_dir  ")` | Trim outer padding, canonicalized | Resolved & canonicalized | **PASS** |
| Relative path | `Some("target/tmp_rel_test_dist")` | Resolve to absolute canonical path | Resolved to absolute path | **PASS** |
| Absolute path | `Some("C:\\...")` | Canonicalized absolute path | Canonicalized absolute path | **PASS** |

---

## 3. Static Web Serving Behavior under `/app`

### Attack Scenarios & Boundary Conditions

1. **Root & Nested Serving**:
   - `GET /app/` with valid `Host` -> Returns `200 OK` with root `index.html` content.
   - `GET /app/nested/` with valid `Host` -> Returns `200 OK` with nested `index.html` content.
   - `GET /app/file%20with%20space.css` -> Returns `200 OK` with CSS asset content.
2. **Missing Files**:
   - `GET /app/nonexistent_file.js` -> Returns `404 Not Found` cleanly.
3. **Directory Traversal Attack Resistance**:
   - Attempt: `GET /app/../secret.txt` attempting to read sensitive file outside web distribution root.
   - Result: Request does NOT return 200 OK and never leaks `SUPER_SECRET_TOKEN`. `tower-http::services::ServeDir` restricts access strictly within the root directory.
   - Attempt: Encoded traversal `GET /app/%2e%2e/secret.txt`.
   - Result: Traversal blocked cleanly.
4. **Missing Distribution Directory Resilience**:
   - When `MEDIA_WEB_DIST` points to a non-existent path:
   - Server boots successfully, logs warning (`Warning: Static web assets not found at '...'. /app routes will return 404.`).
   - `GET /app/` returns `404 Not Found` without server panic or unhandled error.
5. **Security Wrapper Invariants**:
   - All `/app` routes are wrapped by `local_security`:
     - `Host: evil-domain.com` -> Returns `403 Forbidden`.
     - `sec-fetch-site: cross-site` -> Returns `403 Forbidden`.
     - Responses contain mandatory security headers:
       - `X-Content-Type-Options: nosniff`
       - `Cache-Control: no-store`
       - `Content-Security-Policy: default-src 'none'; script-src 'self'; ...`

---

## 4. Test Suite & Code Hygiene Verification

- `cargo fmt --check`: PASSED (exit code 0).
- `cargo clippy --all-targets -- -D warnings`: PASSED (exit code 0, 0 warnings).
- `cargo test --bin media-poc`: PASSED (3 unit tests passed).
- `cargo test --lib media_poc -- tests::configured_web_dist`: PASSED (3 unit tests passed).
- `cargo test --test static_web --test healthcheck_resilience --test adversarial_challenge`: PASSED (7 integration tests passed).

---

## Conclusion & Verdict

The implementation in `crates/core/src/main.rs` and `crates/core/src/lib.rs` satisfies all edge case criteria, robustly handles whitespace padding and invalid IP formats, protects against directory traversal, and enforces security boundaries on `/app`.

**VERDICT: APPROVE**
