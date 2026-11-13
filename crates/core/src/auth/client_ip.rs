//! Real client address for throttles, resolved once per request from the TCP peer.
//!
//! `X-Forwarded-For` and `X-Real-IP` are only believed when the TCP peer is a trusted
//! proxy (`MEDIA_TRUSTED_PROXIES`, loopback and private ranges by default). The
//! forwarded chain is then read from the right, skipping trusted hops, so a value the
//! client prepended itself is never used. The resolved address replaces both headers,
//! which keeps `extract_client_key` the single place handlers read it from.
use axum::{
    extract::{ConnectInfo, Request},
    http::{HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    fn parse(raw: &str) -> Option<Self> {
        let (address, prefix) = match raw.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix.parse::<u8>().ok()?)),
            None => (raw, None),
        };
        let network = address.trim().parse::<IpAddr>().ok()?.to_canonical();
        let max = if network.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(max);
        (prefix <= max).then_some(Self { network, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, ip.to_canonical()) {
            (IpAddr::V4(network), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

const DEFAULT_TRUSTED: [&str; 7] = [
    "127.0.0.0/8",
    "::1/128",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "fc00::/7",
    "fe80::/10",
];

/// `MEDIA_TRUSTED_PROXIES`: comma-separated addresses or CIDR ranges; `none` trusts no proxy.
pub(crate) fn trusted_from(raw: Option<&str>) -> Vec<Cidr> {
    let Some(raw) = raw.map(str::trim) else {
        return DEFAULT_TRUSTED
            .iter()
            .filter_map(|c| Cidr::parse(c))
            .collect();
    };
    if raw.is_empty() || raw.eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let parsed = Cidr::parse(entry);
            if parsed.is_none() {
                eprintln!("MEDIA_TRUSTED_PROXIES: entrée ignorée « {entry} »");
            }
            parsed
        })
        .collect()
}

static TRUSTED: LazyLock<Vec<Cidr>> =
    LazyLock::new(|| trusted_from(std::env::var("MEDIA_TRUSTED_PROXIES").ok().as_deref()));

fn is_trusted(ip: IpAddr, trusted: &[Cidr]) -> bool {
    trusted.iter().any(|cidr| cidr.contains(ip))
}

pub(crate) fn resolve(peer: IpAddr, headers: &HeaderMap, trusted: &[Cidr]) -> IpAddr {
    let peer = peer.to_canonical();
    if !is_trusted(peer, trusted) {
        return peer;
    }
    let mut hops: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|hop| super::util::parse_ip_or_socket(hop.trim()))
        .collect();
    let mut closest_trusted = None;
    while let Some(hop) = hops.pop() {
        if !is_trusted(hop, trusted) {
            return hop;
        }
        closest_trusted = Some(hop);
    }
    closest_trusted
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| super::util::parse_ip_or_socket(value.trim()))
        })
        .unwrap_or(peer)
}

/// Outermost layer of the served Core. Requests without a TCP peer (in-process tests)
/// are left untouched.
pub async fn resolve_client_ip(mut request: Request, next: Next) -> Response {
    if let Some(ConnectInfo(peer)) = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied()
    {
        let client = resolve(peer.ip(), request.headers(), &TRUSTED);
        let headers = request.headers_mut();
        headers.remove("x-real-ip");
        headers.remove("x-forwarded-for");
        if let Ok(value) = HeaderValue::from_str(&client.to_string()) {
            headers.insert("x-forwarded-for", value);
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().unwrap()
    }

    fn forwarded(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().unwrap());
        headers
    }

    #[test]
    fn untrusted_peer_cannot_choose_its_address() {
        let trusted = trusted_from(None);
        let mut headers = forwarded("198.51.100.7");
        headers.insert("x-real-ip", "198.51.100.8".parse().unwrap());
        assert_eq!(
            resolve(ip("203.0.113.9"), &headers, &trusted),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn trusted_proxy_chain_is_read_from_the_right() {
        let trusted = trusted_from(None);
        // The client prepended a forged hop; the proxy appended the real one.
        let headers = forwarded("198.51.100.7, 203.0.113.9, 10.0.0.2");
        assert_eq!(
            resolve(ip("127.0.0.1"), &headers, &trusted),
            ip("203.0.113.9")
        );
        assert_eq!(
            resolve(ip("::ffff:10.0.0.3"), &headers, &trusted),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn local_clients_behind_proxy_and_real_ip_fallback() {
        let trusted = trusted_from(None);
        assert_eq!(
            resolve(ip("172.18.0.2"), &forwarded("192.168.1.5"), &trusted),
            ip("192.168.1.5")
        );
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "203.0.113.9".parse().unwrap());
        assert_eq!(
            resolve(ip("127.0.0.1"), &headers, &trusted),
            ip("203.0.113.9")
        );
        assert_eq!(
            resolve(ip("127.0.0.1"), &HeaderMap::new(), &trusted),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn configured_proxies_replace_defaults() {
        let none = trusted_from(Some("none"));
        assert_eq!(
            resolve(ip("127.0.0.1"), &forwarded("203.0.113.9"), &none),
            ip("127.0.0.1")
        );
        let custom = trusted_from(Some("203.0.113.0/24, nope, 2001:db8::1"));
        assert_eq!(custom.len(), 2);
        assert_eq!(
            resolve(ip("203.0.113.4"), &forwarded("198.51.100.7"), &custom),
            ip("198.51.100.7")
        );
        assert_eq!(
            resolve(ip("10.0.0.2"), &forwarded("198.51.100.7"), &custom),
            ip("10.0.0.2")
        );
        assert_eq!(
            resolve(ip("2001:db8::1"), &forwarded("198.51.100.7"), &custom),
            ip("198.51.100.7")
        );
    }
}
