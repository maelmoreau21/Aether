//! Private artwork kept in application storage. Media folders remain read-only.
mod migration;
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::get,
};
#[cfg(test)]
use image::ImageFormat;
pub(crate) use migration::migrate_one;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Execute;
#[cfg(test)]
use std::io::Cursor;
pub(crate) const ROLES: &[&str] = &[
    "poster",
    "thumb",
    "logo",
    "illustration",
    "backdrop",
    "banner",
    "disc",
    "landscape",
];
#[derive(Default, Deserialize)]
pub struct PreviewOptions {
    pub width: Option<u32>,
}
pub async fn legacy(
    store: &IdentityStore,
    work: &str,
    width: u32,
    headers: &HeaderMap,
) -> Result<Response, StatusCode> {
    if ![0, 320, 640, 1280].contains(&width) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (revision, sha256): (String, String) =
        query_as("SELECT revision,sha256 FROM work_posters WHERE work_id=?")
            .bind(work)
            .fetch_optional(&store.pool)
            .await
            .map_err(db)?
            .ok_or(StatusCode::NOT_FOUND)?;
    if width == 0 {
        return store
            .assets
            .response_after_authorization(&sha256, headers)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }
    let cached: Option<String> = query_scalar(
        "SELECT sha256 FROM poster_previews WHERE work_id=? AND width=? AND revision=?",
    )
    .bind(work)
    .bind(i64::from(width))
    .bind(&revision)
    .fetch_optional(&store.pool)
    .await
    .map_err(db)?;

    // Missing previews are prepared by import/migration, never by a GET.
    let target_sha = cached.unwrap_or(sha256);
    store
        .assets
        .response_after_authorization(&target_sha, headers)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
use crate::image_import::DECODERS;
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub work_id: String,
    pub kind: String,
    pub position: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub url: String,
    pub origin: String,
    pub language: Option<String>,
    pub provider: Option<String>,
    #[sqlx(default)]
    #[serde(serialize_with = "serialize_flag")]
    pub pinned: i64,
    #[sqlx(default)]
    pub region: Option<String>,
    #[sqlx(default)]
    pub content_hash: Option<String>,
    #[sqlx(default)]
    pub representation_count: i64,
    #[sqlx(default)]
    pub release_evidence: Option<String>,
    #[sqlx(default)]
    #[serde(serialize_with = "serialize_flag")]
    pub original_release_verified: i64,
}
fn serialize_flag<S: serde::Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bool(*value != 0)
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/works/{work}/images", get(list))
        .route(
            "/api/v1/works/{work}/images/order",
            axum::routing::put(order),
        )
        .route("/api/v1/works/{work}/images/similar", get(similar_images))
        .route(
            "/api/v1/works/{work}/images/{id}/pin",
            axum::routing::put(pin),
        )
        .route(
            "/api/v1/works/{work}/images/{id}",
            get(read).post(upload).delete(remove),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(auth)
}
async fn allowed(
    auth: &AuthState,
    headers: &HeaderMap,
    work: &str,
    write: bool,
) -> Result<(), StatusCode> {
    if write {
        let actor = auth.authenticate(headers).await?;
        if actor.user.role != Role::Administrator {
            return Err(StatusCode::FORBIDDEN);
        }
        auth.require_csrf(headers, &actor)?;
    }
    crate::feedback::authorized(auth, headers, work, false).await?;
    Ok(())
}
fn db(_: sqlx::Error) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}
// One bounded page, three batched metadata reads instead of per-card SQL round trips.
pub async fn entries(store: &IdentityStore, ids: &[String]) -> Result<Vec<Entry>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for sql in [
        "SELECT e.id,e.work_id,e.kind,COALESCE(o.position,e.position) AS position,e.width,e.height,'/api/v1/works/'||e.work_id||'/images/'||e.id AS url,CASE WHEN e.provider IS NULL THEN 'manual' ELSE 'provider' END AS origin,e.language,e.provider,(SELECT sha256 FROM work_image_data WHERE image_id=e.id AND width=0) AS content_hash,(SELECT COUNT(*) FROM work_image_data WHERE image_id=e.id) AS representation_count FROM work_image_entries e LEFT JOIN work_image_order o ON o.work_id=e.work_id AND o.image_id=e.id WHERE e.work_id IN (",
        "SELECT i.id,i.work_id,i.role AS kind,COALESCE(o.position,10000+i.priority) AS position,CAST(NULL AS BIGINT) AS width,CAST(NULL AS BIGINT) AS height,'/api/v1/assets/'||i.id||'/image?v='||i.modified_ns||'-'||i.size AS url,'local' AS origin,CAST(NULL AS TEXT) AS language,CAST(NULL AS TEXT) AS provider FROM inventory_images i JOIN libraries l ON l.id=i.library_id JOIN media_nodes n ON n.id=l.node_id LEFT JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id LEFT JOIN work_image_order o ON o.work_id=i.work_id AND o.image_id=i.id WHERE i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND n.status='connected' AND h.image_id IS NULL AND i.role IN ('poster','thumb','logo','illustration','backdrop','banner','disc','landscape') AND i.work_id IN (",
        "SELECT 'legacy-poster' AS id,p.work_id,'poster' AS kind,5000 AS position,CAST(NULL AS BIGINT) AS width,CAST(NULL AS BIGINT) AS height,'/api/v1/works/'||p.work_id||'/poster' AS url,'legacy' AS origin,CAST(NULL AS TEXT) AS language,CAST(NULL AS TEXT) AS provider FROM work_posters p WHERE p.work_id IN (",
    ] {
        let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(sql);
        let mut values = builder.separated(",");
        for id in ids {
            values.push_bind(id);
        }
        values.push_unseparated(")");
        let sql = crate::database::normalize_sql(builder.sql().as_str());
        let args = builder
            .build()
            .take_arguments()
            .map_err(sqlx::Error::Encode)?
            .unwrap_or_default();
        result.extend(
            sqlx::query_as_with::<sqlx::Any, Entry, _>(sqlx::AssertSqlSafe(sql), args)
                .fetch_all(&store.pool)
                .await?,
        );
    }
    let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT work_id,image_id,pinned,release_evidence FROM artwork_annotations WHERE work_id IN (",
    );
    let mut values = builder.separated(",");
    for id in ids {
        values.push_bind(id);
    }
    values.push_unseparated(")");
    let sql = crate::database::normalize_sql(builder.sql().as_str());
    let args = builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let annotations: Vec<(String, String, i64, Option<String>)> =
        sqlx::query_as_with(sqlx::AssertSqlSafe(sql), args)
            .fetch_all(&store.pool)
            .await?;
    for entry in &mut result {
        if let Some((_, _, pinned, evidence)) = annotations
            .iter()
            .find(|(work, id, _, _)| work == &entry.work_id && id == &entry.id)
        {
            entry.pinned = *pinned;
            entry.release_evidence = evidence.clone();
            entry.original_release_verified = i64::from(evidence.is_some());
        }
        entry.region = entry
            .language
            .as_deref()
            .and_then(|language| {
                language.split('-').skip(1).find(|part| {
                    (part.len() == 2 && part.bytes().all(|b| b.is_ascii_alphabetic()))
                        || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit()))
                })
            })
            .map(str::to_ascii_uppercase);
    }
    result.sort_by(|a, b| {
        (
            &a.work_id,
            &a.kind,
            std::cmp::Reverse(a.pinned),
            a.position,
            &a.id,
        )
            .cmp(&(
                &b.work_id,
                &b.kind,
                std::cmp::Reverse(b.pinned),
                b.position,
                &b.id,
            ))
    });
    Ok(result)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    pinned: bool,
}
async fn pin(
    State(auth): State<AuthState>,
    Path((work, id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(input): Json<Pin>,
) -> Result<StatusCode, StatusCode> {
    allowed(&auth, &headers, &work, true).await?;
    let images = entries(&auth.store, std::slice::from_ref(&work))
        .await
        .map_err(db)?;
    let image = images
        .iter()
        .find(|image| image.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut connection = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut connection, 0x41525450494e)
        .await
        .map_err(db)?;
    if input.pinned {
        query("UPDATE artwork_annotations SET pinned=0 WHERE work_id=? AND kind=?")
            .bind(&work)
            .bind(&image.kind)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    query("INSERT INTO artwork_annotations(work_id,image_id,kind,pinned) VALUES(?,?,?,?) ON CONFLICT(work_id,image_id) DO UPDATE SET pinned=excluded.pinned")
        .bind(&work).bind(&id).bind(&image.kind).bind(i64::from(input.pinned)).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn list(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    allowed(&auth, &headers, &work, false).await?;
    let original = original_language(&auth.store, &work).await.map_err(db)?;
    let images: Vec<_> = entries(&auth.store, &[work])
        .await
        .map_err(db)?
        .into_iter()
        .collect();
    Ok(Json(json!({"images":images,"originalLanguage":original})))
}
pub(crate) async fn original_language(
    store: &IdentityStore,
    work: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String, Option<String>)> =
        query_as("SELECT editorial_json,manual_json FROM inventory_works WHERE id=?")
            .bind(work)
            .fetch_optional(store.pool())
            .await?;
    let Some((editorial, manual)) = row else {
        return Ok(None);
    };
    let editorial: Value = serde_json::from_str(&editorial).unwrap_or(Value::Null);
    let manual: Value = manual
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or(Value::Null);
    let language = manual
        .get("original_language")
        .unwrap_or(&editorial["original_language"]);
    Ok(language
        .as_str()
        .map(str::trim)
        .filter(|v| valid_language(v) && !["und", "xx", "zxx"].contains(v))
        .map(str::to_ascii_lowercase))
}
#[derive(Default, Deserialize)]
pub(crate) struct ImageQuery {
    pub(crate) width: Option<u32>,
    pub(crate) replace: Option<String>,
    #[serde(default)]
    pub(crate) append: bool,
    pub(crate) language: Option<String>,
    #[serde(skip)]
    pub(crate) provider: Option<String>,
    #[serde(skip)]
    pub(crate) source_url_hash: Option<String>,
    #[serde(skip)]
    pub(crate) apply_collection_policy: bool,
}
async fn read(
    State(auth): State<AuthState>,
    Path((work, id)): Path<(String, String)>,
    Query(options): Query<ImageQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    allowed(&auth, &headers, &work, false).await?;
    let width = options.width.unwrap_or(0);
    if ![0, 320, 640, 1280].contains(&width) {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Check existence/ownership before returning 304; a deleted image cannot be revived by a validator.
    let exists: i64 =
        query_scalar("SELECT COUNT(*) FROM work_image_entries WHERE id=? AND work_id=?")
            .bind(&id)
            .bind(&work)
            .fetch_one(&auth.store.pool)
            .await
            .map_err(db)?;
    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let sha256: String = query_scalar("SELECT d.sha256 FROM work_image_data d JOIN work_image_entries e ON e.id=d.image_id WHERE e.work_id=? AND e.id=? AND d.width=?")
        .bind(work)
        .bind(id)
        .bind(i64::from(width))
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(db)?
        .ok_or(StatusCode::NOT_FOUND)?;
    auth.store
        .assets
        .response_after_authorization(&sha256, &headers)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn valid_language(value: &str) -> bool {
    value.len() <= 35
        && value.split('-').enumerate().all(|(index, part)| {
            (if index == 0 {
                (2..=8).contains(&part.len())
            } else {
                !part.is_empty() && part.len() <= 8
            }) && part.bytes().all(|b| {
                if index == 0 {
                    b.is_ascii_alphabetic()
                } else {
                    b.is_ascii_alphanumeric()
                }
            })
        })
}
struct Fingerprint(String);
impl Fingerprint {
    fn of(image: &image::DynamicImage) -> Self {
        // Composite only tiny samples; preserve alpha without another full-size image allocation.
        let sample = |width, height| {
            let rgba = image
                .resize_exact(width, height, image::imageops::FilterType::Triangle)
                .to_rgba8();
            image::RgbImage::from_fn(width, height, |x, y| {
                let pixel = rgba.get_pixel(x, y);
                let alpha = u32::from(pixel[3]);
                image::Rgb(std::array::from_fn(|index| {
                    ((u32::from(pixel[index]) * alpha + 128 * (255 - alpha)) / 255) as u8
                }))
            })
        };
        let pixels = sample(9, 8);
        let mut hash = 0u64;
        for y in 0..8 {
            for x in 0..8 {
                let luma = |p: &image::Rgb<u8>| {
                    299u32 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2])
                };
                hash = (hash << 1)
                    | u64::from(luma(pixels.get_pixel(x, y)) > luma(pixels.get_pixel(x + 1, y)));
            }
        }
        let colors = sample(4, 4);
        Self(format!(
            "{hash:016x}:{}:{}",
            colors
                .as_raw()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>(),
            sample(16, 16)
                .as_raw()
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        ))
    }
    fn similar(
        &self,
        other: &str,
        width: i64,
        height: i64,
        other_width: i64,
        other_height: i64,
        tolerance: u32,
    ) -> bool {
        // A crop or a different aspect ratio may change the composition substantially.
        if height <= 0
            || other_height <= 0
            || ((width as f64 / height as f64) / (other_width as f64 / other_height as f64) - 1.0)
                .abs()
                > 0.04
        {
            return false;
        }
        type Parsed = (u64, Vec<u8>, Option<Vec<u8>>);
        let parse = |value: &str| -> Option<Parsed> {
            let mut parts = value.split(':');
            let hash = parts.next()?;
            let colors = parts.next()?;
            let detail = parts.next();
            if parts.next().is_some()
                || colors.len() != 96
                || !colors.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            let bytes = |hex: &str| {
                (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
            };
            let detail = match detail {
                None => None,
                Some(detail)
                    if detail.len() == 1536 && detail.bytes().all(|b| b.is_ascii_hexdigit()) =>
                {
                    Some(bytes(detail)?)
                }
                _ => return None,
            };
            Some((u64::from_str_radix(hash, 16).ok()?, bytes(colors)?, detail))
        };
        let (Some((hash, colors, detail)), Some((other_hash, other_colors, other_detail))) =
            (parse(&self.0), parse(other))
        else {
            return false;
        };
        if let (Some(detail), Some(other)) = (detail, other_detail) {
            let differences: Vec<u32> = detail
                .iter()
                .zip(other)
                .map(|(a, b)| u32::from(a.abs_diff(b)))
                .collect();
            // The finer grid catches changed art/layout that 4x4 colours miss.
            // Older signatures remain readable until the progressive migration.
            if differences.iter().sum::<u32>() > 768 * 8
                || differences.iter().filter(|v| **v > 24).count() > 76
            {
                return false;
            }
        }
        (hash ^ other_hash).count_ones() <= tolerance
            && colors
                .iter()
                .zip(other_colors)
                .map(|(a, b)| u32::from(a.abs_diff(b)))
                .sum::<u32>()
                <= 48 * 10
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarQuery {
    #[serde(default = "default_similarity")]
    threshold: u32,
}
fn default_similarity() -> u32 {
    5
}

type ImageEntryRow = (String, String, Option<String>, i64, i64, Option<String>);

/// Read-only suggestions. Never deletes artwork, even when a match is found.
async fn similar_images(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    Query(options): Query<SimilarQuery>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<Value>), StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    allowed(&auth, &headers, &work, false).await?;
    if options.threshold > 16 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let rows: Vec<ImageEntryRow> = query_as(
        "SELECT id,kind,language,width,height,fingerprint FROM work_image_entries WHERE work_id=? ORDER BY kind,position,id LIMIT 65"
    ).bind(&work).fetch_all(auth.store.pool()).await.map_err(db)?;
    if rows.len() > 64 {
        return Err(StatusCode::CONFLICT);
    }
    let mut pairs = Vec::new();
    let mut truncated = false;
    'outer: for (index, (id, kind, language, width, height, fingerprint)) in rows.iter().enumerate()
    {
        let Some(fingerprint) = fingerprint else {
            continue;
        };
        let signature = Fingerprint(fingerprint.clone());
        for (other_id, other_kind, other_language, other_width, other_height, other_fingerprint) in
            rows.iter().skip(index + 1)
        {
            if kind == other_kind
                && other_fingerprint.as_ref().is_some_and(|other| {
                    signature.similar(
                        other,
                        *width,
                        *height,
                        *other_width,
                        *other_height,
                        options.threshold,
                    )
                })
            {
                if pairs.len() == 128 {
                    truncated = true;
                    break 'outer;
                }
                pairs.push(json!({"first":id,"second":other_id,"differentLanguages":language != other_language}));
            }
        }
    }
    let uncompared = rows
        .iter()
        .filter(|row| {
            row.5.as_ref().is_none_or(|fingerprint| {
                !Fingerprint(fingerprint.clone()).similar(
                    fingerprint,
                    row.3,
                    row.4,
                    row.3,
                    row.4,
                    0,
                )
            })
        })
        .count();
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CACHE_CONTROL,
        "private, no-store"
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok((
        response_headers,
        Json(json!({"pairs":pairs,"uncompared":uncompared,"truncated":truncated})),
    ))
}

pub(crate) struct Encoded {
    pub(crate) width: i64,
    pub(crate) height: i64,
    pub(crate) variants: Vec<(i64, &'static str, Vec<u8>)>,
    fingerprint: Fingerprint,
}
#[cfg(test)]
fn normalize(bytes: Bytes, content: &str) -> Result<Encoded, StatusCode> {
    normalize_kind(bytes, content, "poster")
}
fn normalize_kind(bytes: Bytes, content: &str, kind: &str) -> Result<Encoded, StatusCode> {
    let image = crate::image_import::decode(&bytes, content)?;
    let fingerprint = Fingerprint::of(&image);
    let mut variants = vec![];
    let lossless = kind == "logo" || image.color().has_alpha();
    let mut source = None;
    for width in [0, 320, 640, 1280] {
        // Keep the historical width keys even for a smaller original. Those keys
        // reference the same content-addressed object; no upscaling or re-encoding.
        let bytes = if width == 0 {
            let bytes = if content == "image/webp"
                && media_node::image_encoding::reusable_webp(&bytes, lossless)
            {
                bytes.to_vec()
            } else {
                crate::image_import::encode(&image, lossless)?
            };
            source = Some(bytes.clone());
            bytes
        } else if image.width() <= width {
            source.clone().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
        } else {
            crate::image_import::encode(&image.thumbnail(width, 4096), lossless)?
        };
        variants.push((i64::from(width), "image/webp", bytes));
    }
    Ok(Encoded {
        width: i64::from(image.width()),
        height: i64::from(image.height()),
        variants,
        fingerprint,
    })
}
pub(crate) async fn prepare(
    bytes: Bytes,
    content: String,
    kind: String,
) -> Result<Encoded, StatusCode> {
    let permit = tokio::time::timeout(std::time::Duration::from_secs(8), DECODERS.acquire())
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        normalize_kind(bytes, &content, &kind)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}
async fn reusable(
    store: &IdentityStore,
    digest: &str,
    kind: &str,
) -> Result<Option<Encoded>, StatusCode> {
    let entry: Option<(String,i64,i64,String)> = query_as("SELECT e.id,e.width,e.height,e.fingerprint FROM artwork_annotations a JOIN work_image_entries e ON e.work_id=a.work_id AND e.id=a.image_id JOIN work_image_data d ON d.image_id=e.id AND d.width=0 WHERE a.encoding=? AND a.kind=? AND (a.source_sha256=? OR d.sha256=?) AND e.fingerprint IS NOT NULL LIMIT 1")
        .bind(crate::image_import::ENCODING).bind(kind).bind(digest).bind(digest).fetch_optional(store.pool()).await.map_err(db)?;
    let Some((id, width, height, fingerprint)) = entry else {
        return Ok(None);
    };
    let data: Vec<(i64, String)> =
        query_as("SELECT width,sha256 FROM work_image_data WHERE image_id=? ORDER BY width")
            .bind(id)
            .fetch_all(store.pool())
            .await
            .map_err(db)?;
    if data.iter().map(|r| r.0).collect::<Vec<_>>() != [0, 320, 640, 1280] {
        return Ok(None);
    }
    let mut variants = Vec::new();
    for (width, sha) in data {
        let (_, bytes) = store
            .assets
            .read_all(&sha)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        variants.push((width, "image/webp", bytes));
    }
    Ok(Some(Encoded {
        width,
        height,
        variants,
        fingerprint: Fingerprint(fingerprint),
    }))
}
pub(crate) async fn upload(
    State(auth): State<AuthState>,
    Path((work, kind)): Path<(String, String)>,
    Query(options): Query<ImageQuery>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    allowed(&auth, &headers, &work, true).await?;
    let content = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    upload_internal(&auth.store, work, kind, options, content, bytes).await
}

pub(crate) async fn upload_internal(
    store: &IdentityStore,
    work: String,
    kind: String,
    options: ImageQuery,
    content: String,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    if !ROLES.contains(&kind.as_str())
        || options
            .language
            .as_ref()
            .is_some_and(|v| !valid_language(v))
        || (options.append && options.replace.is_some())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let language = options.language.as_ref().map(|v| v.to_ascii_lowercase());
    let existing = entries(store, std::slice::from_ref(&work))
        .await
        .map_err(db)?;
    if let Some(replace) = &options.replace
        && !existing.iter().any(|e| &e.id == replace && e.kind == kind)
    {
        return Err(StatusCode::NOT_FOUND);
    }
    use sha2::{Digest, Sha256};
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let guessed = image::guess_format(&bytes).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if guessed.to_mime_type() != content {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let encoded = match reusable(store, &source_sha256, &kind).await? {
        Some(reused) => reused,
        None => prepare(bytes, content, kind.clone()).await?,
    };
    // Provider additions use the owning library's policy; uploads and explicit
    // replacements keep their existing behaviour. Child folders inherit the
    // options of the parent library.
    let policy = if options.append && options.apply_collection_policy {
        let settings: String = query_scalar("SELECT COALESCE(parent.options_json,l.options_json) FROM inventory_works w JOIN libraries l ON l.id=w.library_id LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 LEFT JOIN libraries parent ON parent.id=f.parent_id WHERE w.id=?")
            .bind(&work).fetch_one(&store.pool).await.map_err(db)?;
        let policy: crate::library_options::LibraryOptions =
            serde_json::from_str(&settings).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !policy.validate() {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        if kind == "poster"
            && (encoded.width < i64::from(policy.artwork_min_poster_width)
                || encoded.height < i64::from(policy.artwork_min_poster_height))
        {
            return Ok((
                StatusCode::OK,
                Json(json!({"skipped":true,"reason":"poster_too_small"})),
            ));
        }
        Some(policy)
    } else {
        None
    };
    let mut stored_variants = Vec::with_capacity(encoded.variants.len());
    let mut leases = Vec::new();
    for (width, mime, data) in &encoded.variants {
        let meta = store
            .assets
            .put_bytes(data, mime)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        leases.push(
            store
                .assets
                .lease(&meta.sha256)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        stored_variants.push((*width, *mime, meta.sha256));
    }
    let mut connection = store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut connection)
        .await
        .map_err(db)?;
    if options.append {
        let candidates: Vec<(String, String, i64, i64)> = query_as("SELECT id,fingerprint,width,height FROM work_image_entries WHERE work_id=? AND kind=? AND COALESCE(language,'')=COALESCE(?,'') AND fingerprint IS NOT NULL")
            .bind(&work).bind(&kind).bind(&language).fetch_all(&mut *tx).await.map_err(db)?;
        if let Some((id, _, _, _)) = candidates.iter().find(|(_, fingerprint, width, height)| {
            if policy.as_ref().is_some_and(|p| !p.artwork_deduplicate) {
                return false;
            }
            encoded.fingerprint.similar(
                fingerprint,
                encoded.width,
                encoded.height,
                *width,
                *height,
                policy
                    .as_ref()
                    .map_or(5, |p| u32::from(p.artwork_similarity)),
            )
        }) {
            if let Some(source) = &options.source_url_hash {
                query("INSERT INTO artwork_image_sources(work_id,source_url_hash,image_id) VALUES(?,?,?) ON CONFLICT(work_id,source_url_hash) DO UPDATE SET image_id=excluded.image_id")
                    .bind(&work).bind(source).bind(id).execute(&mut *tx).await.map_err(db)?;
            }
            tx.commit().await.map_err(db)?;
            return Ok((StatusCode::OK, Json(json!({"id":id,"duplicate":true}))));
        }
        let same_kind: i64 =
            query_scalar("SELECT COUNT(*) FROM work_image_entries WHERE work_id=? AND kind=?")
                .bind(&work)
                .bind(&kind)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if same_kind >= 12 {
            return Err(StatusCode::CONFLICT);
        }
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM work_image_entries WHERE work_id=?")
        .bind(&work)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(replace) = &options.replace {
        query("DELETE FROM artwork_annotations WHERE work_id=? AND image_id=?")
            .bind(&work)
            .bind(replace)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        query("DELETE FROM work_image_entries WHERE work_id=? AND id=?")
            .bind(&work)
            .bind(replace)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if replace == "legacy-poster" {
            query("DELETE FROM work_posters WHERE work_id=?")
                .bind(&work)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        if existing
            .iter()
            .any(|e| &e.id == replace && e.origin == "local")
        {
            query("INSERT INTO work_image_hidden(work_id,image_id) VALUES(?,?) ON CONFLICT DO NOTHING")
            .bind(&work)
            .bind(replace)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        query("DELETE FROM work_image_order WHERE work_id=? AND image_id=?")
            .bind(&work)
            .bind(replace)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    if kind != "backdrop" && !options.append && options.provider.is_none() {
        query("DELETE FROM artwork_annotations WHERE work_id=? AND image_id IN (SELECT id FROM work_image_entries WHERE work_id=? AND kind=? AND provider IS NULL)")
            .bind(&work).bind(&work).bind(&kind).execute(&mut *tx).await.map_err(db)?;
        query("DELETE FROM work_image_entries WHERE work_id=? AND kind=? AND provider IS NULL")
            .bind(&work)
            .bind(&kind)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    let remaining: i64 = query_scalar("SELECT COUNT(*) FROM work_image_entries WHERE work_id=?")
        .bind(&work)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if remaining >= 64 {
        return Err(StatusCode::CONFLICT);
    }
    let id = uuid::Uuid::new_v4().to_string();
    query(
        "INSERT INTO work_image_entries(id,work_id,kind,position,width,height,language,provider,fingerprint,source_url_hash) VALUES(?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&work)
    .bind(&kind)
    .bind(if options.provider.is_none() { -1 } else { count })
    .bind(encoded.width)
    .bind(encoded.height)
    .bind(language)
    .bind(options.provider)
    .bind(encoded.fingerprint.0)
    .bind(&options.source_url_hash)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if let Some(source) = &options.source_url_hash {
        query("INSERT INTO artwork_image_sources(work_id,source_url_hash,image_id) VALUES(?,?,?) ON CONFLICT(work_id,source_url_hash) DO UPDATE SET image_id=excluded.image_id")
            .bind(&work).bind(source).bind(&id).execute(&mut *tx).await.map_err(db)?;
    }
    for (width, mime, sha256) in stored_variants {
        query("INSERT INTO work_image_data(image_id,width,mime,sha256) VALUES(?,?,?,?)")
            .bind(&id)
            .bind(width)
            .bind(mime)
            .bind(&sha256)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    query("INSERT INTO artwork_annotations(work_id,image_id,kind,source_sha256,encoding) VALUES(?,?,?,?,?)")
        .bind(&work).bind(&id).bind(&kind).bind(&source_sha256).bind(crate::image_import::ENCODING)
        .execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok((
        StatusCode::CREATED,
        Json(json!({"id":id,"duplicate":false})),
    ))
}
async fn remove(
    State(auth): State<AuthState>,
    Path((work, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    allowed(&auth, &headers, &work, true).await?;
    let entries = entries(&auth.store, std::slice::from_ref(&work))
        .await
        .map_err(db)?;
    if !entries.iter().any(|e| e.id == id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut tx = auth.store.pool.begin().await.map_err(db)?;
    query("DELETE FROM artwork_annotations WHERE work_id=? AND image_id=?")
        .bind(&work)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    query("DELETE FROM work_image_entries WHERE work_id=? AND id=?")
        .bind(&work)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if id == "legacy-poster" {
        query("DELETE FROM work_posters WHERE work_id=?")
            .bind(&work)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    if entries.iter().any(|e| e.id == id && e.origin == "local") {
        query("INSERT INTO work_image_hidden(work_id,image_id) VALUES(?,?) ON CONFLICT DO NOTHING")
            .bind(&work)
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    query("DELETE FROM work_image_order WHERE work_id=? AND image_id=?")
        .bind(&work)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Order {
    ids: Vec<String>,
}
async fn order(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Order>,
) -> Result<StatusCode, StatusCode> {
    allowed(&auth, &headers, &work, true).await?;
    let existing = entries(&auth.store, std::slice::from_ref(&work))
        .await
        .map_err(db)?;
    if input.ids.len() > 32
        || input.ids.iter().enumerate().any(|(i, id)| {
            input.ids[..i].contains(id)
                || !existing.iter().any(|e| &e.id == id && e.kind == "backdrop")
        })
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut tx = auth.store.pool.begin().await.map_err(db)?;
    for (position, id) in input.ids.iter().enumerate() {
        query("INSERT INTO work_image_order(work_id,image_id,position) VALUES(?,?,?) ON CONFLICT(work_id,image_id) DO UPDATE SET position=excluded.position").bind(&work).bind(id).bind(position as i64).execute(&mut *tx).await.map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn provider_artwork_policy_inherits_library_and_preserves_manual_uploads() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap(
                "admin",
                zeroize::Zeroizing::new("synthetic artwork password".into()),
            )
            .await
            .unwrap();
        crate::libraries::initialize_local(&store).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES('parent','Parent','films','local','parent',1,?)")
            .bind(r#"{"artworkDeduplicate":true,"artworkSimilarity":5,"artworkMinPosterWidth":20,"artworkMinPosterHeight":20}"#)
            .execute(store.pool()).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('child','Child','films','local','child',1)").execute(store.pool()).await.unwrap();
        query(
            "INSERT INTO library_folders(library_id,parent_id,active) VALUES('child','parent',1)",
        )
        .execute(store.pool())
        .await
        .unwrap();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','child','work','Test','{}')").execute(store.pool()).await.unwrap();
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let app = crate::auth::router(auth.clone());
        let login = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ORIGIN, "http://127.0.0.1:3000")
                    .body(Body::from(
                        r#"{"login":"admin","password":"synthetic artwork password"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body = login.into_body().collect().await.unwrap().to_bytes();
        let csrf = serde_json::from_slice::<Value>(&body).unwrap()["csrfToken"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
        headers.insert(header::ORIGIN, "http://127.0.0.1:3000".parse().unwrap());
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        headers.insert("x-csrf-token", csrf.parse().unwrap());
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            12,
            18,
            image::Rgb([255, 0, 0]),
        ))
        .write_to(&mut png, ImageFormat::Png)
        .unwrap();
        let bytes = Bytes::from(png.into_inner());
        let provider = || ImageQuery {
            append: true,
            provider: Some("test".into()),
            apply_collection_policy: true,
            ..Default::default()
        };
        let result = upload(
            State(auth.clone()),
            Path(("work".into(), "poster".into())),
            Query(provider()),
            headers.clone(),
            bytes.clone(),
        )
        .await
        .unwrap();
        assert_eq!(result.1.0["reason"], "poster_too_small");
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM work_image_entries")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            0
        );
        let manual = upload(
            State(auth.clone()),
            Path(("work".into(), "poster".into())),
            Query(ImageQuery::default()),
            headers.clone(),
            bytes.clone(),
        )
        .await
        .unwrap();
        assert_eq!(manual.0, StatusCode::CREATED);
        query("UPDATE libraries SET options_json=? WHERE id='parent'")
            .bind(r#"{"artworkDeduplicate":true,"artworkSimilarity":5}"#)
            .execute(store.pool())
            .await
            .unwrap();
        let duplicate = upload(
            State(auth.clone()),
            Path(("work".into(), "poster".into())),
            Query(provider()),
            headers.clone(),
            bytes.clone(),
        )
        .await
        .unwrap();
        assert_eq!(duplicate.1.0["duplicate"], true);
        query("UPDATE libraries SET options_json=? WHERE id='parent'")
            .bind(r#"{"artworkDeduplicate":false}"#)
            .execute(store.pool())
            .await
            .unwrap();
        let retained = upload(
            State(auth.clone()),
            Path(("work".into(), "poster".into())),
            Query(provider()),
            headers,
            bytes,
        )
        .await
        .unwrap();
        assert_eq!(retained.0, StatusCode::CREATED);
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM work_image_entries")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            2
        );
        let app = router(auth);
        let uri = "/api/v1/works/work/images/similar?threshold=5";
        let anonymous = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        query("INSERT INTO users(id,role) VALUES('reader','user')")
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO library_permissions(library_id,user_id) VALUES('child','reader')")
            .execute(store.pool())
            .await
            .unwrap();
        let token = "b".repeat(64);
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?, 'reader',unixepoch(),unixepoch(),unixepoch()+3600)").bind(crate::auth::hash(&token)).execute(store.pool()).await.unwrap();
        let reader = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::COOKIE, format!("media_session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reader.status(), StatusCode::FORBIDDEN);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let suggestions: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(suggestions["pairs"].as_array().unwrap().len(), 1);
        assert_eq!(suggestions["uncompared"], 0);
        let invalid = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/works/work/images/similar?threshold=17")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM work_image_entries")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            2
        );
        for index in 0..16 {
            query("INSERT INTO work_image_entries(id,work_id,kind,position,width,height,fingerprint) SELECT ?,work_id,kind,position,width,height,fingerprint FROM work_image_entries WHERE work_id='work' LIMIT 1")
                .bind(format!("extra-{index}"))
                .execute(store.pool()).await.unwrap();
        }
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let suggestions: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(suggestions["pairs"].as_array().unwrap().len(), 128);
        assert_eq!(suggestions["truncated"], true);
    }
    #[test]
    fn perceptual_signature_compares_decoded_pixels_and_preserves_language() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(120, 180, |x, y| {
            image::Rgb([(x * 2) as u8, y as u8, 80])
        }));
        let signature = Fingerprint::of(&image);
        let resized = image.resize_exact(240, 360, image::imageops::FilterType::Triangle);
        assert!(signature.similar(&Fingerprint::of(&resized).0, 120, 180, 240, 360, 5));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 70)
            .encode_image(&image)
            .unwrap();
        let recompressed = image::load_from_memory(&jpeg).unwrap();
        assert!(signature.similar(&Fingerprint::of(&recompressed).0, 120, 180, 120, 180, 5));
        let opaque = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            120,
            180,
            image::Rgba([255, 255, 255, 255]),
        ));
        let transparent = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            120,
            180,
            image::Rgba([255, 255, 255, 0]),
        ));
        assert!(!Fingerprint::of(&opaque).similar(
            &Fingerprint::of(&transparent).0,
            120,
            180,
            120,
            180,
            5
        ));
        let blue = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            120,
            180,
            image::Rgb([0, 0, 255]),
        ));
        let red = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            120,
            180,
            image::Rgb([255, 0, 0]),
        ));
        assert!(!Fingerprint::of(&blue).similar(&Fingerprint::of(&red).0, 120, 180, 120, 180, 5));
        assert!(!signature.similar(&signature.0, 120, 180, 180, 120, 5));
        assert!(!signature.similar("invalid", 120, 180, 120, 180, 5));
        let fingerprint = Fingerprint(format!("{:016x}:{}", 0u64, "00".repeat(48)));
        let near = format!("{:016x}:{}", 0b111u64, "00".repeat(48));
        assert!(!fingerprint.similar(&near, 120, 180, 120, 180, 2));
        assert!(fingerprint.similar(&near, 120, 180, 120, 180, 3));
        assert!(valid_language("fr-CA"));
        assert!(valid_language("zh-Hant-TW"));
        assert!(!valid_language("fr--CA"));
        assert!(!valid_language("fr<script>"));
    }
    #[test]
    fn bounded_representations_preserve_dimensions_and_reduce_transfer() {
        let mut state = 17u32;
        let pixels = image::RgbImage::from_fn(640, 960, |_, _| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            image::Rgb([(state >> 24) as u8, (state >> 16) as u8, (state >> 8) as u8])
        });
        let mut bytes = Cursor::new(vec![]);
        image::DynamicImage::ImageRgb8(pixels)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let source_bytes = bytes.get_ref().len();
        let normalized = normalize(Bytes::from(bytes.into_inner()), "image/png").unwrap();
        assert_eq!((normalized.width, normalized.height), (640, 960));
        let preview = normalized.variants.iter().find(|v| v.0 == 320).unwrap();
        assert!(preview.2.len() < source_bytes / 4);
        println!(
            "artwork PNG bytes={source_bytes}, 320px preview bytes={}",
            preview.2.len()
        );
        let decoded = image::load_from_memory(&preview.2).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 480));
        assert!(normalize(Bytes::from_static(b"invalid"), "image/png").is_err());
    }
}
