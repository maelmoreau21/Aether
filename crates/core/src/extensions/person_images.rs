//! Cast and crew portraits.
//!
//! Metadata providers return portrait URLs with their credits (`thumbUrl`, or `thumb` from a local
//! NFO). Browsers never load those third-party URLs: the catalogue exposes a Core route per credit,
//! and the first authorised request downloads the portrait through the host allowlist of an
//! extension granted to the work's library, re-encodes a bounded JPEG and keeps it in the asset
//! store. Later requests are served from that cache with an immutable private `ETag`.
use super::*;
use crate::assets::AssetError;
use axum::response::Response;
#[cfg(test)]
use std::io::Cursor;

/// Longest accepted provider URL (TMDb portraits are about 60 bytes).
const MAX_SOURCE_URL: usize = 2048;
/// Public key length: 128 bits of the SHA-256 of the provider URL.
const KEY_LEN: usize = 32;
/// Cached portraits are bounded to a 2x-3x density of the largest card (about 140 CSS pixels).
const MAX_WIDTH: u32 = 400;
const MAX_HEIGHT: u32 = 600;
/// A failed download is retried after this delay instead of on every page view.
const RETRY_FAILED_AFTER: i64 = 6 * 3600;
/// Credits handled per work, matching the provider-side bound on `actors`.
const MAX_CREDITS: usize = 100;

static DOWNLOADS: Semaphore = Semaphore::const_new(4);

/// Accepted provider portrait URL of one credit object, if any.
pub(crate) fn source_url(credit: &Value) -> Option<&str> {
    ["thumbUrl", "thumb_url", "thumb"]
        .iter()
        .find_map(|key| credit.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|url| {
            url.len() <= MAX_SOURCE_URL
                && url.starts_with("https://")
                && !url.chars().any(char::is_control)
        })
}

fn source_hash(url: &str) -> String {
    format!("{:x}", Sha256::digest(url.as_bytes()))
}

fn valid_key(key: &str) -> bool {
    key.len() == KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Catalogue view of `actors`: each credit with a usable provider portrait gains a Core-served
/// `imageUrl`. Credits without one are returned unchanged so clients fall back to initials.
pub(crate) fn decorate_credits(work: &str, credits: &[Value]) -> Vec<Value> {
    credits
        .iter()
        .take(MAX_CREDITS)
        .map(|credit| {
            let mut credit = credit.clone();
            let key = source_url(&credit).map(|url| source_hash(url)[..KEY_LEN].to_owned());
            if let (Some(key), Some(object)) = (key, credit.as_object_mut()) {
                object.insert(
                    "imageUrl".into(),
                    json!(format!("/api/v1/works/{work}/people/{key}/image")),
                );
            }
            credit
        })
        .collect()
}

/// Same merge as the catalogue: manual fields replace provider fields.
fn credits_of(editorial: &str, manual: Option<&str>) -> Vec<Value> {
    let mut metadata: Value = serde_json::from_str(editorial).unwrap_or_else(|_| json!({}));
    if let Some(manual) = manual
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|v| v.as_object().cloned())
        && let Some(object) = metadata.as_object_mut()
    {
        object.extend(manual);
    }
    ["actors", "crewCredits"]
        .into_iter()
        .filter_map(|key| metadata[key].as_array())
        .flat_map(|credits| credits.iter().take(MAX_CREDITS).cloned())
        .collect()
}

/// Enabled, granted extension allowed to read metadata whose declared hosts include `host`.
async fn provider_for(
    auth: &AuthState,
    library: &str,
    host: &str,
) -> Result<Option<Package>, Error> {
    let rows: Vec<(String,)> = query_as(
        "SELECT e.package_json \
            FROM extensions e \
            JOIN extension_publishers p ON p.id=e.publisher_id \
            JOIN libraries l ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g \
            WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 \
            FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id \
            WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) \
            WHERE l.id=? AND e.enabled=1 AND p.active=1 \
            ORDER BY e.id \
            LIMIT 65",
    )
    .bind(library)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(rows
        .into_iter()
        .filter_map(|(stored,)| serde_json::from_str::<Package>(&stored).ok())
        .find(|package| {
            package.manifest.http_hosts.iter().any(|h| h == host)
                && package
                    .manifest
                    .permissions
                    .iter()
                    .any(|p| p == "metadata.read" || p == "images")
        }))
}

struct Portrait {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

/// Decode with bounded allocation, then re-encode: provider bytes are never served verbatim.
fn normalize(bytes: Vec<u8>, content: &str) -> Result<Portrait, &'static str> {
    let decoded = crate::image_import::decode(&bytes, content).map_err(|status| {
        if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
            "unsupported_image"
        } else {
            "invalid_image"
        }
    })?;
    let resized = if decoded.width() > MAX_WIDTH || decoded.height() > MAX_HEIGHT {
        decoded.thumbnail(MAX_WIDTH, MAX_HEIGHT)
    } else {
        decoded
    };
    let rgb = image::DynamicImage::ImageRgb8(resized.to_rgb8());
    let out = crate::image_import::encode(&resized, resized.color().has_alpha())
        .map_err(|_| "invalid_image")?;
    Ok(Portrait {
        bytes: out,
        width: rgb.width(),
        height: rgb.height(),
    })
}

