//! DLNA/UPnP MediaServer for old TVs and consoles, which can neither sign in
//! nor play HLS: they browse a ContentDirectory and read original files.
//!
//! Opt-in: `MEDIA_DLNA_API_KEY` names a `catalog:read` API key. What the key
//! may read (its libraries, its owner's permissions, parental policy and
//! access schedule) is exactly what the network sees; revoking the key turns
//! DLNA off at the next request. URLs carry a secret derived from the key, and
//! only private, loopback and link-local addresses are answered.
//!
//! Tree: root → libraries → films (items) | series → seasons → episodes.
pub mod ssdp;

use crate::{database::query_as, identity::IdentityStore};
use axum::{
    Router,
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use sha2::{Digest, Sha256};
use std::{fmt::Write, net::SocketAddr, sync::Arc};

const PAGE_MAX: i64 = 500;

#[derive(Clone)]
pub struct DlnaConfig {
    token: String,
    /// Path segment and device UUID, both derived from the key.
    pub secret: String,
    pub uuid: String,
    pub name: String,
    pub port: u16,
}

impl DlnaConfig {
    pub fn new(token: String, name: String, port: u16) -> Self {
        let digest = Sha256::digest(format!("aether-dlna:{token}").as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            &hex[32..40],
            &hex[40..44],
            &hex[44..48],
            &hex[48..52],
            &hex[52..64]
        );
        Self {
            token,
            secret: hex[..32].to_owned(),
            uuid,
            name,
            port,
        }
    }

    /// `MEDIA_DLNA_API_KEY` (required) and `MEDIA_DLNA_NAME` (default "Aether").
    pub fn from_env(port: u16) -> Option<Self> {
        let token = std::env::var("MEDIA_DLNA_API_KEY").ok()?.trim().to_owned();
        if token.is_empty() {
            return None;
        }
        let name = std::env::var("MEDIA_DLNA_NAME")
            .ok()
            .map(|n| n.trim().chars().take(64).collect::<String>())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Aether".to_owned());
        Some(Self::new(token, name, port))
    }

    pub fn description_path(&self) -> String {
        format!("/dlna/{}/device.xml", self.secret)
    }
}

#[derive(Clone)]
struct DlnaState {
    store: IdentityStore,
    config: Arc<DlnaConfig>,
}

pub fn router(store: IdentityStore, config: DlnaConfig) -> Router {
    Router::new()
        .route("/dlna/{secret}/device.xml", get(device))
        .route("/dlna/{secret}/cds.xml", get(cds_scpd))
        .route("/dlna/{secret}/cms.xml", get(cms_scpd))
        .route("/dlna/{secret}/control/cds", post(cds_control))
        .route("/dlna/{secret}/control/cms", post(cms_control))
        .route("/dlna/{secret}/media/{asset}", get(media).head(media))
        .with_state(DlnaState {
            store,
            config: Arc::new(config),
        })
}

/// Escapes text for XML content and attributes.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

fn lan(request: &Request) -> bool {
    match request.extensions().get::<ConnectInfo<SocketAddr>>() {
        // In-process calls (tests) carry no peer address.
        None => true,
        Some(ConnectInfo(address)) => match address.ip().to_canonical() {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
            }
        },
    }
}

/// Secret matches (constant time) and the peer is on the local network.
fn admitted(state: &DlnaState, secret: &str, request: &Request) -> bool {
    let expected = state.config.secret.as_bytes();
    let given = secret.as_bytes();
    given.len() == expected.len()
        && given
            .iter()
            .zip(expected)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
        && lan(request)
}

fn xml_response(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/xml; charset=\"utf-8\""),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

async fn device(
    State(state): State<DlnaState>,
    Path(secret): Path<String>,
    request: Request,
) -> Response {
    if !admitted(&state, &secret, &request) {
        return not_found();
    }
    let c = &state.config;
    let base = format!("/dlna/{}", c.secret);
    xml_response(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:dlna="urn:schemas-dlna-org:device-1-0">
<specVersion><major>1</major><minor>0</minor></specVersion>
<device>
<deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
<dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC>
<friendlyName>{name}</friendlyName>
<manufacturer>Aether</manufacturer>
<modelName>Aether Media Server</modelName>
<modelNumber>{version}</modelNumber>
<UDN>uuid:{uuid}</UDN>
<serviceList>
<service><serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType><serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId><SCPDURL>{base}/cds.xml</SCPDURL><controlURL>{base}/control/cds</controlURL><eventSubURL>{base}/events/cds</eventSubURL></service>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId><SCPDURL>{base}/cms.xml</SCPDURL><controlURL>{base}/control/cms</controlURL><eventSubURL>{base}/events/cms</eventSubURL></service>
</serviceList>
</device>
</root>"#,
        name = xml(&c.name),
        version = xml(media_contracts::version::current_version()),
        uuid = c.uuid,
    ))
}

