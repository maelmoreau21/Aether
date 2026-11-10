//! SSDP (UDP 239.255.255.250:1900): answers `M-SEARCH` from players on the
//! local network and announces the server every 10 minutes, so TVs list it
//! without configuration. Only private and link-local senders are answered.
use super::DlnaConfig;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};
use tokio::net::UdpSocket;

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const PORT: u16 = 1900;
const MAX_AGE: u32 = 1800;

/// Notification types this server answers for, the device UUID aside.
const TYPES: [&str; 4] = [
    "upnp:rootdevice",
    "urn:schemas-upnp-org:device:MediaServer:1",
    "urn:schemas-upnp-org:service:ContentDirectory:1",
    "urn:schemas-upnp-org:service:ConnectionManager:1",
];

/// The `ST` values a search target matches: `None` when it is not ours.
pub(crate) fn matches(target: &str, uuid: &str) -> Option<Vec<String>> {
    let device = format!("uuid:{uuid}");
    match target {
        "ssdp:all" => Some(
            std::iter::once(device)
                .chain(TYPES.iter().map(|t| (*t).to_owned()))
                .collect(),
        ),
        t if t == device || TYPES.contains(&t) => Some(vec![t.to_owned()]),
        _ => None,
    }
}

fn usn(target: &str, uuid: &str) -> String {
    if target.starts_with("uuid:") {
        target.to_owned()
    } else {
        format!("uuid:{uuid}::{target}")
    }
}

/// `ST` of an `M-SEARCH * HTTP/1.1` with `MAN: "ssdp:discover"`, else `None`.
pub(crate) fn search_target(datagram: &str) -> Option<String> {
    let mut lines = datagram.split("\r\n");
    if !lines.next()?.eq_ignore_ascii_case("M-SEARCH * HTTP/1.1") {
        return None;
    }
    let mut target = None;
    let mut discover = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_uppercase().as_str() {
            "ST" => target = Some(value.to_owned()),
            "MAN" => discover = value.trim_matches('"') == "ssdp:discover",
            _ => {}
        }
    }
    target.filter(|_| discover)
}

fn server_header() -> String {
    format!(
        "{}/1.0 UPnP/1.0 Aether/{}",
        std::env::consts::OS,
        media_contracts::version::current_version()
    )
}

pub(crate) fn response(config: &DlnaConfig, local: Ipv4Addr, target: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age={MAX_AGE}\r\nEXT:\r\nLOCATION: http://{local}:{}{}\r\nSERVER: {}\r\nST: {target}\r\nUSN: {}\r\nContent-Length: 0\r\n\r\n",
        config.port,
        config.description_path(),
        server_header(),
        usn(target, &config.uuid)
    )
}

fn notify(config: &DlnaConfig, local: Ipv4Addr, target: &str, alive: bool) -> String {
    let kind = if alive { "ssdp:alive" } else { "ssdp:byebye" };
    format!(
        "NOTIFY * HTTP/1.1\r\nHOST: {GROUP}:{PORT}\r\nCACHE-CONTROL: max-age={MAX_AGE}\r\nLOCATION: http://{local}:{}{}\r\nNT: {target}\r\nNTS: {kind}\r\nSERVER: {}\r\nUSN: {}\r\n\r\n",
        config.port,
        config.description_path(),
        server_header(),
        usn(target, &config.uuid)
    )
}

/// The local address that reaches `peer` (no packet is sent).
fn local_address(peer: SocketAddr) -> Option<Ipv4Addr> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect(peer).ok()?;
    match probe.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

fn bind() -> std::io::Result<UdpSocket> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket.set_reuse_address(true)?;
    #[cfg(all(unix, not(target_os = "solaris"), not(target_os = "illumos")))]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT).into())?;
    socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_multicast_ttl_v4(2)?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

/// Runs until the process stops. Errors are logged once; DLNA HTTP keeps working.
pub fn spawn(config: DlnaConfig) {
    let config = Arc::new(config);
    tokio::spawn(async move {
        let socket = match bind() {
            Ok(socket) => Arc::new(socket),
            Err(error) => {
                eprintln!(
                    "{{\"event\":\"dlna_ssdp_unavailable\",\"error\":{:?}}}",
                    error.to_string()
                );
                return;
            }
        };
        eprintln!(
            "{{\"event\":\"dlna_ssdp_started\",\"name\":{:?}}}",
            config.name
        );
        let _announcer = {
            let socket = socket.clone();
            let config = config.clone();
            tokio::spawn(async move {
                let group = SocketAddr::V4(SocketAddrV4::new(GROUP, PORT));
                loop {
                    if let Some(local) = local_address(group) {
                        let device = format!("uuid:{}", config.uuid);
                        for target in std::iter::once(device.as_str()).chain(TYPES) {
                            let _ = socket
                                .send_to(notify(&config, local, target, true).as_bytes(), group)
                                .await;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(600)).await;
                }
            })
        };
        let mut buffer = [0u8; 2048];
        loop {
            let Ok((length, peer)) = socket.recv_from(&mut buffer).await else {
                continue;
            };
            let private = match peer.ip().to_canonical() {
                std::net::IpAddr::V4(v4) => {
                    v4.is_private() || v4.is_link_local() || v4.is_loopback()
                }
                std::net::IpAddr::V6(_) => false,
            };
            if !private {
                continue;
            }
            let Some(target) = std::str::from_utf8(&buffer[..length])
                .ok()
                .and_then(search_target)
            else {
                continue;
            };
            let Some(targets) = matches(&target, &config.uuid) else {
                continue;
            };
            let Some(local) = local_address(peer) else {
                continue;
            };
            for target in targets {
                let _ = socket
                    .send_to(response(&config, local, &target).as_bytes(), peer)
                    .await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_and_answers() {
        let config = DlnaConfig::new("media_x".into(), "Salon".into(), 3000);
        let search = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n";
        let target = search_target(search).unwrap();
        assert_eq!(
            matches(&target, &config.uuid).unwrap(),
            std::slice::from_ref(&target)
        );
        assert_eq!(matches("ssdp:all", &config.uuid).unwrap().len(), 5);
        assert_eq!(
            matches("urn:schemas-upnp-org:device:MediaRenderer:1", &config.uuid),
            None
        );
        assert_eq!(
            search_target("M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n"),
            None,
            "MAN is required"
        );
        assert_eq!(
            search_target("NOTIFY * HTTP/1.1\r\nMAN: \"ssdp:discover\"\r\nST: ssdp:all\r\n\r\n"),
            None
        );
        let answer = response(&config, Ipv4Addr::new(192, 168, 1, 10), &target);
        assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(answer.contains(&format!(
            "LOCATION: http://192.168.1.10:3000/dlna/{}/device.xml\r\n",
            config.secret
        )));
        assert!(answer.contains(&format!(
            "USN: uuid:{}::urn:schemas-upnp-org:device:MediaServer:1\r\n",
            config.uuid
        )));
    }
}
