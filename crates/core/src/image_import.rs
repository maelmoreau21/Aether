//! Core HTTP mapping for the shared Node/Core raster pipeline.
use axum::http::StatusCode;
pub(crate) use media_node::image_encoding::DECODERS;
pub(crate) const ENCODING: &str = "webp-v1";
fn status(error: media_node::image_encoding::ImportError) -> StatusCode {
    match error {
        media_node::image_encoding::ImportError::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        media_node::image_encoding::ImportError::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
    }
}
pub(crate) fn decode(bytes: &[u8], content: &str) -> Result<image::DynamicImage, StatusCode> {
    media_node::image_encoding::decode(bytes, content).map_err(status)
}
pub(crate) fn encode(image: &image::DynamicImage, lossless: bool) -> Result<Vec<u8>, StatusCode> {
    media_node::image_encoding::encode(image, lossless).map_err(status)
}
