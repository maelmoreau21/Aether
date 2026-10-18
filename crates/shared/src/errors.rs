use serde::{Deserialize, Serialize};

/// Public error contract that never discloses host filesystem paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
}

impl ApiErrorBody {
    pub fn new(code: impl Into<String>) -> Self {
        Self { error: code.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NodeErrorCode {
    #[error("unknown_asset")]
    UnknownAsset,
    #[error("unavailable_asset")]
    UnavailableAsset,
    #[error("asset_outside_fixture_root")]
    AssetOutsideFixtureRoot,
    #[error("stream_error")]
    StreamError,
    #[error("unknown_image")]
    UnknownImage,
    #[error("unavailable_image")]
    UnavailableImage,
    #[error("invalid_image")]
    InvalidImage,
    #[error("image_changed_rescan_required")]
    ImageChangedRescanRequired,
    #[error("image_stream_error")]
    ImageStreamError,
    #[error("unknown_experiment_asset")]
    UnknownExperimentAsset,
    #[error("experiment_asset_unavailable")]
    ExperimentAssetUnavailable,
    #[error("experiment_read_failed")]
    ExperimentReadFailed,
    #[error("unknown_subtitle")]
    UnknownSubtitle,
}

impl NodeErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownAsset => "unknown_asset",
            Self::UnavailableAsset => "unavailable_asset",
            Self::AssetOutsideFixtureRoot => "asset_outside_fixture_root",
            Self::StreamError => "stream_error",
            Self::UnknownImage => "unknown_image",
            Self::UnavailableImage => "unavailable_image",
            Self::InvalidImage => "invalid_image",
            Self::ImageChangedRescanRequired => "image_changed_rescan_required",
            Self::ImageStreamError => "image_stream_error",
            Self::UnknownExperimentAsset => "unknown_experiment_asset",
            Self::ExperimentAssetUnavailable => "experiment_asset_unavailable",
            Self::ExperimentReadFailed => "experiment_read_failed",
            Self::UnknownSubtitle => "unknown_subtitle",
        }
    }
}