fn scpd(actions: &str, variables: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0"><specVersion><major>1</major><minor>0</minor></specVersion>
<actionList>{actions}</actionList><serviceStateTable>{variables}</serviceStateTable></scpd>"#
    )
}

fn action(name: &str, arguments: &[(&str, &str, &str)]) -> String {
    let mut out = format!("<action><name>{name}</name><argumentList>");
    for (argument, direction, variable) in arguments {
        let _ = write!(
            out,
            "<argument><name>{argument}</name><direction>{direction}</direction><relatedStateVariable>{variable}</relatedStateVariable></argument>"
        );
    }
    out.push_str("</argumentList></action>");
    out
}

fn variable(name: &str, kind: &str, events: bool) -> String {
    let events = if events { "yes" } else { "no" };
    format!(
        "<stateVariable sendEvents=\"{events}\"><name>{name}</name><dataType>{kind}</dataType></stateVariable>"
    )
}

async fn cds_scpd(
    State(state): State<DlnaState>,
    Path(secret): Path<String>,
    request: Request,
) -> Response {
    if !admitted(&state, &secret, &request) {
        return not_found();
    }
    let actions = [
        action(
            "Browse",
            &[
                ("ObjectID", "in", "A_ARG_TYPE_ObjectID"),
                ("BrowseFlag", "in", "A_ARG_TYPE_BrowseFlag"),
                ("Filter", "in", "A_ARG_TYPE_Filter"),
                ("StartingIndex", "in", "A_ARG_TYPE_Index"),
                ("RequestedCount", "in", "A_ARG_TYPE_Count"),
                ("SortCriteria", "in", "A_ARG_TYPE_SortCriteria"),
                ("Result", "out", "A_ARG_TYPE_Result"),
                ("NumberReturned", "out", "A_ARG_TYPE_Count"),
                ("TotalMatches", "out", "A_ARG_TYPE_Count"),
                ("UpdateID", "out", "A_ARG_TYPE_UpdateID"),
            ],
        ),
        action(
            "GetSearchCapabilities",
            &[("SearchCaps", "out", "SearchCapabilities")],
        ),
        action(
            "GetSortCapabilities",
            &[("SortCaps", "out", "SortCapabilities")],
        ),
        action("GetSystemUpdateID", &[("Id", "out", "SystemUpdateID")]),
    ]
    .concat();
    let variables = [
        variable("A_ARG_TYPE_ObjectID", "string", false),
        "<stateVariable sendEvents=\"no\"><name>A_ARG_TYPE_BrowseFlag</name><dataType>string</dataType><allowedValueList><allowedValue>BrowseMetadata</allowedValue><allowedValue>BrowseDirectChildren</allowedValue></allowedValueList></stateVariable>".to_owned(),
        variable("A_ARG_TYPE_Filter", "string", false),
        variable("A_ARG_TYPE_Index", "ui4", false),
        variable("A_ARG_TYPE_Count", "ui4", false),
        variable("A_ARG_TYPE_SortCriteria", "string", false),
        variable("A_ARG_TYPE_Result", "string", false),
        variable("A_ARG_TYPE_UpdateID", "ui4", false),
        variable("SearchCapabilities", "string", false),
        variable("SortCapabilities", "string", false),
        variable("SystemUpdateID", "ui4", true),
    ]
    .concat();
    xml_response(scpd(&actions, &variables))
}

async fn cms_scpd(
    State(state): State<DlnaState>,
    Path(secret): Path<String>,
    request: Request,
) -> Response {
    if !admitted(&state, &secret, &request) {
        return not_found();
    }
    let actions = [
        action(
            "GetProtocolInfo",
            &[
                ("Source", "out", "SourceProtocolInfo"),
                ("Sink", "out", "SinkProtocolInfo"),
            ],
        ),
        action(
            "GetCurrentConnectionIDs",
            &[("ConnectionIDs", "out", "CurrentConnectionIDs")],
        ),
    ]
    .concat();
    let variables = [
        variable("SourceProtocolInfo", "string", true),
        variable("SinkProtocolInfo", "string", true),
        variable("CurrentConnectionIDs", "string", true),
    ]
    .concat();
    xml_response(scpd(&actions, &variables))
}

