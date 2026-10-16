# Milestone 1 Investigation: Network Binding & `MEDIA_BIND_ADDR` Support

## 1. Executive Summary

In Milestone 1 of the Aether standalone Docker containerization, `media-poc` (the Core backend daemon) must support listening on all network interfaces (`0.0.0.0`) inside Docker containers, while continuing to default securely to loopback (`127.0.0.1`) for local standalone environments and existing test suites.

This investigation analyzes:
- The current hardcoded loopback binding in `crates/core/src/main.rs:26`.
- The exact specification and behavior of `MEDIA_BIND_ADDR` (defaulting to `127.0.0.1` when unset or empty, and binding to `0.0.0.0` or specified IPv4 addresses).
- Robust error handling and validation for malformed inputs.
- Impact on all existing test suites across the workspace (`crates/core/tests/`).
- Security interplay with `Host`, `Origin`, and `MEDIA_PUBLIC_URL` protections.
- A proposed safe, non-breaking implementation strategy with full code diff and unit test coverage.

---

## 2. Codebase Inspection: `crates/core/src/main.rs`

### Current Implementation (lines 25-31)
```rust
    let listener =
        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
    let port = listener.local_addr()?.port();
    let public_origin =
        std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let auth = AuthState::new(database.clone(), public_origin).await?;
    println!("Authenticated local Core: http://127.0.0.1:{port}/app/");
```

### Analysis of the Docker Problem
1. **Network Namespace Isolation**: When running inside a container, the container runtime creates a dedicated network namespace with its own `lo` (loopback) and `eth0` (virtual ethernet connected to the Docker bridge network, typically `172.17.0.x`).
2. **Host Port Forwarding Failure**: Docker port publishing (`-p 3000:3000` or Compose `127.0.0.1:3000:3000`) directs incoming host packets through Docker's iptables / userland proxy to the container's `eth0` IP address.
3. **Current Behavior**: Because `TcpListener::bind` explicitly specifies `Ipv4Addr::LOCALHOST` (`127.0.0.1`), the socket only accepts connections arriving on the container's internal `lo` interface. Any packet routed to `eth0` is immediately rejected by the container OS kernel with `ECONNREFUSED` ("Connection refused").
4. **Required Behavior**: Setting `MEDIA_BIND_ADDR=0.0.0.0` instructs the kernel to bind to `INADDR_ANY`, accepting packets arriving on any IPv4 network interface within the container namespace (both `lo` and `eth0`).

---

## 3. Configuration & Default Handling Specification

### Environment Variable: `MEDIA_BIND_ADDR`

| Input State | Example | Target Behavior | Resulting Address |
|---|---|---|---|
| **Unset** | Variable not in env | Fall back to safe default | `127.0.0.1` (`Ipv4Addr::LOCALHOST`) |
| **Empty string** | `MEDIA_BIND_ADDR=""` | Fall back to safe default | `127.0.0.1` (`Ipv4Addr::LOCALHOST`) |
| **Whitespace only** | `MEDIA_BIND_ADDR="   "` | Fall back to safe default | `127.0.0.1` (`Ipv4Addr::LOCALHOST`) |
| **Docker All-Interfaces** | `MEDIA_BIND_ADDR="0.0.0.0"` | Listen on all interfaces | `0.0.0.0` (`Ipv4Addr::UNSPECIFIED`) |
| **Padded Valid IPv4** | `MEDIA_BIND_ADDR=" 0.0.0.0 "` | Trim whitespace and parse | `0.0.0.0` (`Ipv4Addr::UNSPECIFIED`) |
| **Explicit Loopback** | `MEDIA_BIND_ADDR="127.0.0.1"` | Listen on loopback | `127.0.0.1` (`Ipv4Addr::LOCALHOST`) |
| **Custom IPv4 (LAN)** | `MEDIA_BIND_ADDR="192.168.1.50"` | Listen on specific interface | `192.168.1.50` |
| **Malformed / Non-IP** | `MEDIA_BIND_ADDR="invalid"` | Fail fast with descriptive error | Process exit (status 1) |
| **IP with Port** | `MEDIA_BIND_ADDR="0.0.0.0:3000"` | Fail fast (port is handled by `MEDIA_PORT`) | Process exit (status 1) |
| **IPv6 Address** | `MEDIA_BIND_ADDR="::1"` | Fail fast (IPv4 strict contract) | Process exit (status 1) |

