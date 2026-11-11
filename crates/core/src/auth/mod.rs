//! Local authentication. Session secrets never enter the database or application logs.
//!
//! Module layout:
//! - `accounts`: several accounts on one browser, parked beside the active one.
//! - `touch`: coalesced, batched session activity writes.
//! - `state`: `AuthState`, identity types and session / CSRF / origin checks.
//! - `config`: environment-driven authentication configuration.
//! - `util`: token, cookie, hashing and client-key helpers.
//! - `http`: REST router and handlers.

mod accounts;
mod config;
mod grants;
mod http;
mod mfa;
mod passkey;
mod quick_connect;
mod state;
mod touch;
mod util;

#[cfg(test)]
mod mfa_tests;
#[cfg(test)]
mod tests;

pub use config::{is_http_bootstrap_disabled, is_local_login_enabled};
pub(crate) use http::issue_session;
pub use http::{authenticate, router};
pub use state::{AuthConfigError, AuthState, AuthUser, Authenticated};
pub use touch::SessionTouchStats;
pub use util::extract_client_key;
pub(crate) use util::{ABSOLUTE_SECONDS, IDLE_SECONDS, device_label, hash, random_token};
