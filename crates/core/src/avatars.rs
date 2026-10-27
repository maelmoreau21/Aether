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
    response::Response,
    routing::get,
};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;
use tokio::sync::Semaphore;
const MAX_UPLOAD: usize = 2 * 1024 * 1024;
static DECODERS: Semaphore = Semaphore::const_new(2);
pub(crate) static POLICY_LOCK: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route(
            "/api/v1/account/avatar",
            get(read).put(write).delete(remove),
        )
        .route("/api/v1/account/avatar/policy", get(policy))
        .route(
            "/api/v1/admin/avatar-policy",
            get(admin_policy).put(set_admin_policy),
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
/// Deliberately small inert SVG subset for extension avatars: no scripts, links,
/// styles, embedded images, references, entities or animation. Never inline HTML.
pub(crate) fn validate_svg(svg: &str) -> Result<(), StatusCode> {
    use quick_xml::events::Event;
    let invalid = StatusCode::UNPROCESSABLE_ENTITY;
    if svg.len() > 32_768 || !svg.starts_with("<svg ") || svg.contains('&') {
        return Err(invalid);
    }
    let mut reader = quick_xml::Reader::from_str(svg);
    let mut depth = 0usize;
    let mut roots = 0;
    let mut count = 0;
    loop {
        let event = reader.read_event().map_err(|_| invalid)?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                count += 1;
                if count > 512 {
                    return Err(invalid);
                }
                let name = element.name();
                if !matches!(
                    name.as_ref(),
                    "svg"
                        | "g"
                        | "rect"
                        | "circle"
                        | "ellipse"
                        | "path"
                        | "line"
                        | "polygon"
                        | "polyline"
                ) {
                    return Err(invalid);
                }
                if depth == 0 {
                    roots += 1;
                    if roots != 1 || name.as_ref() != "svg" {
                        return Err(invalid);
                    }
                }
                for attr in element.attributes() {
                    let attr = attr.map_err(|_| invalid)?;
                    let key = attr.key.as_ref();
                    let value = attr.value.as_ref();
                    match key {
                        "xmlns"
                            if name.as_ref() == "svg" && value == "http://www.w3.org/2000/svg" => {}
                        "fill" | "stroke" => {
                            if value != "none"
                                && !(value.starts_with('#')
                                    && matches!(value.len(), 4 | 7 | 9)
                                    && value[1..].bytes().all(|b| b.is_ascii_hexdigit()))
                            {
                                return Err(invalid);
                            }
                        }
                        "stroke-linecap" if matches!(value, "round" | "butt" | "square") => {}
                        "viewBox" | "width" | "height" | "x" | "y" | "rx" | "ry" | "cx" | "cy"
                        | "r" | "x1" | "x2" | "y1" | "y2" | "opacity" | "fill-opacity"
                        | "stroke-opacity" | "stroke-width" => {
                            if value.len() > 128
                                || !value
                                    .bytes()
                                    .all(|b| b.is_ascii_digit() || b" .,+-eE".contains(&b))
                            {
                                return Err(invalid);
                            }
                        }
                        "d" | "points" => {
                            if !value.bytes().all(|b| {
                                b.is_ascii_digit() || b"MmZzLlHhVvCcSsQqTtAaEe .,+-".contains(&b)
                            }) {
                                return Err(invalid);
                            }
                        }
                        _ => return Err(invalid),
                    }
                }
                if !empty {
                    depth += 1;
                    if depth > 16 {
                        return Err(invalid);
                    }
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or(invalid)?;
            }
            Event::Text(text) if text.as_ref().bytes().all(|b| b.is_ascii_whitespace()) => {}
            Event::Eof => break,
            _ => return Err(invalid),
        }
    }
    if roots != 1 || depth != 0 {
        return Err(invalid);
    }
    Ok(())
}
pub(crate) fn content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"<svg ") {
        "image/svg+xml"
    } else {
        "image/png"
    }
}
async fn read(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    let sha256: Option<String> = query_scalar("SELECT sha256 FROM user_avatars WHERE user_id=?")
        .bind(user.user.id)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let sha256 = sha256.ok_or(StatusCode::NOT_FOUND)?;
    auth.store
        .assets
        .response_after_authorization(&sha256, &headers)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AvatarPolicy {
    pub allow_custom: bool,
    pub allow_gravatar: bool,
    pub assign_on_create: bool,
}
impl Default for AvatarPolicy {
    fn default() -> Self {
        Self {
            allow_custom: true,
            allow_gravatar: false,
            assign_on_create: false,
        }
    }
}
pub(crate) async fn load_policy(auth: &AuthState) -> Result<AvatarPolicy, StatusCode> {
    let row: Option<(i64, i64, i64)> = crate::database::query_as(
        "SELECT allow_custom,allow_gravatar,assign_on_create FROM avatar_policy WHERE singleton=1",
    )
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(row
        .map(|(custom, gravatar, assign)| AvatarPolicy {
            allow_custom: custom != 0,
            allow_gravatar: gravatar != 0,
            assign_on_create: assign != 0,
        })
        .unwrap_or_default())
}
async fn admin_policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<axum::Json<AvatarPolicy>, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    if user.user.role != crate::identity::Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(axum::Json(load_policy(&auth).await?))
}
async fn set_admin_policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    axum::Json(input): axum::Json<AvatarPolicy>,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    if user.user.role != crate::identity::Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    auth.require_csrf(&headers, &user)?;
    let _policy_guard = POLICY_LOCK.write().await;
    query("INSERT INTO avatar_policy(singleton,allow_custom,allow_gravatar,assign_on_create) VALUES(1,?,?,?) ON CONFLICT(singleton) DO UPDATE SET allow_custom=excluded.allow_custom,allow_gravatar=excluded.allow_gravatar,assign_on_create=excluded.assign_on_create")
       .bind(i64::from(input.allow_custom)).bind(i64::from(input.allow_gravatar)).bind(i64::from(input.assign_on_create))
       .execute(&auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn permissions(auth: &AuthState) -> Result<(bool, bool), StatusCode> {
    Ok((load_policy(auth).await?.allow_custom, true))
}
async fn policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    auth.authenticate(&headers).await?;
    let (upload, remove) = permissions(&auth).await?;
    Ok(axum::Json(
        serde_json::json!({"uploadAllowed":upload,"removeAllowed":remove}),
    ))
}
async fn write(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &user)?;
    if !permissions(&auth).await?.0 {
        return Err(StatusCode::FORBIDDEN);
    }
    if bytes.is_empty() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let req_content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let permit = DECODERS
        .try_acquire()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let bytes = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        normalize(bytes, &req_content_type)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    let _policy_guard = POLICY_LOCK.read().await;
    if !permissions(&auth).await?.0 {
        return Err(StatusCode::FORBIDDEN);
    }
    let mime = content_type(&bytes);
    let meta = auth
        .store
        .assets
        .put_bytes(&bytes, mime)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("INSERT INTO user_avatars(user_id,sha256) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET sha256=excluded.sha256")
        .bind(user.user.id)
        .bind(&meta.sha256)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &user)?;
    if !permissions(&auth).await?.1 {
        return Err(StatusCode::FORBIDDEN);
    }
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
    fn extension_svg_only_accepts_inert_bounded_shapes() {
        assert!(validate_svg(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><rect width="256" height="256" fill="#fff"/><g><path d="M 0,0 L 10,10 Z" stroke="#abcdef"/></g></svg>"##).is_ok());
        for svg in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.invalid/"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path fill="url(#x)"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"/><svg/>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g></svg>"#,
        ] {
            assert!(validate_svg(svg).is_err(), "{svg}");
        }
    }
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
