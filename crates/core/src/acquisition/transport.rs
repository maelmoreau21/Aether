//! Administrator-configured, address-pinned connection broker. Never follows redirects.
use crate::applications::{Error, err};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[cfg(test)]
mod tests {
    use super::*;
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