/// Text of the first `<name>` element (namespace prefix ignored), unescaped.
/// SOAP arguments are flat text: no XML library needed.
fn argument(body: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(found) = body[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        if !body[from..].starts_with('>') {
            continue;
        }
        let open = body[..at].rfind('<')?;
        let prefix = &body[open + 1..at];
        if !(prefix.is_empty()
            || prefix
                .strip_suffix(':')
                .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric())))
        {
            continue;
        }
        let start = from + 1;
        let end = start + body[start..].find('<')?;
        let text = body[start..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&");
        return Some(text.trim().to_owned());
    }
    None
}

fn soap(service: &str, action: &str, values: &[(&str, String)]) -> Response {
    let mut body = String::new();
    for (name, value) in values {
        let _ = write!(body, "<{name}>{}</{name}>", xml(value));
    }
    xml_response(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:{service}:1">{body}</u:{action}Response></s:Body></s:Envelope>"#
    ))
}

fn soap_fault(code: u16, description: &str) -> Response {
    let mut response = xml_response(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#
    ));
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response
}

fn soap_action(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("soapaction")?.to_str().ok()?;
    Some(value.trim_matches('"').rsplit('#').next()?.to_owned())
}

async fn body_text(request: Request) -> Option<String> {
    let bytes = axum::body::to_bytes(request.into_body(), 64 * 1024)
        .await
        .ok()?;
    String::from_utf8(bytes.to_vec()).ok()
}

async fn cms_control(
    State(state): State<DlnaState>,
    Path(secret): Path<String>,
    request: Request,
) -> Response {
    if !admitted(&state, &secret, &request) {
        return not_found();
    }
    match soap_action(request.headers()).as_deref() {
        Some("GetProtocolInfo") => soap(
            "ConnectionManager",
            "GetProtocolInfo",
            &[
                (
                    "Source",
                    "http-get:*:video/x-matroska:*,http-get:*:video/mp4:*,http-get:*:video/webm:*,http-get:*:video/x-msvideo:*,http-get:*:video/mpeg:*,http-get:*:video/mp2t:*".to_owned(),
                ),
                ("Sink", String::new()),
            ],
        ),
        Some("GetCurrentConnectionIDs") => soap(
            "ConnectionManager",
            "GetCurrentConnectionIDs",
            &[("ConnectionIDs", "0".to_owned())],
        ),
        _ => soap_fault(401, "Invalid Action"),
    }
}

/// What the key may read, for one request.
struct Viewer {
    key: String,
    user: String,
    role: String,
}

async fn viewer(store: &IdentityStore, token: &str) -> Result<Viewer, StatusCode> {
    let mut tx = store
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (key, user, role) = crate::api_keys::resolve(&mut tx, token).await?;
    let _ = tx.commit().await;
    Ok(Viewer { key, user, role })
}

/// Libraries `l` (with node `n`) the key may read; binds: key, role, user.
const LIBRARY_SQL: &str = "l.enabled=1 AND n.status!='revoked' AND EXISTS(SELECT 1 FROM api_key_libraries kl WHERE kl.library_id=l.id AND kl.key_id=?) AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?))";

/// One playable copy of work `w`.
const ASSET_SQL: &str = "(SELECT a.id FROM inventory_editions e JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root ORDER BY a.size DESC, a.id LIMIT 1)";

#[derive(Debug, PartialEq)]
enum Object {
    Root,
    Library(String),
    Series(String),
    Season(String),
    Work(String),
}

impl Object {
    fn parse(id: &str) -> Option<Self> {
        if id == "0" {
            return Some(Self::Root);
        }
        let (kind, value) = id.split_once(':')?;
        if value.is_empty() || value.len() > 128 {
            return None;
        }
        let value = value.to_owned();
        Some(match kind {
            "l" => Self::Library(value),
            "s" => Self::Series(value),
            "n" => Self::Season(value),
            "w" => Self::Work(value),
            _ => return None,
        })
    }
}

