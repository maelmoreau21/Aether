//! Private artwork kept in application storage. Media folders remain read-only.
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
    response::{IntoResponse, Response},
    routing::get,
};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Execute;
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
    let revision: String = query_scalar("SELECT revision FROM work_posters WHERE work_id=?")
        .bind(work)
        .fetch_optional(&store.pool)
        .await
        .map_err(db)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let etag = format!("\"poster-{work}-{revision}-{width}\"");
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag.as_str()),
                (header::CACHE_CONTROL, "private, no-cache"),
            ],
        )
            .into_response());
    }
    let (mime, data) = if width == 0 {
        (
            "image/png".to_owned(),
            query_scalar::<_, Vec<u8>>("SELECT image_data FROM work_posters WHERE work_id=?")
                .bind(work)
                .fetch_one(&store.pool)
                .await
                .map_err(db)?,
        )
    } else {
        let cached:Option<(String,Vec<u8>)>=query_as("SELECT mime,image_data FROM poster_previews WHERE work_id=? AND width=? AND revision=?").bind(work).bind(i64::from(width)).bind(&revision).fetch_optional(&store.pool).await.map_err(db)?;
        if let Some(cached) = cached {
            cached
        } else {
            let permit =
                tokio::time::timeout(std::time::Duration::from_secs(8), DECODERS.acquire())
                    .await
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            let original: Vec<u8> =
                query_scalar("SELECT image_data FROM work_posters WHERE work_id=?")
                    .bind(work)
                    .fetch_optional(&store.pool)
                    .await
                    .map_err(db)?
                    .ok_or(StatusCode::NOT_FOUND)?;
            let encoded = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                normalize(Bytes::from(original), "image/png")
            })
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
            let mut target = None;
            for (size, mime, data) in encoded.variants {
                if size == 0 {
                    continue;
                }
                if size == i64::from(width) {
                    target = Some((mime.to_owned(), data.clone()));
                }
                query("INSERT INTO poster_previews(work_id,width,revision,mime,image_data) SELECT work_id,?,?,?,? FROM work_posters WHERE work_id=? AND revision=? ON CONFLICT(work_id,width) DO UPDATE SET revision=excluded.revision,mime=excluded.mime,image_data=excluded.image_data").bind(size).bind(&revision).bind(mime).bind(data).bind(work).bind(&revision).execute(&store.pool).await.map_err(db)?;
            }
            target.ok_or(StatusCode::NOT_FOUND)?
        }
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime.as_str()),
            (header::ETAG, etag.as_str()),
            (header::CACHE_CONTROL, "private, no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        data,
    )
        .into_response())
}
static DECODERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
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
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/works/{work}/images", get(list))
        .route(
            "/api/v1/works/{work}/images/order",
            axum::routing::put(order),
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
        "SELECT e.id,e.work_id,e.kind,COALESCE(o.position,e.position) AS position,e.width,e.height,'/api/v1/works/'||e.work_id||'/images/'||e.id AS url,'manual' AS origin FROM work_image_entries e LEFT JOIN work_image_order o ON o.work_id=e.work_id AND o.image_id=e.id WHERE e.work_id IN (",
        "SELECT i.id,i.work_id,i.role AS kind,COALESCE(o.position,10000+i.priority) AS position,CAST(NULL AS BIGINT) AS width,CAST(NULL AS BIGINT) AS height,'/api/v1/assets/'||i.id||'/image?v='||i.modified_ns||'-'||i.size AS url,'local' AS origin FROM inventory_images i JOIN libraries l ON l.id=i.library_id JOIN media_nodes n ON n.id=l.node_id LEFT JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id LEFT JOIN work_image_order o ON o.work_id=i.work_id AND o.image_id=i.id WHERE i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND n.status='connected' AND h.image_id IS NULL AND i.role IN ('poster','thumb','logo','illustration','backdrop','banner','disc','landscape') AND i.work_id IN (",
        "SELECT 'legacy-poster' AS id,p.work_id,'poster' AS kind,5000 AS position,CAST(NULL AS BIGINT) AS width,CAST(NULL AS BIGINT) AS height,'/api/v1/works/'||p.work_id||'/poster' AS url,'legacy' AS origin FROM work_posters p WHERE p.work_id IN (",
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
    result.sort_by(|a, b| {
        (&a.work_id, &a.kind, a.position, &a.id).cmp(&(&b.work_id, &b.kind, b.position, &b.id))
    });
    Ok(result)
}
async fn list(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    allowed(&auth, &headers, &work, false).await?;
    let mut seen = std::collections::HashSet::new();
    let images: Vec<_> = entries(&auth.store, &[work])
        .await
        .map_err(db)?
        .into_iter()
        .filter(|e| e.kind == "backdrop" || seen.insert(e.kind.clone()))
        .collect();
    Ok(Json(json!({"images":images})))
}
#[derive(Default, Deserialize)]
pub(crate) struct ImageQuery {
    width: Option<u32>,
    replace: Option<String>,
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
    let etag = format!("\"{id}-{width}\"");
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
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag.as_str()),
                (header::CACHE_CONTROL, "private, no-cache"),
            ],
        )
            .into_response());
    }
    let (mime,data):(String,Vec<u8>)=query_as("SELECT d.mime,d.image_data FROM work_image_data d JOIN work_image_entries e ON e.id=d.image_id WHERE e.work_id=? AND e.id=? AND d.width=?").bind(work).bind(id).bind(i64::from(width)).fetch_optional(&auth.store.pool).await.map_err(db)?.ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        [
            (header::CONTENT_TYPE, mime.as_str()),
            (header::CACHE_CONTROL, "private, no-cache"),
            (header::ETAG, etag.as_str()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        data,
    )
        .into_response())
}
struct Encoded {
    width: i64,
    height: i64,
    variants: Vec<(i64, &'static str, Vec<u8>)>,
}
fn normalize(bytes: Bytes, content: &str) -> Result<Encoded, StatusCode> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let expected = match reader.format() {
        Some(ImageFormat::Png) => "image/png",
        Some(ImageFormat::Jpeg) => "image/jpeg",
        Some(ImageFormat::WebP) => "image/webp",
        _ => return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
    };
    if content != expected {
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
    let mut variants = vec![];
    for width in [0, 320, 640, 1280] {
        let resized = if width == 0 {
            None
        } else {
            Some(image.thumbnail(width, 4096))
        };
        let image = resized.as_ref().unwrap_or(&image);
        let mut bytes = vec![];
        let mime = if image.color().has_alpha() {
            image
                .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
                .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            "image/png"
        } else {
            image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut bytes,
                if width == 0 { 90 } else { 78 },
            )
            .encode_image(image)
            .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            "image/jpeg"
        };
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        variants.push((i64::from(width), mime, bytes));
    }
    Ok(Encoded {
        width: i64::from(image.width()),
        height: i64::from(image.height()),
        variants,
    })
}
pub(crate) async fn upload(
    State(auth): State<AuthState>,
    Path((work, kind)): Path<(String, String)>,
    Query(options): Query<ImageQuery>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    allowed(&auth, &headers, &work, true).await?;
    if !ROLES.contains(&kind.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let existing = entries(&auth.store, std::slice::from_ref(&work))
        .await
        .map_err(db)?;
    if let Some(replace) = &options.replace
        && !existing.iter().any(|e| &e.id == replace && e.kind == kind)
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let content = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let permit = tokio::time::timeout(std::time::Duration::from_secs(8), DECODERS.acquire())
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let encoded = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        normalize(bytes, &content)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    let mut connection = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut connection)
        .await
        .map_err(db)?;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM work_image_entries WHERE work_id=?")
        .bind(&work)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(replace) = &options.replace {
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
    if kind != "backdrop" {
        query("DELETE FROM work_image_entries WHERE work_id=? AND kind=?")
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
    if remaining >= 16 {
        return Err(StatusCode::CONFLICT);
    }
    let id = uuid::Uuid::new_v4().to_string();
    query(
        "INSERT INTO work_image_entries(id,work_id,kind,position,width,height) VALUES(?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&work)
    .bind(kind)
    .bind(count)
    .bind(encoded.width)
    .bind(encoded.height)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    for (width, mime, data) in encoded.variants {
        query("INSERT INTO work_image_data(image_id,width,mime,image_data) VALUES(?,?,?,?)")
            .bind(&id)
            .bind(width)
            .bind(mime)
            .bind(data)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
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
