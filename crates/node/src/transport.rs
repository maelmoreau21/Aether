//! One outbound transport boundary. No redirects; DNS results are checked before use.
use std::{io, net::IpAddr, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_native_tls::{TlsConnector, native_tls};

pub trait Connection: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Connection for T {}

pub fn validate_url(raw: &str) -> io::Result<url::Url> {
    let u = url::Url::parse(raw).map_err(io::Error::other)?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid internal origin",
        ));
    }
    Ok(u)
}

fn permitted(ip: IpAddr, host: &str, port: u16, tls: bool) -> bool {
    let ip = match ip {
        IpAddr::V6(v) => v.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    };
    // Loopback is an explicitly local trust boundary. All other internal destinations
    // require an exact operator-configured host:port, including HTTPS private services.
    if ip.is_loopback() {
        return true;
    }
    let blocked = match ip {
        IpAddr::V4(v) => {
            v.is_unspecified() || v.is_multicast() || v.is_link_local() || v.is_broadcast()
        }
        IpAddr::V6(v) => v.is_unspecified() || v.is_multicast() || v.is_unicast_link_local(),
    };
    if blocked {
        return false;
    }
    let allowed = std::env::var("MEDIA_INTERNAL_ALLOWED_AUTHORITIES").unwrap_or_default();
    let exact = allowed
        .split(',')
        .any(|s| s.trim() == format!("{host}:{port}"));
    exact && (tls || std::env::var("MEDIA_INTERNAL_TRUSTED_HTTP").as_deref() == Ok("true"))
}

pub async fn connect(raw: &str) -> io::Result<Box<dyn Connection>> {
    let u = validate_url(raw)?;
    let host = u
        .host_str()
        .ok_or_else(|| io::Error::other("missing host"))?
        .trim_matches(['[', ']']);
    let port = u
        .port_or_known_default()
        .ok_or_else(|| io::Error::other("missing port"))?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let addresses: Vec<_> = tokio::net::lookup_host((host, port)).await?.collect();
        if addresses.is_empty()
            || addresses
                .iter()
                .any(|a| !permitted(a.ip(), host, port, u.scheme() == "https"))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "internal destination denied",
            ));
        }
        // Connect to the checked address, without a second DNS resolution.
        let tcp = TcpStream::connect(addresses.as_slice()).await?;
        if u.scheme() == "http" {
            return Ok(Box::new(tcp) as Box<dyn Connection>);
        }
        let mut builder = native_tls::TlsConnector::builder();
        builder.min_protocol_version(Some(native_tls::Protocol::Tlsv12));
        if let Ok(path) = std::env::var("MEDIA_INTERNAL_CA_FILE") {
            let pem = tokio::fs::read(path).await?;
            builder.add_root_certificate(
                native_tls::Certificate::from_pem(&pem).map_err(io::Error::other)?,
            );
        }
        let connector = TlsConnector::from(builder.build().map_err(io::Error::other)?);
        Ok(Box::new(
            connector
                .connect(host, tcp)
                .await
                .map_err(io::Error::other)?,
        ) as Box<dyn Connection>)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "internal connection timeout"))?
}

/// Bind representation metadata to the authenticated relay response. TLS protects stream bytes.
pub fn stream_metadata_digest(headers: &http::HeaderMap) -> String {
    let mut canonical = String::from("stream-transport-integrity\n");
    for name in [
        "content-type",
        "content-length",
        "content-range",
        "accept-ranges",
        "cache-control",
    ] {
        canonical.push_str(name);
        canonical.push(':');
        canonical.push_str(
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
        );
        canonical.push('\n');
    }
    media_contracts::protocol::sha256_hex(canonical.as_bytes())
}

/// Bound control response headers independently of the streamed media body.
pub async fn send_request(
    sender: &mut hyper::client::conn::http1::SendRequest<http_body_util::Full<bytes::Bytes>>,
    request: http::Request<http_body_util::Full<bytes::Bytes>>,
) -> io::Result<http::Response<hyper::body::Incoming>> {
    tokio::time::timeout(Duration::from_secs(15), sender.send_request(request))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "internal response timeout"))?
        .map_err(io::Error::other)
}

pub async fn json_body(body: hyper::body::Incoming) -> io::Result<bytes::Bytes> {
    use http_body_util::BodyExt;
    tokio::time::timeout(
        Duration::from_secs(15),
        http_body_util::Limited::new(body, 8 * 1024 * 1024).collect(),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "internal body timeout"))?
    .map(|body| body.to_bytes())
    .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_reject_credentials_paths_queries_and_non_http_schemes() {
        for url in [
            "file:///etc/passwd",
            "http://user:password@localhost",
            "https://localhost/path",
            "https://localhost/?token=secret",
            "https://localhost/#fragment",
        ] {
            assert!(validate_url(url).is_err());
        }
        for url in [
            "https://localhost:443",
            "http://127.0.0.1:3101",
            "https://[::1]:443",
        ] {
            assert!(validate_url(url).is_ok());
        }
    }
    #[tokio::test]
    async fn metadata_and_multicast_destinations_are_denied_before_connect() {
        for url in [
            "http://169.254.169.254",
            "https://169.254.169.254",
            "http://224.0.0.1",
            "http://[::ffff:169.254.169.254]",
        ] {
            assert_eq!(
                connect(url).await.err().unwrap().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }
}