/// A DIDL-Lite entry.
enum Entry {
    Container {
        id: String,
        parent: String,
        title: String,
        children: i64,
    },
    Item {
        id: String,
        parent: String,
        title: String,
        asset: String,
        mime: String,
        size: i64,
        probe: String,
    },
}

fn duration(probe: &serde_json::Value) -> Option<String> {
    let seconds: f64 = probe
        .get("format")?
        .get("duration")?
        .as_str()?
        .parse()
        .ok()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let millis = (seconds * 1000.0).round() as u64;
    Some(format!(
        "{}:{:02}:{:02}.{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    ))
}

fn resolution(probe: &serde_json::Value) -> Option<String> {
    let video = probe
        .get("streams")?
        .as_array()?
        .iter()
        .find(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("video"))?;
    Some(format!(
        "{}x{}",
        video.get("width")?.as_i64()?,
        video.get("height")?.as_i64()?
    ))
}

fn didl(entries: &[Entry], base: &str) -> String {
    let mut out = String::from(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">"#,
    );
    for entry in entries {
        match entry {
            Entry::Container {
                id,
                parent,
                title,
                children,
            } => {
                let _ = write!(
                    out,
                    r#"<container id="{}" parentID="{}" restricted="1" searchable="0" childCount="{children}"><dc:title>{}</dc:title><upnp:class>object.container.storageFolder</upnp:class></container>"#,
                    xml(id),
                    xml(parent),
                    xml(title)
                );
            }
            Entry::Item {
                id,
                parent,
                title,
                asset,
                mime,
                size,
                probe,
            } => {
                let probe: serde_json::Value = serde_json::from_str(probe).unwrap_or_default();
                let mut attributes = format!(" size=\"{size}\"");
                if let Some(duration) = duration(&probe) {
                    let _ = write!(attributes, " duration=\"{duration}\"");
                }
                if let Some(resolution) = resolution(&probe) {
                    let _ = write!(attributes, " resolution=\"{resolution}\"");
                }
                // Streaming, byte seek (OP=01), no conversion.
                let protocol = format!(
                    "http-get:*:{}:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
                    xml(mime)
                );
                let _ = write!(
                    out,
                    r#"<item id="{}" parentID="{}" restricted="1"><dc:title>{}</dc:title><upnp:class>object.item.videoItem</upnp:class><res protocolInfo="{protocol}"{attributes}>{base}/media/{}</res></item>"#,
                    xml(id),
                    xml(parent),
                    xml(title),
                    xml(asset)
                );
            }
        }
    }
    out.push_str("</DIDL-Lite>");
    out
}

type Rows<T> = Result<Vec<T>, sqlx::Error>;
/// id, title, kind, asset, mime, size, probe.
type WorkRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
);

impl DlnaState {
    async fn libraries(&self, viewer: &Viewer) -> Rows<(String, String, String, i64)> {
        let sql = format!(
            "SELECT l.id,l.name,l.kind,(SELECT COUNT(*) FROM inventory_works w WHERE w.library_id=l.id AND w.has_available_asset=1 AND w.kind IN ('film','series')) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.kind IN ('films','series') AND {LIBRARY_SQL} ORDER BY l.name,l.id"
        );
        query_as(sqlx::AssertSqlSafe(sql))
            .bind(&viewer.key)
            .bind(&viewer.role)
            .bind(&viewer.user)
            .fetch_all(&self.store.pool)
            .await
    }

    /// Works of a library (films, or series), or episodes of a season.
    async fn works(
        &self,
        viewer: &Viewer,
        filter: &str,
        value: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<WorkRow>, i64), sqlx::Error> {
        let from = format!(
            "FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id {filter} AND {LIBRARY_SQL} AND {}",
            crate::product::policy_sql()
        );
        let order = if filter.contains("inventory_episodes") {
            "ep.episode_number,w.id"
        } else {
            "w.title_sort,w.id"
        };
        let sql = format!(
            "SELECT w.id,w.title,w.kind,{ASSET_SQL} AS asset,(SELECT a.mime FROM inventory_assets a WHERE a.id={ASSET_SQL}),(SELECT a.size FROM inventory_assets a WHERE a.id={ASSET_SQL}),(SELECT a.probe_json FROM inventory_assets a WHERE a.id={ASSET_SQL}) {from} ORDER BY {order} LIMIT ? OFFSET ?"
        );
        let rows = query_as(sqlx::AssertSqlSafe(sql))
            .bind(value)
            .bind(&viewer.key)
            .bind(&viewer.role)
            .bind(&viewer.user)
            .bind(&viewer.user)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.store.pool)
            .await?;
        let total: (i64,) = query_as(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) {from}")))
            .bind(value)
            .bind(&viewer.key)
            .bind(&viewer.role)
            .bind(&viewer.user)
            .bind(&viewer.user)
            .fetch_one(&self.store.pool)
            .await?;
        Ok((rows, total.0))
    }

    /// Seasons of a series the key may read: (id, number, title, episodes).
    async fn seasons(&self, viewer: &Viewer, series: &str) -> Rows<(String, i64, String, i64)> {
        let sql = format!(
            "SELECT se.id,se.season_number,se.title,(SELECT COUNT(*) FROM inventory_episodes ep WHERE ep.season_id=se.id) FROM inventory_seasons se JOIN inventory_works w ON w.id=se.series_id JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE se.series_id=? AND {LIBRARY_SQL} AND {} ORDER BY se.season_number",
            crate::product::policy_sql()
        );
        query_as(sqlx::AssertSqlSafe(sql))
            .bind(series)
            .bind(&viewer.key)
            .bind(&viewer.role)
            .bind(&viewer.user)
            .bind(&viewer.user)
            .fetch_all(&self.store.pool)
            .await
    }

    /// Parent of a season (its series) or of a work (season, series or library).
    async fn parent(&self, object: &Object) -> Option<String> {
        match object {
            Object::Root => Some("-1".to_owned()),
            Object::Library(_) => Some("0".to_owned()),
            Object::Season(id) => {
                query_as::<_, (String,)>("SELECT series_id FROM inventory_seasons WHERE id=?")
                    .bind(id)
                    .fetch_optional(&self.store.pool)
                    .await
                    .ok()?
                    .map(|(s,)| format!("s:{s}"))
            }
            Object::Series(id) | Object::Work(id) => {
                if let Ok(Some((season,))) = query_as::<_, (String,)>(
                    "SELECT season_id FROM inventory_episodes WHERE work_id=?",
                )
                .bind(id)
                .fetch_optional(&self.store.pool)
                .await
                {
                    return Some(format!("n:{season}"));
                }
                query_as::<_, (String,)>("SELECT library_id FROM inventory_works WHERE id=?")
                    .bind(id)
                    .fetch_optional(&self.store.pool)
                    .await
                    .ok()?
                    .map(|(l,)| format!("l:{l}"))
            }
        }
    }

    /// Children of `object`, paged, with the total count.
    async fn children(
        &self,
        viewer: &Viewer,
        object: &Object,
        offset: i64,
        limit: i64,
    ) -> Result<Option<(Vec<Entry>, i64)>, sqlx::Error> {
        let parent = match object {
            Object::Root => "0".to_owned(),
            Object::Library(id) => format!("l:{id}"),
            Object::Series(id) => format!("s:{id}"),
            Object::Season(id) => format!("n:{id}"),
            Object::Work(_) => return Ok(Some((Vec::new(), 0))),
        };
        Ok(Some(match object {
            Object::Root => {
                let all = self.libraries(viewer).await?;
                let total = all.len() as i64;
                let page = all
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .map(|(id, name, _, count)| Entry::Container {
                        id: format!("l:{id}"),
                        parent: parent.clone(),
                        title: name,
                        children: count,
                    })
                    .collect();
                (page, total)
            }
            Object::Library(id) => {
                let (rows, total) = self
                    .works(
                        viewer,
                        "WHERE w.library_id=? AND w.has_available_asset=1 AND w.kind IN ('film','series')",
                        id,
                        offset,
                        limit,
                    )
                    .await?;
                (self.entries(rows, &parent), total)
            }
            Object::Series(id) => {
                let all = self.seasons(viewer, id).await?;
                let total = all.len() as i64;
                let page = all
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .map(|(season, number, title, count)| Entry::Container {
                        id: format!("n:{season}"),
                        parent: parent.clone(),
                        title: if title.is_empty() {
                            format!("Saison {number}")
                        } else {
                            title
                        },
                        children: count,
                    })
                    .collect();
                (page, total)
            }
            Object::Season(id) => {
                let (rows, total) = self
                    .works(
                        viewer,
                        "JOIN inventory_episodes ep ON ep.work_id=w.id WHERE ep.season_id=?",
                        id,
                        offset,
                        limit,
                    )
                    .await?;
                (self.entries(rows, &parent), total)
            }
            Object::Work(_) => unreachable!(),
        }))
    }

    fn entries(&self, rows: Vec<WorkRow>, parent: &str) -> Vec<Entry> {
        rows.into_iter()
            .filter_map(|(id, title, kind, asset, mime, size, probe)| {
                if kind == "series" {
                    return Some(Entry::Container {
                        id: format!("s:{id}"),
                        parent: parent.to_owned(),
                        title,
                        children: 1,
                    });
                }
                Some(Entry::Item {
                    id: format!("w:{id}"),
                    parent: parent.to_owned(),
                    title,
                    asset: asset?,
                    mime: mime?,
                    size: size.unwrap_or(0),
                    probe: probe.unwrap_or_default(),
                })
            })
            .collect()
    }

    /// The entry of `object` itself (BrowseMetadata), if the key may read it.
    async fn metadata(
        &self,
        viewer: &Viewer,
        object: &Object,
    ) -> Result<Option<Entry>, sqlx::Error> {
        let parent = self.parent(object).await.unwrap_or_else(|| "0".to_owned());
        Ok(match object {
            Object::Root => Some(Entry::Container {
                id: "0".to_owned(),
                parent,
                title: self.config.name.clone(),
                children: self.libraries(viewer).await?.len() as i64,
            }),
            Object::Library(id) => self
                .libraries(viewer)
                .await?
                .into_iter()
                .find(|l| &l.0 == id)
                .map(|(id, name, _, count)| Entry::Container {
                    id: format!("l:{id}"),
                    parent,
                    title: name,
                    children: count,
                }),
            Object::Season(id) => {
                let series: Option<(String,)> =
                    query_as("SELECT series_id FROM inventory_seasons WHERE id=?")
                        .bind(id)
                        .fetch_optional(&self.store.pool)
                        .await?;
                match series {
                    Some((series,)) => self
                        .seasons(viewer, &series)
                        .await?
                        .into_iter()
                        .find(|s| &s.0 == id)
                        .map(|(season, number, title, count)| Entry::Container {
                            id: format!("n:{season}"),
                            parent,
                            title: if title.is_empty() {
                                format!("Saison {number}")
                            } else {
                                title
                            },
                            children: count,
                        }),
                    None => None,
                }
            }
            Object::Series(id) | Object::Work(id) => {
                let (rows, _) = self.works(viewer, "WHERE w.id=?", id, 0, 1).await?;
                self.entries(rows, &parent).into_iter().next()
            }
        })
    }
}