async fn cached(auth: &AuthState, hash: &str) -> Result<Option<(Option<String>, i64)>, Error> {
    query_as("SELECT sha256,fetched_at FROM person_images WHERE source_hash=?")
        .bind(hash)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)
}

async fn record_failure(auth: &AuthState, hash: &str, code: &str) -> Result<(), Error> {
    // Never downgrade a portrait another request stored meanwhile.
    query("INSERT INTO person_images(source_hash,sha256,width,height,error_code,fetched_at) VALUES(?,NULL,NULL,NULL,?,?) ON CONFLICT(source_hash) DO UPDATE SET error_code=excluded.error_code,fetched_at=excluded.fetched_at WHERE person_images.sha256 IS NULL")
        .bind(hash)
        .bind(code)
        .bind(now())
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(())
}

fn serve_error(_: AssetError) -> Error {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "person_image_storage_failed",
    )
}

/// `GET /api/v1/works/{work}/people/{key}/image`: portrait of a credit of a readable work.
pub(super) async fn read(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((work, key)): Path<(String, String)>,
) -> Result<Response, Error> {
    if !valid_key(&key) {
        return Err(err(StatusCode::NOT_FOUND, "unknown_person_image"));
    }
    crate::feedback::authorized(&auth, &headers, &work, false)
        .await
        .map_err(|status| err(status, "work_denied"))?;
    let row: Option<(String, String, Option<String>)> =
        query_as("SELECT library_id,editorial_json,manual_json FROM inventory_works WHERE id=?")
            .bind(&work)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (library, editorial, manual) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    // The key must name a credit of this work: no arbitrary URL or foreign cache entry is reachable.
    let url = credits_of(&editorial, manual.as_deref())
        .iter()
        .filter_map(|credit| source_url(credit).map(str::to_owned))
        .find(|url| source_hash(url)[..KEY_LEN] == key)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_person_image"))?;
    let hash = source_hash(&url);
    let fresh_failure = |at: i64| now() - at < RETRY_FAILED_AFTER;
    match cached(&auth, &hash).await? {
        Some((Some(sha256), _)) => {
            return auth
                .store
                .assets
                .response_after_authorization(&sha256, &headers)
                .await
                .map_err(serve_error);
        }
        Some((None, at)) if fresh_failure(at) => {
            return Err(err(StatusCode::NOT_FOUND, "person_image_unavailable"));
        }
        _ => {}
    }
    let _slot = tokio::time::timeout(Duration::from_secs(10), DOWNLOADS.acquire())
        .await
        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "person_image_busy"))?
        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "person_image_busy"))?;
    // Another request may have filled the cache while this one waited.
    match cached(&auth, &hash).await? {
        Some((Some(sha256), _)) => {
            return auth
                .store
                .assets
                .response_after_authorization(&sha256, &headers)
                .await
                .map_err(serve_error);
        }
        Some((None, at)) if fresh_failure(at) => {
            return Err(err(StatusCode::NOT_FOUND, "person_image_unavailable"));
        }
        _ => {}
    }
    let host = reqwest::Url::parse(&url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "person_image_unavailable"))?;
    // Not cached as a failure: enabling or granting a provider later makes the portrait available.
    let package = provider_for(&auth, &library, &host)
        .await?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "person_image_host_denied"))?;
    let (bytes, content) = match adapters::download_provider_image(&package, &url).await {
        Ok(downloaded) => downloaded,
        Err((StatusCode::REQUEST_TIMEOUT, _)) => {
            return Err(err(StatusCode::SERVICE_UNAVAILABLE, "person_image_timeout"));
        }
        Err(_) => {
            record_failure(&auth, &hash, "download_failed").await?;
            return Err(err(StatusCode::NOT_FOUND, "person_image_unavailable"));
        }
    };
    let permit = crate::image_import::DECODERS
        .acquire()
        .await
        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "person_image_busy"))?;
    let portrait = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        normalize(bytes, &content)
    })
    .await
    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "person_image_failed"))?;
    let portrait = match portrait {
        Ok(portrait) => portrait,
        Err(code) => {
            record_failure(&auth, &hash, code).await?;
            return Err(err(StatusCode::NOT_FOUND, "person_image_unavailable"));
        }
    };
    let meta = auth
        .store
        .assets
        .put_bytes(&portrait.bytes, "image/webp")
        .await
        .map_err(|_| {
            err(
                StatusCode::SERVICE_UNAVAILABLE,
                "person_image_storage_failed",
            )
        })?;
    query("INSERT INTO person_images(source_hash,sha256,width,height,error_code,fetched_at) VALUES(?,?,?,?,NULL,?) ON CONFLICT(source_hash) DO UPDATE SET sha256=excluded.sha256,width=excluded.width,height=excluded.height,error_code=NULL,fetched_at=excluded.fetched_at")
        .bind(&hash)
        .bind(&meta.sha256)
        .bind(i64::from(portrait.width))
        .bind(i64::from(portrait.height))
        .bind(now())
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    auth.store
        .assets
        .response_after_authorization(&meta.sha256, &headers)
        .await
        .map_err(serve_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityStore;

    #[test]
    fn only_https_provider_portraits_get_a_core_route() {
        let credits = vec![
            json!({"name":"A","thumbUrl":"https://image.example.org/w185/a.jpg"}),
            json!({"name":"B","thumb":"https://image.example.org/b.jpg"}),
            json!({"name":"C","thumbUrl":"http://image.example.org/c.jpg"}),
            json!({"name":"D","thumb":"/config/metadata/People/D/folder.jpg"}),
            json!({"name":"E"}),
            json!({"name":"F","thumbUrl":format!("https://image.example.org/{}", "x".repeat(MAX_SOURCE_URL))}),
        ];
        let decorated = decorate_credits("work-1", &credits);
        let urls: Vec<Option<&str>> = decorated
            .iter()
            .map(|credit| credit["imageUrl"].as_str())
            .collect();
        assert!(urls[0].is_some_and(
            |u| u.starts_with("/api/v1/works/work-1/people/") && u.ends_with("/image")
        ));
        assert!(urls[1].is_some());
        assert_ne!(urls[0], urls[1]);
        assert!(urls[2..].iter().all(Option::is_none));
        // Provider URLs are hashed: the route never carries the third-party address.
        assert!(!urls[0].unwrap().contains("example.org"));
        let key = urls[0].unwrap().split('/').nth(6).unwrap();
        assert!(valid_key(key));
        assert!(!valid_key("../../etc"));
        assert!(!valid_key(&key.to_uppercase()));
    }

    #[test]
    fn manual_credits_replace_provider_credits() {
        let editorial =
            r#"{"actors":[{"name":"Provider","thumbUrl":"https://image.example.org/p.jpg"}]}"#;
        let manual = r#"{"actors":[{"name":"Manual"}]}"#;
        assert_eq!(
            credits_of(editorial, Some(manual)),
            vec![json!({"name":"Manual"})]
        );
        assert_eq!(credits_of(editorial, None).len(), 1);
        assert!(credits_of("not json", None).is_empty());
    }

    #[test]
    fn crew_portraits_are_authorized_only_from_this_works_credits() {
        let editorial = json!({"actors":[{"name":"Cast"}],"crewCredits":[{"name":"Director","role":"Director","thumbUrl":"https://image.example.org/director.jpg"}]}).to_string();
        let credits = credits_of(&editorial, None);
        assert_eq!(credits.len(), 2);
        let decorated = decorate_credits("work-1", &credits);
        let key = decorated[1]["imageUrl"]
            .as_str()
            .unwrap()
            .split('/')
            .nth(6)
            .unwrap();
        assert_eq!(
            &source_hash(source_url(&credits[1]).unwrap())[..KEY_LEN],
            key
        );
        assert!(
            !credits_of("{}", None)
                .iter()
                .filter_map(source_url)
                .any(|url| &source_hash(url)[..KEY_LEN] == key)
        );
    }

    #[test]
    fn portraits_are_bounded_and_reencoded() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(800, 1200)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let portrait = normalize(png.clone(), "image/png").unwrap();
        assert_eq!((portrait.width, portrait.height), (400, 600));
        assert_eq!(
            image::guess_format(&portrait.bytes).unwrap(),
            image::ImageFormat::WebP
        );
        assert_eq!(
            normalize(png, "image/jpeg").err(),
            Some("unsupported_image")
        );
        assert_eq!(
            normalize(b"<svg/>".to_vec(), "image/svg+xml").err(),
            Some("unsupported_image")
        );
    }

    #[tokio::test]
    async fn portraits_require_a_reader_and_a_credit_of_the_work() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("people.sqlite"))
            .await
            .unwrap();
        let auth = AuthState::new(store, "http://localhost".into())
            .await
            .unwrap();
        let key = "0".repeat(KEY_LEN);
        assert_eq!(
            read(
                State(auth.clone()),
                HeaderMap::new(),
                Path(("work".into(), key))
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            read(
                State(auth),
                HeaderMap::new(),
                Path(("work".into(), "not-a-key".into()))
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::NOT_FOUND
        );
    }
}
