//! Account-owned normalized avatars. No client paths, remote URLs or original metadata persisted.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;
use tokio::sync::Semaphore;
const MAX_UPLOAD: usize = 2 * 1024 * 1024;
static DECODERS: Semaphore = Semaphore::const_new(2);

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route(
            "/api/v1/account/avatar",
            get(read).put(write).delete(remove),
        )
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
pub(crate) fn normalize(bytes: Bytes, content_type: &str) -> Result<Vec<u8>, StatusCode> {
    normalize_image(bytes, content_type, false)
}
pub(crate) fn normalize_image(
    bytes: Bytes,
    content_type: &str,
    poster: bool,
) -> Result<Vec<u8>, StatusCode> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let expected = match reader.format() {
        Some(ImageFormat::Png) => "image/png",
        Some(ImageFormat::Jpeg) => "image/jpeg",
        Some(ImageFormat::WebP) => "image/webp",
        _ => return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
    };
    if content_type != expected {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if decoder.total_bytes() > 32 * 1024 * 1024 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let image =
        image::DynamicImage::from_decoder(decoder).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if image.width() == 0 || image.height() == 0 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let resized = if poster {
        image.resize(1000, 1500, image::imageops::FilterType::Triangle)
    } else {
        image.resize_to_fill(256, 256, image::imageops::FilterType::Triangle)
    };
    let mut output = Cursor::new(Vec::new());
    resized
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    Ok(output.into_inner())
}
async fn read(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    let bytes: Option<Vec<u8>> =
        query_scalar("SELECT image_data FROM user_avatars WHERE user_id=?")
            .bind(user.user.id)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes.ok_or(StatusCode::NOT_FOUND)?,
    )
        .into_response())
}
async fn write(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &user)?;
    if user.user.role != crate::identity::Role::Administrator {
        let rows: Vec<String> = query_scalar(
            "SELECT es.settings_json FROM extension_settings es \
             JOIN extensions e ON e.id = es.extension_id \
             WHERE e.enabled = 1",
        )
        .fetch_all(&auth.store.pool)
        .await
        .unwrap_or_default();

        for json_str in rows {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
                && val
                    .get("disallow_custom_avatars")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }
    if bytes.is_empty() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let permit = DECODERS
        .try_acquire()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let bytes = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        normalize(bytes, &content_type)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    query("INSERT INTO user_avatars(user_id,image_data) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET image_data=excluded.image_data").bind(user.user.id).bind(bytes).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &user)?;
    query("DELETE FROM user_avatars WHERE user_id=?")
        .bind(user.user.id)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avatar_formats_are_reencoded_and_dimensions_are_bounded() {
        for (format, mime) in [
            (ImageFormat::Png, "image/png"),
            (ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::WebP, "image/webp"),
        ] {
            let mut buffer = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(32, 16)
                .write_to(&mut buffer, format)
                .unwrap();
            let normalized = normalize(Bytes::from(buffer.into_inner()), mime).unwrap();
            assert_eq!(image::guess_format(&normalized).unwrap(), ImageFormat::Png);
            let image = image::load_from_memory(&normalized).unwrap();
            assert_eq!((image.width(), image.height()), (256, 256));
        }
        let mut large = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4097, 1)
            .write_to(&mut large, ImageFormat::Png)
            .unwrap();
        assert!(normalize(Bytes::from(large.into_inner()), "image/png").is_err());
    }
}