async fn cds_control(
    State(state): State<DlnaState>,
    Path(secret): Path<String>,
    request: Request,
) -> Response {
    if !admitted(&state, &secret, &request) {
        return not_found();
    }
    let action = soap_action(request.headers());
    match action.as_deref() {
        Some("GetSearchCapabilities") => {
            return soap(
                "ContentDirectory",
                "GetSearchCapabilities",
                &[("SearchCaps", String::new())],
            );
        }
        Some("GetSortCapabilities") => {
            return soap(
                "ContentDirectory",
                "GetSortCapabilities",
                &[("SortCaps", String::new())],
            );
        }
        Some("GetSystemUpdateID") => {
            return soap(
                "ContentDirectory",
                "GetSystemUpdateID",
                &[("Id", "1".to_owned())],
            );
        }
        Some("Browse") => {}
        _ => return soap_fault(401, "Invalid Action"),
    }
    let origin = origin(request.headers());
    let Some(body) = body_text(request).await else {
        return soap_fault(402, "Invalid Args");
    };
    let viewer = match viewer(&state.store, &state.config.token).await {
        Ok(viewer) => viewer,
        // Revoked key, disabled owner or outside the access schedule: nothing to show.
        Err(_) => return soap_fault(701, "No such object"),
    };
    let Some(object) = argument(&body, "ObjectID")
        .as_deref()
        .and_then(Object::parse)
    else {
        return soap_fault(701, "No such object");
    };
    let offset = argument(&body, "StartingIndex")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        .clamp(0, 1_000_000);
    let limit = match argument(&body, "RequestedCount").and_then(|v| v.parse::<i64>().ok()) {
        Some(count) if count > 0 => count.min(PAGE_MAX),
        _ => PAGE_MAX,
    };
    let base = format!("/dlna/{}", state.config.secret);
    let result = match argument(&body, "BrowseFlag").as_deref() {
        Some("BrowseMetadata") => state
            .metadata(&viewer, &object)
            .await
            .map(|entry| entry.map(|entry| (vec![entry], 1))),
        Some("BrowseDirectChildren") => state.children(&viewer, &object, offset, limit).await,
        _ => return soap_fault(402, "Invalid Args"),
    };
    let (entries, total) = match result {
        Ok(Some(found)) => found,
        Ok(None) => return soap_fault(701, "No such object"),
        Err(_) => return soap_fault(501, "Action Failed"),
    };
    // Players resolve relative <res> URLs badly: the base is absolute.
    let absolute = format!("{origin}{base}");
    soap(
        "ContentDirectory",
        "Browse",
        &[
            ("Result", didl(&entries, &absolute)),
            ("NumberReturned", entries.len().to_string()),
            ("TotalMatches", total.to_string()),
            ("UpdateID", "1".to_owned()),
        ],
    )
}

