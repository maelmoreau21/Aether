use super::*;

pub(super) type ImageCache = std::sync::Mutex<std::collections::HashMap<String, (Instant, String)>>;

pub(super) fn image_cache() -> &'static ImageCache {
    static CACHE: std::sync::LazyLock<ImageCache> = std::sync::LazyLock::new(Default::default);
    &CACHE
}

pub(super) fn is_safe_svg_text(lower: &str) -> bool {
    let forbidden = [
        "<script",
        "<foreignobject",
        "<iframe",
        "<embed",
        "<object",
        "<use",
        "<animate",
        "<set",
        "<!doctype",
        "<!entity",
        "javascript:",
        "vbscript:",
        "data:text/html",
    ];
    if forbidden.iter().any(|bad| lower.contains(bad)) {
        return false;
    }
    for (i, _) in lower.match_indices("on") {
        if i > 0 {
            let prev = lower.as_bytes()[i - 1];
            if !prev.is_ascii_whitespace() && prev != b'/' && prev != b'<' {
                continue;
            }
        }
        let rest = &lower[i + 2..];
        let name_len = rest.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        if name_len >= 2 {
            let after_name = rest[name_len..].trim_start();
            if after_name.starts_with('=') {
                return false;
            }
        }
    }
    true
}

/// Image types accepted from a catalogue `imageUrl`, recognised by content.
pub(super) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        let text = std::str::from_utf8(bytes).ok()?;
        let text = text.trim_start_matches('\u{feff}').trim_start();
        if text.starts_with('<') && text.contains("<svg") {
            let lower = text.to_ascii_lowercase();
            if is_safe_svg_text(&lower) {
                return Some("image/svg+xml");
            }
        }
        None
    }
}

/// The catalogue image as a `data:` URL. The Web UI only loads same-origin or
/// `data:` images, and a JSON response is never rendered as a document.
pub(super) async fn image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, extension)): Path<(String, String)>,
) -> Result<impl IntoResponse, Error> {
    admin(&auth, &headers, false).await?;
    let snapshot: Option<String> =
        query_scalar("SELECT manifest_json FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let snapshot = snapshot.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_repository"))?;
    let url = parse_manifest(snapshot.as_bytes())?
        .into_iter()
        .find(|item| item.id == extension)
        .and_then(|item| item.image_url)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "extension_image_not_found"))?;
    let cached = image_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&url).cloned())
        .filter(|(time, _)| time.elapsed() < Duration::from_secs(3600))
        .map(|(_, data)| data);
    let data = match cached {
        Some(data) => data,
        None => {
            safe_url(&url)?;
            #[cfg(test)]
            let test = test_document(&url, MAX_IMAGE);
            #[cfg(not(test))]
            let test: Option<Result<Vec<u8>, Error>> = None;
            let bytes = match test {
                Some(result) => result?,
                None => {
                    let _permit = IMAGES
                        .acquire()
                        .await
                        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "repository_busy"))?;
                    download(&url, MAX_IMAGE, "image/*", Duration::from_secs(10)).await?
                }
            };
            let kind = image_type(&bytes)
                .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "extension_image_invalid"))?;
            let data = format!("data:{kind};base64,{}", STANDARD.encode(&bytes));
            if let Ok(mut cache) = image_cache().lock() {
                if cache.len() >= 128 {
                    cache.clear();
                }
                cache.insert(url, (Instant::now(), data.clone()));
            }
            data
        }
    };
    Ok((
        [(header::CACHE_CONTROL, "private, max-age=3600")],
        Json(json!({"dataUrl": data})),
    ))
}
