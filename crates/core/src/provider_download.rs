//! Bounded downloads from hosts approved by a signed extension manifest.
use axum::{Json, http::StatusCode};
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};

type Error = (StatusCode, Json<Value>);
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;

#[cfg(test)]
pub(crate) fn test_documents()
-> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>> {
    static DOCUMENTS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    > = std::sync::OnceLock::new();
    DOCUMENTS.get_or_init(Default::default)
}

fn error(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error": code})))
}

fn checked_url(allowed_hosts: &[String], raw_url: &str) -> Result<reqwest::Url, Error> {
    if raw_url.len() > 4096 {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "provider_download_invalid_url",
        ));
    }
    let url = reqwest::Url::parse(raw_url)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "provider_download_invalid_url"))?;
    let host = url
        .host_str()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "provider_download_invalid_url"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || !allowed_hosts.iter().any(|allowed| allowed == host)
    {
        return Err(error(
            StatusCode::FORBIDDEN,
            "provider_download_host_denied",
        ));
    }
    Ok(url)
}

fn check_addresses(addresses: &[SocketAddr]) -> Result<(), Error> {
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses
            .iter()
            .any(|address| !crate::extensions::repositories::public_ip(address.ip()))
    {
        return Err(error(
            StatusCode::FORBIDDEN,
            "provider_download_host_denied",
        ));
    }
    Ok(())
}

pub(crate) async fn download(
    allowed_hosts: &[String],
    raw_url: &str,
    max_bytes: usize,
) -> Result<(Vec<u8>, String), Error> {
    if !(1..=MAX_DOWNLOAD_BYTES).contains(&max_bytes) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "provider_download_invalid_limit",
        ));
    }
    let url = checked_url(allowed_hosts, raw_url)?;
    #[cfg(test)]
    if let Some(bytes) = test_documents().lock().unwrap().get(raw_url).cloned() {
        return if bytes.len() <= max_bytes {
            Ok((bytes, "text/plain".into()))
        } else {
            Err(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "provider_download_too_large",
            ))
        };
    }
    let host = url
        .host_str()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "provider_download_invalid_url"))?
        .to_owned();
    tokio::time::timeout(Duration::from_secs(12), async {
        let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
            .await
            .map_err(|_| error(StatusCode::BAD_GATEWAY, "provider_download_dns_failed"))?
            .collect();
        check_addresses(&addresses)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| error(StatusCode::BAD_GATEWAY, "provider_download_failed"))?;
        let mut response = client
            .get(url)
            .header(reqwest::header::USER_AGENT, "Media-Subtitle-Client/1.0")
            .send()
            .await
            .map_err(|_| error(StatusCode::BAD_GATEWAY, "provider_download_failed"))?;
        if !response.status().is_success() {
            return Err(error(StatusCode::BAD_GATEWAY, "provider_download_failed"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "provider_download_too_large",
            ));
        }
        let content = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| error(StatusCode::BAD_GATEWAY, "provider_download_failed"))?
        {
            if chunk.len() > max_bytes.saturating_sub(bytes.len()) {
                return Err(error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "provider_download_too_large",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((bytes, content))
    })
    .await
    .map_err(|_| error(StatusCode::REQUEST_TIMEOUT, "provider_download_timeout"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_require_exact_approved_https_hosts_without_credentials_or_fragments() {
        let allowed = vec!["downloads.example.org".to_owned()];
        assert!(checked_url(&allowed, "https://downloads.example.org/a.srt?token=opaque").is_ok());
        for url in [
            "http://downloads.example.org/a.srt",
            "https://downloads.example.org:444/a.srt",
            "https://user@downloads.example.org/a.srt",
            "https://user:secret@downloads.example.org/a.srt",
            "https://downloads.example.org/a.srt#fragment",
            "https://sub.downloads.example.org/a.srt",
            "https://downloads.example.org.evil.example/a.srt",
            "https://127.0.0.1/a.srt",
        ] {
            assert!(checked_url(&allowed, url).is_err(), "{url}");
        }
        assert!(checked_url(&[], "https://downloads.example.org/a.srt").is_err());
        assert!(checked_url(&allowed, &"a".repeat(4097)).is_err());
    }

    #[test]
    fn dns_answers_must_all_be_public_and_bounded() {
        let public: SocketAddr = "93.184.216.34:443".parse().unwrap();
        assert!(check_addresses(&[public]).is_ok());
        assert!(check_addresses(&[]).is_err());
        assert!(check_addresses(&[public; 33]).is_err());
        for address in [
            "127.0.0.1:443",
            "10.0.0.1:443",
            "169.254.169.254:443",
            "100.64.0.1:443",
            "198.18.0.1:443",
            "[2002:7f00:1::]:443",
            "[::1]:443",
            "[fc00::1]:443",
            "[::ffff:127.0.0.1]:443",
        ] {
            let private = address.parse().unwrap();
            assert!(check_addresses(&[private]).is_err(), "{address}");
            assert!(check_addresses(&[public, private]).is_err(), "{address}");
        }
    }

    #[tokio::test]
    async fn invalid_limits_and_local_dns_are_rejected_without_http() {
        let allowed = vec!["localhost".to_owned()];
        for limit in [0, MAX_DOWNLOAD_BYTES + 1] {
            assert_eq!(
                download(&allowed, "https://localhost/a.srt", limit)
                    .await
                    .unwrap_err()
                    .0,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            download(&allowed, "https://localhost/a.srt", 1024)
                .await
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
}
