//! Administrator-configured, address-pinned connection broker. Never follows redirects.
use crate::applications::{Error, err};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub base_url: String,
    pub api_key: String,
    pub allowed_ips: Vec<IpAddr>,
}

fn allowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_link_local()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240
                || ip == Ipv4Addr::new(100, 100, 100, 200))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return allowed_ip(IpAddr::V4(v4));
            }
            !(ip.is_unspecified()
                || ip.is_multicast()
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip
                    == "fd00:ec2::254"
                        .parse::<Ipv6Addr>()
                        .unwrap_or(Ipv6Addr::UNSPECIFIED))
        }
    }
}

fn base(value: &str) -> Result<url::Url, Error> {
    let mut url = url::Url::parse(value)
        .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_connection_url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
        || value.len() > 2048
        || url.path().contains('%')
        || url.path().contains("//")
    {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_connection_url",
        ));
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

async fn addresses(url: &url::Url) -> Result<Vec<IpAddr>, Error> {
    let host = url
        .host_str()
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_connection_url"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url
        .port_or_known_default()
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_connection_url"))?;
    let resolved = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_dns_timeout"))?
    .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_dns_failed"))?;
    let mut ips: Vec<_> = resolved.map(|a| a.ip()).collect();
    ips.sort();
    ips.dedup();
    if ips.is_empty() || ips.iter().any(|ip| !allowed_ip(*ip)) {
        return Err(err(StatusCode::FORBIDDEN, "connection_address_forbidden"));
    }
    Ok(ips)
}

/// Resolve once on explicit administrator configuration; later requests cannot change addresses.
pub async fn configure(base_url: &str, api_key: &str) -> Result<Connection, Error> {
    if api_key.len() > 4096 || api_key.chars().any(char::is_control) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_connection_key",
        ));
    }
    let url = base(base_url)?;
    let allowed_ips = addresses(&url).await?;
    Ok(Connection {
        base_url: url.to_string(),
        api_key: api_key.into(),
        allowed_ips,
    })
}

/// Path is a trusted, relative API endpoint chosen by a Core adapter, never client input.
pub async fn request(
    connection: &Connection,
    method: &str,
    path: &str,
    query: &[(&str, String)],
    body: Option<Value>,
) -> Result<Value, Error> {
    if path.starts_with('/') || path.contains("..") || path.contains(['?', '#', '%', '\\', ':']) {
        return Err(err(StatusCode::FORBIDDEN, "connection_path_forbidden"));
    }
    let url = base(&connection.base_url)?;
    let ips = addresses(&url).await?;
    if ips.iter().any(|ip| !connection.allowed_ips.contains(ip)) {
        return Err(err(StatusCode::FORBIDDEN, "connection_address_changed"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_connection_url"))?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = url
        .port_or_known_default()
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_connection_url"))?;
    let sockets: Vec<_> = ips
        .iter()
        .map(|ip| std::net::SocketAddr::new(*ip, port))
        .collect();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &sockets)
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;
    let mut target = url
        .join(path)
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;
    target
        .query_pairs_mut()
        .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
    let mut request = match method {
        "GET" => client.get(target),
        "POST" => client.post(target),
        "PUT" => client.put(target),
        _ => return Err(err(StatusCode::FORBIDDEN, "connection_method_forbidden")),
    };
    request = request.header(
        "User-Agent",
        "Media-Requests/0.1 (self-hosted media manager)",
    );
    if !connection.api_key.is_empty() {
        request = request.header("X-Api-Key", &connection.api_key);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;
    if !response.status().is_success() {
        let code = match response.status().as_u16() {
            401 | 403 => "connection_auth_failed",
            404 => "remote_content_not_found",
            429 => "connection_rate_limited",
            _ => "remote_request_failed",
        };
        return Err(err(StatusCode::BAD_GATEWAY, code));
    }
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?
    {
        if data.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(err(StatusCode::BAD_GATEWAY, "remote_response_too_large"));
        }
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&data)
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "invalid_remote_response"))
}

/// Artwork hosts the interface may show for titles that are not in the library yet.
/// The host is fixed here and the path is validated by the caller: no URL is client input.
const IMAGE_HOSTS: [&str; 2] = ["image.tmdb.org", "artworks.thetvdb.com"];
static IMAGE_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

