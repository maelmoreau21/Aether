//! Confidential OIDC code flow. Provider tokens never leave this module.
//!
//! Module layout:
//! - `error`: HTTP error type and constructors.
//! - `config`: environment configuration and issuer / endpoint validation.
//! - `types`: shared state, provider payloads and request / response shapes.
//! - `claims`: ID token claims and derived identity attributes.
//! - `provider`: discovery, bounded JSON fetches and ID token validation.
//! - `flow`: authorization-code start, callback completion, cookies, logout URL.
//! - `http`: REST router and handlers.

mod claims;
mod config;
mod error;
mod flow;
mod http;
mod provider;
mod types;

#[cfg(test)]
mod tests;

pub use http::router;