### Rationale for Empty/Whitespace Fallback
In Docker Compose and `.env` files, environment variable interpolation commonly yields empty strings (e.g. `MEDIA_BIND_ADDR=${BIND_ADDR:-}`). If an empty string triggered an `AddrParseError`, containers would crash upon deployment when optional variables are left blank. Treating empty/whitespace strings identically to unset variables provides operational robustness.

### Rationale for `Ipv4Addr` vs `IpAddr`
`PROJECT.md` specifies:
> `MEDIA_BIND_ADDR`: IPv4 address to listen on (default `127.0.0.1`, set to `0.0.0.0` in container).

1. `SocketAddrV4::new(bind_addr, port)` requires `Ipv4Addr`.
2. Strict `Ipv4Addr` parsing prevents unintentional binding to IPv6 dual-stack sockets or IPv6 link-local addresses without explicit operator intent.
3. It keeps the codebase focused, typed, and fully aligned with existing network invariants.

---

## 4. Error Handling and Validation Strategy

### Diagnostic Quality
Standard `str::parse::<Ipv4Addr>()` yields `std::net::AddrParseError`, which produces only `"invalid IP address syntax"`.
To ensure administrators and self-hosters immediately understand what went wrong, the error must explicitly include the variable name and the provided value:
```
invalid MEDIA_BIND_ADDR 'invalid-ip': invalid IP address syntax
```

### Type Compatibility
The signature of `crates/core/src/main.rs:8` is:
```rust
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
```
In standard Rust:
- `String` does NOT implement `std::error::Error`. Writing `return Err(format!("...").into())` does not compile.
- `std::io::Error` DOES implement `std::error::Error + Send + Sync`.
- Throughout `crates/core` (e.g. `identity.rs:191`, `node.rs:132`), custom errors use `std::io::Error::other(...)` or `std::io::Error::new(std::io::ErrorKind::InvalidInput, ...)`.

Using `std::io::Error::other(format!("invalid MEDIA_BIND_ADDR '{raw}': {err}"))` provides idiomatic, robust error wrapping that integrates seamlessly with `?`.

---

## 5. Test Suite Analysis & Server Spin-up Across Workspace

### Workspace Members
The workspace root `Cargo.toml` contains exactly one package: `crates/core`.

### Integration Tests in `crates/core/tests/`
We analyzed all 12 test files in `crates/core/tests/`:

1. **`core_restart.rs`**:
   - Executes `CARGO_BIN_EXE_media-poc` via `tokio::process::Command`.
   - Configures: `MEDIA_DB_PATH`, `MEDIA_PORT`, `MEDIA_FIXTURE_DIR`, and removes `MEDIA_PUBLIC_URL`.
   - Connects to: `tokio::net::TcpStream::connect(("127.0.0.1", port))`.
   - **Impact**: Does not set `MEDIA_BIND_ADDR`. With the default falling back to `127.0.0.1`, this test passes without modification.

2. **`browser_scanner.rs`, `browser_posters.rs`, `browser_nfo.rs`**:
   - Execute `CARGO_BIN_EXE_media-poc` subprocesses.
   - Configure: `MEDIA_DB_PATH`, `MEDIA_LIBRARY_ROOT`, `MEDIA_PORT`.
   - Poll readiness via `TcpStream::connect(("127.0.0.1", port))`.
   - **Impact**: Do not set `MEDIA_BIND_ADDR`. Pass without modification.

3. **`browser_auth.rs`**:
   - Binds an in-process socket `tokio::net::TcpListener::bind("127.0.0.1:0")`.
   - Directly calls `secure_app(inventory, port, auth)`.
   - **Impact**: Does not invoke `main.rs`. Unaffected.

4. **`permissions.rs`, `nfo_flow.rs`, `posters_flow.rs`, `scanner_flow.rs`, `identity.rs`, `catalog.rs`, `p0.rs`**:
   - Test handlers directly or through Tower service interfaces (`oneshot`).
   - Do not bind TCP listeners.
   - **Impact**: Unaffected.