/// Unlike an administrator-configured connection, a public host must never resolve to a
/// private, loopback or link-local address (DNS rebinding towards the local network).
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let first = ip.segments()[0];
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// One small poster from a fixed artwork host: HTTPS only, resolved address pinned and public,
/// no redirect, bounded size, and only image types the browser may render.
pub async fn public_image(host: &str, path: &str) -> Result<(Vec<u8>, &'static str), Error> {
    if !IMAGE_HOSTS.contains(&host)
        || !path.starts_with('/')
        || path.contains(['?', '#', '%', '\\'])
    {
        return Err(err(StatusCode::FORBIDDEN, "connection_path_forbidden"));
    }
    let _slot = IMAGE_SLOTS
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "image_busy"))?;
    let resolved =
        tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 443)))
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_dns_timeout"))?
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_dns_failed"))?;
    let sockets: Vec<_> = resolved.collect();
    if sockets.is_empty() || sockets.iter().any(|socket| !public_ip(socket.ip())) {
        return Err(err(StatusCode::FORBIDDEN, "connection_address_forbidden"));
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &sockets)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;
    let mut response = client
        .get(format!("https://{host}{path}"))
        .header(
            "User-Agent",
            "Media-Requests/0.1 (self-hosted media manager)",
        )
        .send()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;
    if !response.status().is_success() {
        return Err(err(StatusCode::NOT_FOUND, "image_not_found"));
    }
    let content_type = match response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or("").trim())
    {
        Some("image/jpeg") => "image/jpeg",
        Some("image/png") => "image/png",
        Some("image/webp") => "image/webp",
        _ => return Err(err(StatusCode::BAD_GATEWAY, "invalid_remote_response")),
    };
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?
    {
        if data.len() + chunk.len() > 1_500_000 {
            return Err(err(StatusCode::BAD_GATEWAY, "remote_response_too_large"));
        }
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() {
        return Err(err(StatusCode::BAD_GATEWAY, "invalid_remote_response"));
    }
    Ok((data, content_type))
}

