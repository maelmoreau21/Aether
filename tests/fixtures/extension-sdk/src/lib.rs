//! Test-only Wasm guest. Not an installable product extension.
use media_extension_sdk::{
    ExtensionRequest, ExtensionResponse, MetadataInput, MetadataProposal, serde_json,
};

pub fn invoke(request: ExtensionRequest) -> ExtensionResponse {
    let Ok(input) = serde_json::from_value::<MetadataInput>(request.input) else {
        return ExtensionResponse {
            api_version: 1,
            result: serde_json::Value::Null,
        };
    };
    let proposal = MetadataProposal {
        title: input.title,
        summary: format!(
            "Proposition synthétique pour {}. Aucun média modifié.",
            input.work_id
        ),
    };
    ExtensionResponse {
        api_version: 1,
        result: serde_json::json!(proposal),
    }
}
media_extension_sdk::extension!(invoke);