/// `http://host[:port]` as the player reached us, for absolute `<res>` URLs.
fn origin(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .filter(|h| {
            !h.is_empty()
                && h.len() <= 255
                && h.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
        })
        .map(|h| format!("http://{h}"))
        .unwrap_or_default()
}

/// Media of an item: the original file, after checking again that the key
/// may read it (it may have been revoked since the Browse).
async fn media(
    State(state): State<DlnaState>,
    Path((secret, asset)): Path<(String, String)>,
    request: Request,
) -> Response {
    if !admitted(&state, &secret, &request) || asset.is_empty() || asset.len() > 128 {
        return not_found();
    }
    let Ok(viewer) = viewer(&state.store, &state.config.token).await else {
        return not_found();
    };
    let sql = format!(
        "SELECT COUNT(*) FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_works w ON w.id=e.work_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND {LIBRARY_SQL} AND {}",
        crate::product::policy_sql()
    );
    let allowed: Result<(i64,), _> = query_as(sqlx::AssertSqlSafe(sql))
        .bind(&asset)
        .bind(&viewer.key)
        .bind(&viewer.role)
        .bind(&viewer.user)
        .bind(&viewer.user)
        .fetch_one(&state.store.pool)
        .await;
    if !matches!(allowed, Ok((count,)) if count > 0) {
        return not_found();
    }
    let mut response = crate::inventory::content(&state.store, &asset, request).await;
    let headers = response.headers_mut();
    headers.insert(
        "transferMode.dlna.org",
        header::HeaderValue::from_static("Streaming"),
    );
    headers.insert(
        "contentFeatures.dlna.org",
        header::HeaderValue::from_static(
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        ),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_arguments_and_didl() {
        assert_eq!(Object::parse("0"), Some(Object::Root));
        assert_eq!(Object::parse("s:abc"), Some(Object::Series("abc".into())));
        assert_eq!(Object::parse("x:abc"), None);
        assert_eq!(Object::parse("w:"), None);
        let body = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><ObjectID>l:films&amp;1</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"#;
        assert_eq!(argument(body, "ObjectID").as_deref(), Some("l:films&1"));
        assert_eq!(argument(body, "SortCriteria").as_deref(), Some(""));
        assert_eq!(argument(body, "Missing"), None);
        let probe = r#"{"format":{"duration":"5400.250"},"streams":[{"codec_type":"video","width":1920,"height":1080}]}"#;
        let text = didl(
            &[Entry::Item {
                id: "w:1".into(),
                parent: "l:a".into(),
                title: "Tom & Jerry <3>".into(),
                asset: "asset-1".into(),
                mime: "video/x-matroska".into(),
                size: 42,
                probe: probe.into(),
            }],
            "http://10.0.0.2:3000/dlna/s",
        );
        assert!(
            text.contains("<dc:title>Tom &amp; Jerry &lt;3&gt;</dc:title>"),
            "{text}"
        );
        assert!(
            text.contains(r#"size="42" duration="1:30:00.250" resolution="1920x1080""#),
            "{text}"
        );
        assert!(
            text.contains(">http://10.0.0.2:3000/dlna/s/media/asset-1</res>"),
            "{text}"
        );
        let config = DlnaConfig::new("media_x".into(), "Aether".into(), 3000);
        assert_eq!(config.secret.len(), 32);
        assert_eq!(config.uuid.len(), 36);
        assert_ne!(
            config.secret,
            DlnaConfig::new("media_y".into(), "A".into(), 1).secret
        );
    }
}
