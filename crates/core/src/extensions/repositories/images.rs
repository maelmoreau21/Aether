use super::*;

pub(super) type ImageCache = std::sync::Mutex<std::collections::HashMap<String, (Instant, String)>>;

pub(super) fn image_cache() -> &'static ImageCache {
    static CACHE: std::sync::LazyLock<ImageCache> = std::sync::LazyLock::new(Default::default);
    &CACHE
}

fn is_allowed_svg_attr(name: &str, value: &str) -> bool {
    let lower_val = value.to_ascii_lowercase();
    if lower_val.contains("javascript:")
        || lower_val.contains("vbscript:")
        || lower_val.contains("data:")
        || lower_val.contains("url(")
        || lower_val.contains("expression(")
    {
        return false;
    }
    match name {
        "xmlns" => value == "http://www.w3.org/2000/svg",
        "id" => {
            !value.is_empty()
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        }
        "viewbox" | "width" | "height" | "x" | "y" | "rx" | "ry" | "cx" | "cy" | "r" | "x1"
        | "x2" | "y1" | "y2" | "stroke-width" | "opacity" | "fill-opacity" | "stroke-opacity" => {
            !value.is_empty()
                && value.chars().all(|c| {
                    c.is_ascii_digit()
                        || matches!(c, '.' | '-' | '+' | '%' | ' ' | ',' | 'p' | 'x' | 'e' | 'm')
                })
        }
        "fill" | "stroke" => {
            !value.is_empty()
                && value.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, '#' | '(' | ')' | ',' | '.' | ' ' | '%' | '-')
                })
        }
        "stroke-linecap" | "stroke-linejoin" | "fill-rule" | "clip-rule" => {
            !value.is_empty() && value.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
        }
        "d" => value.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, ' ' | ',' | '.' | '-' | '+' | '\t' | '\n' | '\r')
        }),
        "points" => value.chars().all(|c| {
            c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | ',' | ' ' | '\t' | '\n' | '\r')
        }),
        "transform" => value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | '.' | '-' | '+' | ',' | ' ')
        }),
        _ => false,
    }
}

fn validate_svg_tag(tag: &quick_xml::events::BytesStart<'_>, is_root: bool) -> Option<String> {
    let qname = tag.name();
    let raw_name: &str = qname.as_ref();
    if raw_name.contains(':') {
        return None;
    }
    let name = raw_name.to_ascii_lowercase();
    if is_root && name != "svg" {
        return None;
    }
    let allowed_tag = matches!(
        name.as_str(),
        "svg"
            | "g"
            | "rect"
            | "circle"
            | "ellipse"
            | "path"
            | "line"
            | "polygon"
            | "polyline"
            | "defs"
            | "title"
    );
    if !allowed_tag {
        return None;
    }
    for attr in tag.attributes() {
        let attr = attr.ok()?;
        let attr_name: &str = attr.key.as_ref();
        if attr_name.contains(':') {
            return None;
        }
        let attr_name_lower = attr_name.to_ascii_lowercase();
        let attr_val: &str = attr.value.as_ref();
        if !is_allowed_svg_attr(&attr_name_lower, attr_val) {
            return None;
        }
    }
    Some(name)
}

pub(super) fn is_safe_svg_text(text: &str) -> bool {
    if text.contains('&') {
        return false;
    }
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<String> = Vec::new();
    let mut saw_svg_root = false;
    let mut element_count = 0usize;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => {
                element_count += 1;
                if element_count > 4096 || stack.len() >= 64 || (saw_svg_root && stack.is_empty()) {
                    return false;
                }
                let is_root = !saw_svg_root && stack.is_empty();
                let Some(name) = validate_svg_tag(&e, is_root) else {
                    return false;
                };
                if is_root {
                    saw_svg_root = true;
                }
                stack.push(name);
            }
            Ok(quick_xml::events::Event::Empty(e)) => {
                element_count += 1;
                if element_count > 4096 || stack.len() >= 64 || (saw_svg_root && stack.is_empty()) {
                    return false;
                }
                let is_root = !saw_svg_root && stack.is_empty();
                if validate_svg_tag(&e, is_root).is_none() {
                    return false;
                }
                if is_root {
                    saw_svg_root = true;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                let qname = e.name();
                let raw_name: &str = qname.as_ref();
                let name = raw_name.to_ascii_lowercase();
                if stack.pop().as_deref() != Some(name.as_str()) {
                    return false;
                }
            }
            Ok(quick_xml::events::Event::Text(t)) => {
                let text_content: &str = t.as_ref();
                let trimmed = text_content.trim();
                if !trimmed.is_empty() && stack.last().map(String::as_str) != Some("title") {
                    return false;
                }
            }
            Ok(quick_xml::events::Event::Decl(_)) | Ok(quick_xml::events::Event::Comment(_)) => {}
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(
                quick_xml::events::Event::DocType(_)
                | quick_xml::events::Event::PI(_)
                | quick_xml::events::Event::CData(_)
                | quick_xml::events::Event::GeneralRef(_),
            )
            | Err(_) => return false,
        }
    }

    saw_svg_root && stack.is_empty()
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
        if text.starts_with('<') && is_safe_svg_text(text) {
            return Some("image/svg+xml");
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