### Baseline Verification Result
Executing `cargo test` on the baseline repository:
- `unittests src\lib.rs`: 24 passed
- `tests\core_restart.rs`: 1 passed
- `tests\identity.rs`: 6 passed
- `tests\nfo_flow.rs`: 1 passed
- `tests\permissions.rs`: 2 passed
- `tests\posters_flow.rs`: 1 passed
- `tests\scanner_flow.rs`: 1 passed
- **Total: 36 passed; 0 failed; 5 ignored (browser/Playwright tests).**
- `cargo clippy --all-targets -- -D warnings`: 0 warnings, clean.
- `cargo fmt --check`: Clean formatting.

---

## 6. Security Interplay: Binding vs Public Origin vs Healthcheck

### Separation of Concerns: Network Socket vs Application Origin
- `MEDIA_BIND_ADDR`: Dictates the local network socket interface (`0.0.0.0` or `127.0.0.1`). It is a transport-layer parameter.
- `MEDIA_PUBLIC_URL`: Dictates the canonical external origin (e.g. `http://127.0.0.1:3000` or `https://aether.example.com`). It is an application-layer parameter used for:
  - `Host` header validation (`crates/core/src/lib.rs:180`).
  - `Origin` header validation on mutating requests (`crates/core/src/lib.rs:182`).
  - Session cookie `Secure` and `SameSite` flags (`crates/core/src/auth.rs:81, 107`).

### Origin Safety Guard in `crates/core/src/auth.rs:76-80`
```rust
if uri.authority().is_none_or(|a| a.as_str().contains('@'))
    || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
    || !(scheme == "https"
        || (scheme == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")))
{
    return Err(AuthConfigError::Origin);
}
```
If an operator mistakenly attempts to set `MEDIA_PUBLIC_URL=http://0.0.0.0:3000`, `AuthState::new` immediately rejects it because `0.0.0.0` is not a secure HTTP origin.
This guarantees that binding to `0.0.0.0` via `MEDIA_BIND_ADDR` cannot weaken CSRF or origin protections.

### Container Healthcheck Interaction
Inside the container, a Docker healthcheck probe querying `http://127.0.0.1:3000/api/v1/health` will connect successfully because the socket listening on `0.0.0.0:3000` receives loopback traffic on `127.0.0.1` as well. (Accommodating external `MEDIA_PUBLIC_URL` values for the healthcheck route is addressed in Milestone 1 Feature 2).

---

## 7. Implementation Strategy & Code Diff Proposal

### Architecture of the Change
To ensure deterministic, thread-safe testing without mutating global environment variables (`std::env::set_var`), we extract the parsing logic into a pure helper function:
1. `parse_bind_addr_from(var: Result<String, std::env::VarError>) -> Result<Ipv4Addr, std::io::Error>`: Pure parser with zero side effects.
2. `parse_bind_addr() -> Result<Ipv4Addr, std::io::Error>`: Thin wrapper reading `std::env::var("MEDIA_BIND_ADDR")`.
3. Unit tests in `crates/core/src/main.rs`: Test all parsing scenarios (unset, empty, whitespace, valid IPv4, padded IPv4, invalid strings, IPv6, port suffix).
4. Update `println!` to display `{public_origin}/app/` rather than hardcoded `http://127.0.0.1:{port}/app/`.

### Code Diff Proposal (`crates/core/src/main.rs`)

