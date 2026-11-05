use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

pub(super) type Error = StatusCode;

#[derive(Debug)]
pub struct PartyApiError(pub StatusCode, pub String);

impl IntoResponse for PartyApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<StatusCode> for PartyApiError {
    fn from(s: StatusCode) -> Self {
        let msg = match s {
            StatusCode::FORBIDDEN => "Accès refusé ou permissions insuffisantes.",
            StatusCode::NOT_FOUND => "Séance ou ressource introuvable.",
            StatusCode::BAD_REQUEST => "Données de requête invalides.",
            StatusCode::GONE => "La séance ou l'invitation a expiré ou est terminée.",
            StatusCode::UNAUTHORIZED => "Authentification requise.",
            _ => "Une erreur interne est survenue.",
        };
        Self(s, msg.to_string())
    }
}
