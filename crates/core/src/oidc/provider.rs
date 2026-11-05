//! Provider access: discovery, bounded JSON fetches and ID token validation.
use super::{
    claims::Claims,
    config::{Config, endpoint, issuers_match},
    error::{Error, deny, invalid, unavailable},
    types::{Discovery, Oidc},
};
use axum::http::StatusCode;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::de::DeserializeOwned;

pub(super) const SUPPORTED_ALGS: &[Algorithm] = &[
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

impl Oidc {
    pub(super) fn config(&self) -> Result<&Config, Error> {
        self.config
            .as_ref()
            .ok_or_else(|| deny(StatusCode::NOT_FOUND, "oidc_disabled"))
    }

    pub(super) async fn json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let mut response = request
            .send()
            .await
            .map_err(|_| unavailable())?
            .error_for_status()
            .map_err(|_| unavailable())?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if body.len() + chunk.len() > 262144 {
                return Err(unavailable());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| unavailable())
    }

    pub(super) async fn discover(&self) -> Result<Discovery, Error> {
        let c = self.config()?;
        let d: Discovery = self
            .json(self.http.get(format!(
                "{}/.well-known/openid-configuration",
                c.issuer.trim_end_matches('/')
            )))
            .await?;

        if !issuers_match(&d.issuer, &c.issuer) {
            return Err(invalid());
        }

        let has_supported_alg = d
            .id_token_signing_alg_values_supported
            .iter()
            .any(|a| SUPPORTED_ALGS.iter().any(|sa| format!("{sa:?}") == *a));
        if !has_supported_alg {
            return Err(invalid());
        }

        let issuer_url = endpoint(&c.issuer)?;
        let mut check_endpoints = vec![&d.authorization_endpoint, &d.token_endpoint, &d.jwks_uri];
        if let Some(ref ese) = d.end_session_endpoint {
            check_endpoints.push(ese);
        }

        for raw in check_endpoints {
            let ep = endpoint(raw)?;
            // Endpoints must share origin or be a valid secure endpoint
            if ep.origin() != issuer_url.origin() {
                return Err(invalid());
            }
        }
        Ok(d)
    }

    pub(super) async fn claims(
        &self,
        d: &Discovery,
        token: &str,
        nonce: &str,
    ) -> Result<Claims, Error> {
        let c = self.config()?;
        let h = jsonwebtoken::decode_header(token).map_err(|_| invalid())?;

        // Strict rejection of alg "none" or unsupported algorithms
        if !SUPPORTED_ALGS.contains(&h.alg) {
            return Err(invalid());
        }

        let kid = h.kid.ok_or_else(invalid)?;
        // Fresh JWKS on each callback: rotation is immediate, no stale-key acceptance.
        let keys: JwkSet = self.json(self.http.get(&d.jwks_uri)).await?;
        let jwk = keys.find(&kid).ok_or_else(invalid)?;
        let key = DecodingKey::try_from(jwk).map_err(|_| invalid())?;
        let mut validation = Validation::new(h.alg);
        validation.set_issuer(&[&c.issuer, c.issuer.trim_end_matches('/')]);
        validation.set_audience(&[&c.client_id]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub", "iat"]);
        validation.leeway = 0;
        validation.validate_nbf = true;
        let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map_err(|_| invalid())?
            .claims;
        let now = jsonwebtoken::get_current_timestamp();
        if claims.nonce != nonce
            || claims.sub.is_empty()
            || claims.sub.len() > 255
            || claims.iat > now + 30
            || claims.exp <= now
            || claims.azp.as_ref().is_some_and(|v| v != &c.client_id)
            || (claims.aud.as_array().is_some_and(|a| a.len() > 1)
                && claims.azp.as_deref() != Some(&c.client_id))
        {
            return Err(invalid());
        }
        Ok(claims)
    }
}
