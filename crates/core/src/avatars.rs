//! Account-owned normalized avatars. No client paths, remote URLs or original metadata persisted.
pub(crate) mod defaults;
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
#[cfg(test)]
use image::ImageFormat;
#[cfg(test)]
use std::io::Cursor;
const MAX_UPLOAD: usize = 2 * 1024 * 1024;

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
    let image = crate::image_import::decode(&bytes, content_type)?;
    let (max_w, max_h) = if poster { (1000, 1500) } else { (256, 256) };
    let not_resized = image.width() <= max_w && image.height() <= max_h;
    if content_type == "image/webp"
        && not_resized
        && media_node::image_encoding::reusable_webp(&bytes, image.color().has_alpha())
    {
        return Ok(bytes.to_vec());
    }
    let resized = if not_resized {
        image
    } else {
        image.thumbnail(max_w, max_h)
    };
    crate::image_import::encode(&resized, resized.color().has_alpha())
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
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "image/png"
    }
}
async fn read(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    if let Err(error) = defaults::assign_user(&auth.store, &user.user.id).await {
        eprintln!("Mandatory avatar assignment failed: {error}");
    }
    let sha256: Option<String> = query_scalar("SELECT sha256 FROM user_avatars WHERE user_id=?")
        .bind(&user.user.id)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(sha256) = sha256
        && let Ok(response) = auth
            .store
            .assets
            .response_after_authorization(&sha256, &headers)
            .await
    {
        return Ok(response);
    }
    // The mandatory local avatar also works with an empty gallery or unavailable
    // asset storage; no account is rendered without a profile image.
    Ok(fallback_response(defaults::fallback_for_user(
        &user.user.id,
    )))
}

pub(crate) fn fallback_response(bytes: Vec<u8>) -> Response {
    let mut response = Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("image/svg+xml"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response
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
            allow_custom: false,
            allow_gravatar: false,
            assign_on_create: true,
        }
    }
}
pub(crate) async fn load_policy(_auth: &AuthState) -> Result<AvatarPolicy, StatusCode> {
    // Old persisted policy flags cannot re-enable personal uploads or opt out
    // of the required gallery avatar.
    Ok(AvatarPolicy::default())
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
    if input.allow_custom || input.allow_gravatar || !input.assign_on_create {
        return Err(StatusCode::FORBIDDEN);
    }
    query("INSERT INTO avatar_policy(singleton,allow_custom,allow_gravatar,assign_on_create) VALUES(1,0,0,1) ON CONFLICT(singleton) DO UPDATE SET allow_custom=0,allow_gravatar=0,assign_on_create=1")
       .execute(&auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<axum::Json<serde_json::Value>, StatusCode> {
    auth.authenticate(&headers).await?;
    Ok(axum::Json(
        serde_json::json!({"uploadAllowed":false,"removeAllowed":false}),
    ))
}
async fn write(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &user)?;
    Err(StatusCode::FORBIDDEN)
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let user = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &user)?;
    Err(StatusCode::FORBIDDEN)
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
            assert_eq!(image::guess_format(&normalized).unwrap(), ImageFormat::WebP);
            let image = image::load_from_memory(&normalized).unwrap();
            assert_eq!((image.width(), image.height()), (32, 16));
        }
        let mut large = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4097, 1)
            .write_to(&mut large, ImageFormat::Png)
            .unwrap();
        assert!(normalize(Bytes::from(large.into_inner()), "image/png").is_err());
    }
}
