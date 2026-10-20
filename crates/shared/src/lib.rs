//! Shared typed contracts, identifiers, and brand constants between Core and Media Node.
//! Contains no SQL, HTTP server, user session, or filesystem I/O dependencies.

pub mod brand;
pub mod errors;
pub mod ids;
pub mod inventory;
pub mod media;
pub mod protocol;
pub mod version;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_ids_preserve_historical_and_uuid_strings() {
        let local = ids::NodeId::local();
        assert_eq!(serde_json::to_string(&local).unwrap(), "\"local\"");
        let parsed: ids::NodeId = serde_json::from_str("\"local\"").unwrap();
        assert_eq!(parsed, local);

        let fixture = ids::LibraryId::from("fixture-films");
        assert_eq!(
            serde_json::to_string(&fixture).unwrap(),
            "\"fixture-films\""
        );

        let work = ids::WorkId::from("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(
            serde_json::to_string(&work).unwrap(),
            "\"550e8400-e29b-41d4-a716-446655440000\""
        );
    }

    #[test]
    fn nfo_outcome_serialization_contract_is_preserved() {
        let outcome = media::NfoOutcome::Missing;
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json, serde_json::json!({"status": "missing"}));

        let valid = media::NfoOutcome::Valid {
            metadata: media::FilmMetadata {
                title: Some("Film".into()),
                ..Default::default()
            },
            path: "Film/movie.nfo".into(),
            priority: 1,
        };
        let encoded = serde_json::to_string(&valid).unwrap();
        let decoded: media::NfoOutcome = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, valid);
    }
}