/// Direct search on TheMovieDB API (for films and series) with pinned public IP address.
pub async fn public_tmdb_search(kind: &str, q: &str, api_key: &str) -> Result<Vec<Value>, Error> {
    if !matches!(kind, "film" | "series") {
        return Ok(vec![]);
    }
    let host = "api.themoviedb.org";
    let endpoint = if kind == "film" {
        "/3/search/movie"
    } else {
        "/3/search/tv"
    };

    let resolved =
        tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 443)))
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_dns_timeout"))?
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_dns_failed"))?;
    let sockets: Vec<_> = resolved.collect();
    if sockets.is_empty() || sockets.iter().any(|socket| !public_ip(socket.ip())) {
        return Err(err(StatusCode::FORBIDDEN, "connection_address_forbidden"));
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &sockets)
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;

    let is_bearer = api_key.starts_with("ey") || api_key.len() > 40;
    let trimmed_key = api_key.trim();
    if trimmed_key.is_empty() || trimmed_key.chars().any(|c| !c.is_ascii_graphic()) {
        return Err(err(StatusCode::UNAUTHORIZED, "connection_auth_failed"));
    }
    let query_clean: String = q.chars().filter(|c| !c.is_control()).take(200).collect();
    let query_trimmed = query_clean.trim();
    if query_trimmed.is_empty() {
        return Ok(vec![]);
    }
    let mut url = reqwest::Url::parse(&format!("https://{host}{endpoint}"))
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("include_adult", "false")
            .append_pair("language", "fr-FR")
            .append_pair("query", query_trimmed);
        if !is_bearer {
            pairs.append_pair("api_key", trimmed_key);
        }
    }

    let mut request = client
        .get(url)
        .header(
            "User-Agent",
            "Media-Requests/0.1 (self-hosted media manager)",
        )
        .header("Accept", "application/json");
    if is_bearer {
        request = request.header("Authorization", format!("Bearer {trimmed_key}"));
    }

    let mut response = request
        .send()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?;

    if !response.status().is_success() {
        let code = match response.status().as_u16() {
            401 | 403 => "connection_auth_failed",
            404 => "remote_content_not_found",
            429 => "connection_rate_limited",
            _ => "remote_request_failed",
        };
        return Err(err(StatusCode::BAD_GATEWAY, code));
    }

    let mut body_bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "connection_failed"))?
    {
        if body_bytes.len() + chunk.len() > 2_000_000 {
            return Err(err(StatusCode::BAD_GATEWAY, "remote_response_too_large"));
        }
        body_bytes.extend_from_slice(&chunk);
    }

    let data: Value = serde_json::from_slice(&body_bytes)
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "invalid_remote_response"))?;

    let Some(results) = data["results"].as_array() else {
        return Ok(vec![]);
    };

    let mut items = Vec::new();
    for v in results {
        let Some(id) = v["id"]
            .as_i64()
            .filter(|id| *id > 0)
            .map(|id| id.to_string())
        else {
            continue;
        };
        let raw_title = if kind == "series" {
            v["name"].as_str().or_else(|| v["original_name"].as_str())
        } else {
            v["title"].as_str().or_else(|| v["original_title"].as_str())
        };
        let Some(raw_title) = raw_title.filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        let title: String = raw_title
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        if title.trim().is_empty() {
            continue;
        }
        let year = if kind == "series" {
            v["first_air_date"]
                .as_str()
                .and_then(|d| d.get(..4)?.parse::<i64>().ok())
        } else {
            v["release_date"]
                .as_str()
                .and_then(|d| d.get(..4)?.parse::<i64>().ok())
        }
        .filter(|y| (1800..=3000).contains(y));

        let summary = v["overview"].as_str().map(|s| {
            s.chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .take(600)
                .collect::<String>()
        });

        let poster = v["poster_path"].as_str().and_then(|path| {
            if super::remote::valid_poster_path(path) {
                Some(json!({"src": "tmdb", "path": path}))
            } else {
                None
            }
        });

        items.push(json!({
            "kind": kind,
            "title": title,
            "year": year,
            "externalId": id,
            "externalProvider": "tmdb",
            "summary": summary,
            "poster": poster,
        }));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_hosts_never_resolve_to_the_local_network() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.4",
            "172.16.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.4",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "104.18.20.1", "2606:4700::1"] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
    #[tokio::test]
    async fn only_fixed_artwork_hosts_and_plain_paths_are_fetched() {
        for (host, path) in [
            ("example.com", "/a.jpg"),
            ("127.0.0.1", "/a.jpg"),
            ("image.tmdb.org", "a.jpg"),
            ("image.tmdb.org", "/a.jpg?x=1"),
            ("image.tmdb.org", "/%2e%2e/a.jpg"),
        ] {
            assert!(public_image(host, path).await.is_err(), "{host}{path}");
        }
    }
    #[test]
    fn connection_policy_keeps_admin_lan_but_denies_metadata() {
        assert!(allowed_ip("127.0.0.1".parse().unwrap()));
        assert!(allowed_ip("192.168.1.4".parse().unwrap()));
        for ip in [
            "169.254.169.254",
            "0.0.0.0",
            "224.0.0.1",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
        ] {
            assert!(!allowed_ip(ip.parse().unwrap()));
        }
        for url in [
            "ftp://host/path",
            "http://user:key@host",
            "http://host/?q=x",
            "http://host/#x",
        ] {
            assert!(base(url).is_err());
        }
    }
    #[tokio::test]
    async fn broker_pins_destination_and_never_follows_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let n = stream.read(&mut bytes).await.unwrap();
            let req = String::from_utf8_lossy(&bytes[..n]);
            assert!(req.contains("/arr/api/v3/system/status"));
            assert!(req.to_lowercase().contains("x-api-key: test-only"));
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\n\r\n").await.unwrap();
        });
        let conn = configure(&format!("http://{address}/arr"), "test-only")
            .await
            .unwrap();
        assert!(
            request(&conn, "GET", "api/v3/system/status", &[], None)
                .await
                .is_err()
        );
        task.await.unwrap();
        let mut altered = conn;
        altered.allowed_ips.clear();
        assert!(
            request(&altered, "GET", "api/v3/system/status", &[], None)
                .await
                .is_err()
        );
    }
}