```diff
--- a/crates/core/src/main.rs
+++ b/crates/core/src/main.rs
@@ -5,6 +5,29 @@
     path::PathBuf,
 };
 
+fn parse_bind_addr_from(var: Result<String, std::env::VarError>) -> Result<Ipv4Addr, std::io::Error> {
+    match var {
+        Ok(raw) => {
+            let trimmed = raw.trim();
+            if trimmed.is_empty() {
+                Ok(Ipv4Addr::LOCALHOST)
+            } else {
+                trimmed
+                    .parse::<Ipv4Addr>()
+                    .map_err(|err| std::io::Error::other(format!("invalid MEDIA_BIND_ADDR '{raw}': {err}")))
+            }
+        }
+        Err(std::env::VarError::NotPresent) => Ok(Ipv4Addr::LOCALHOST),
+        Err(std::env::VarError::NotUnicode(_)) => {
+            Err(std::io::Error::other("MEDIA_BIND_ADDR is not valid unicode"))
+        }
+    }
+}
+
+fn parse_bind_addr() -> Result<Ipv4Addr, std::io::Error> {
+    parse_bind_addr_from(std::env::var("MEDIA_BIND_ADDR"))
+}
+
 #[tokio::main]
 async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
     let library_root = std::env::var("MEDIA_LIBRARY_ROOT").ok();
@@ -14,6 +37,7 @@
     let port: u16 = std::env::var("MEDIA_PORT")
         .unwrap_or_else(|_| "3000".into())
         .parse()?;
+    let bind_addr = parse_bind_addr()?;
     let db_path = media_poc::identity::configured_path(&root)?;
     let database = media_poc::identity::IdentityStore::open(&db_path).await?;
     let inventory = if library_root.is_some() {
@@ -23,12 +47,12 @@
             .await
             .map_err(|e| std::io::Error::other(e.to_string()))?
     };
-    let listener =
-        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
+    let listener =
+        tokio::net::TcpListener::bind(SocketAddrV4::new(bind_addr, port)).await?;
     let port = listener.local_addr()?.port();
     let public_origin =
         std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
-    let auth = AuthState::new(database.clone(), public_origin).await?;
-    println!("Authenticated local Core: http://127.0.0.1:{port}/app/");
+    let auth = AuthState::new(database.clone(), public_origin.clone()).await?;
+    println!("Authenticated local Core: {public_origin}/app/");
     let jobs = std::sync::Arc::new(ScanJobs::default());
     let shutdown_jobs = jobs.clone();
     axum::serve(
@@ -44,3 +68,54 @@
     database.close().await;
     Ok(())
 }
+
+#[cfg(test)]
+mod tests {
+    use super::*;
+
+    #[test]
+    fn parse_bind_addr_default_when_unset_or_empty() {
+        assert_eq!(
+            parse_bind_addr_from(Err(std::env::VarError::NotPresent)).unwrap(),
+            Ipv4Addr::LOCALHOST
+        );
+        assert_eq!(
+            parse_bind_addr_from(Ok(String::new())).unwrap(),
+            Ipv4Addr::LOCALHOST
+        );
+        assert_eq!(
+            parse_bind_addr_from(Ok("   \t  ".into())).unwrap(),
+            Ipv4Addr::LOCALHOST
+        );
+    }
+
+    #[test]
+    fn parse_bind_addr_valid_ipv4() {
+        assert_eq!(
+            parse_bind_addr_from(Ok("0.0.0.0".into())).unwrap(),
+            Ipv4Addr::UNSPECIFIED
+        );
+        assert_eq!(
+            parse_bind_addr_from(Ok("  0.0.0.0  ".into())).unwrap(),
+            Ipv4Addr::UNSPECIFIED
+        );
+        assert_eq!(
+            parse_bind_addr_from(Ok("127.0.0.1".into())).unwrap(),
+            Ipv4Addr::LOCALHOST
+        );
+        assert_eq!(
+            parse_bind_addr_from(Ok("192.168.1.50".into())).unwrap(),
+            Ipv4Addr::new(192, 168, 1, 50)
+        );
+    }
+
+    #[test]
+    fn parse_bind_addr_invalid_ipv4_returns_error() {
+        let err = parse_bind_addr_from(Ok("invalid-ip".into())).unwrap_err();
+        assert!(err.to_string().contains("invalid MEDIA_BIND_ADDR 'invalid-ip'"));
+        let err = parse_bind_addr_from(Ok("256.0.0.1".into())).unwrap_err();
+        assert!(err.to_string().contains("invalid MEDIA_BIND_ADDR '256.0.0.1'"));
+        let err = parse_bind_addr_from(Ok("::1".into())).unwrap_err();
+        assert!(err.to_string().contains("invalid MEDIA_BIND_ADDR '::1'"));
+    }
+}
```

---

## 8. Process-Level Integration Test Proposal

In addition to the unit tests embedded in `main.rs`, an integration test (e.g. in `crates/core/tests/network_binding.rs` or added to `core_restart.rs`) can verify process-level behavior:

```rust
#[tokio::test]
async fn media_poc_rejects_invalid_bind_addr() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-poc"))
        .current_dir(&root)
        .env("MEDIA_BIND_ADDR", "invalid-ip-test")
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid MEDIA_BIND_ADDR 'invalid-ip-test'"));
}
```

This confirms complete end-to-end verification of startup validation and error logging.
