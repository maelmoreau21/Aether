//! Network adapters: production requires TLS and DNS-pinned public destinations.
use super::{Config, Event};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};
use serde_json::json;
use std::{net::SocketAddr, time::Duration};

type Result<T> = std::result::Result<T, &'static str>;

#[cfg(test)]
pub(super) fn test_destinations() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static DESTINATIONS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    DESTINATIONS.get_or_init(Default::default)
}

fn secret_ok(secret: &Option<String>) -> bool {
    secret
        .as_ref()
        .is_none_or(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
}
fn mailbox(value: &str) -> bool {
    value.len() <= 254
        && value.parse::<lettre::message::Mailbox>().is_ok()
        && !value.chars().any(char::is_control)
}
fn url(config: &Config) -> Result<reqwest::Url> {
    let value =
        reqwest::Url::parse(&config.destination).map_err(|_| "notification_destination_denied")?;
    let local =
        config.local_test() && value.scheme() == "http" && value.host_str() == Some("127.0.0.1");
    if let Some(host) = value.host_str()
        && let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>()
        && !local
        && !crate::extensions::repositories::public_ip(ip)
    {
        return Err("notification_destination_denied");
    }
    if !local
        && let Some(host) = value.host_str()
    {
        let lower = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if lower == "localhost"
            || lower.ends_with(".localhost")
            || lower.ends_with(".local")
            || lower.ends_with(".internal")
            || lower.ends_with(".arpa")
        {
            return Err("notification_destination_denied");
        }
    }
    if config.destination.len() > 2048
        || value.host_str().is_none()
        || (!local && (value.scheme() != "https" || value.port_or_known_default() != Some(443)))
        || !value.username().is_empty()
        || value.password().is_some()
        || value.query().is_some()
        || value.fragment().is_some()
    {
        return Err("notification_destination_denied");
    }
    Ok(value)
}
pub(super) fn validate(kind: &str, config: &Config) -> Result<()> {
    if !secret_ok(&config.secret)
        || config.username.len() > 254
        || config.username.chars().any(char::is_control)
    {
        return Err("notification_invalid_config");
    }
    match kind {
        "ntfy" | "gotify" | "webhook" => {
            let url = url(config)?;
            if (kind == "gotify" || kind == "webhook")
                && config.secret.as_deref().is_none_or(|s| s.is_empty())
            {
                return Err("notification_secret_required");
            }
            if kind == "webhook" && config.secret.as_ref().is_some_and(|s| s.len() < 32) {
                return Err("notification_secret_too_short");
            }
            if kind == "ntfy" && url.path().trim_matches('/').is_empty() {
                return Err("notification_topic_required");
            }
        }
        "email" => {
            if config.smtp_host.is_empty()
                || config.smtp_host.len() > 253
                || !config
                    .smtp_host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
                || (!config.local_test() && config.smtp_port != 465)
                || config.smtp_port == 0
                || !mailbox(&config.sender)
                || !mailbox(&config.recipient)
            {
                return Err("notification_invalid_config");
            }
            if !config.local_test() {
                let lower = config.smtp_host.trim_matches(['[', ']']).to_ascii_lowercase();
                if let Ok(ip) = lower.parse::<std::net::IpAddr>() {
                    if !crate::extensions::repositories::public_ip(ip) {
                        return Err("notification_destination_denied");
                    }
                }
                if lower == "localhost"
                    || lower.ends_with(".localhost")
                    || lower.ends_with(".local")
                    || lower.ends_with(".internal")
                    || lower.ends_with(".arpa")
                {
                    return Err("notification_destination_denied");
                }
            }
            if !config.username.is_empty() && config.secret.as_deref().is_none_or(|s| s.is_empty())
            {
                return Err("notification_secret_required");
            }
        }
        _ => return Err("notification_adapter_unknown"),
    }
    Ok(())
}
async fn addresses(host: &str, port: u16, local: bool) -> Result<Vec<SocketAddr>> {
    let addresses: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "notification_dns_failed")?
        .collect();
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses.iter().any(|a| {
            if local {
                a.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            } else {
                !crate::extensions::repositories::public_ip(a.ip())
            }
        })
    {
        return Err("notification_destination_denied");
    }
    Ok(addresses)
}
fn message(event: &Event) -> (&'static str, String) {
    let title = match event.kind.as_str() {
        "scan.completed" => "Scan termine",
        "task.failed" => "Tache en echec",
        "content.added" => "Nouveau contenu disponible",
        kind if kind.starts_with("request.") => "Demande mise a jour",
        kind if kind.starts_with("report.") => "Signalement mis a jour",
        _ => "Tache terminee",
    };
    (
        title,
        format!(
            "{title}. Evenement : {}. {} : {}.",
            event.id,
            if event.kind.starts_with("request.") {
                "Demande"
            } else if event.kind.starts_with("report.") {
                "Signalement"
            } else if event.kind == "content.added" {
                "Oeuvre"
            } else {
                "Tache"
            },
            event.task_id
        ),
    )
}
#[cfg(test)]
pub(super) async fn deliver(kind: &str, config: &Config, event: &Event) -> Result<()> {
    deliver_inner(kind, config, event, None).await
}
pub(super) async fn deliver_checked(
    kind: &str,
    config: &Config,
    event: &Event,
    store: &crate::identity::IdentityStore,
    channel: &super::Channel,
) -> Result<()> {
    deliver_inner(kind, config, event, Some((store, channel))).await
}
async fn allowed(
    context: Option<(&crate::identity::IdentityStore, &super::Channel)>,
    event: &Event,
) -> Result<()> {
    if let Some((store, channel)) = context
        && !super::still_allowed(store, channel, event)
            .await
            .map_err(|_| "notification_authorization_error")?
    {
        return Err("notification_revoked");
    }
    Ok(())
}
async fn deliver_inner(
    kind: &str,
    config: &Config,
    event: &Event,
    context: Option<(&crate::identity::IdentityStore, &super::Channel)>,
) -> Result<()> {
    validate(kind, config)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let (title, body) = message(event);
        if kind == "email" {
            let addresses =
                addresses(&config.smtp_host, config.smtp_port, config.local_test()).await?;
            let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
                addresses[0].ip().to_string(),
            )
            .port(config.smtp_port)
            .timeout(Some(Duration::from_secs(8)));
            if !config.local_test() {
                let tls = TlsParameters::new(config.smtp_host.clone())
                    .map_err(|_| "notification_invalid_config")?;
                builder = builder.tls(Tls::Wrapper(tls));
            }
            if !config.username.is_empty() {
                builder = builder.credentials(Credentials::new(
                    config.username.clone(),
                    config.secret.clone().unwrap_or_default(),
                ));
            }
            let mail = Message::builder()
                .from(
                    config
                        .sender
                        .parse()
                        .map_err(|_| "notification_invalid_config")?,
                )
                .to(config
                    .recipient
                    .parse()
                    .map_err(|_| "notification_invalid_config")?)
                .subject(title)
                .message_id(Some(format!("<{}@notifications.local>", event.id)))
                .body(body)
                .map_err(|_| "notification_invalid_config")?;
            allowed(context, event).await?;
            builder
                .build()
                .send(mail)
                .await
                .map_err(|_| "notification_send_failed")?;
            return Ok(());
        }
        let mut url = url(config)?;
        if kind == "gotify" {
            let path = format!("{}/message", url.path().trim_end_matches('/'));
            url.set_path(&path);
        }
        let host = url
            .host_str()
            .ok_or("notification_destination_denied")?
            .to_string();
        let addresses = addresses(
            &host,
            url.port_or_known_default()
                .ok_or("notification_destination_denied")?,
            config.local_test(),
        )
        .await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "notification_send_failed")?;
        let mut request = client
            .post(url)
            .header("User-Agent", "Media-Notifications/1.0")
            .header("X-Media-Event-Id", &event.id)
            .header("Idempotency-Key", &event.id);
        match kind {
            "ntfy" => {
                request = request.header("Title", title).body(body);
                if let Some(secret) = config.secret.as_ref().filter(|s| !s.is_empty()) {
                    request = request.bearer_auth(secret);
                }
            }
            "gotify" => {
                request = request
                    .header("X-Gotify-Key", config.secret.as_deref().unwrap_or_default())
                    .json(&json!({"title":title,"message":body,"priority":5}));
            }
            "webhook" => {
                let bytes = serde_json::to_vec(event).map_err(|_| "notification_invalid_config")?;
                let timestamp = super::now().to_string();
                let signature = signature(
                    config.secret.as_deref().unwrap_or_default(),
                    &timestamp,
                    &event.id,
                    &bytes,
                );
                request = request
                    .header("Content-Type", "application/json")
                    .header("X-Media-Timestamp", timestamp)
                    .header("X-Media-Signature", format!("sha256={signature}"))
                    .body(bytes);
            }
            _ => return Err("notification_adapter_unknown"),
        }
        allowed(context, event).await?;
        let response = request
            .send()
            .await
            .map_err(|_| "notification_send_failed")?;
        match response.status().as_u16() {
            200..=299 => Ok(()),
            401 | 403 => Err("notification_auth_denied"),
            300..=399 => Err("notification_destination_denied"),
            429 => Err("notification_rate_limited"),
            _ => Err("notification_send_failed"),
        }
    })
    .await
    .map_err(|_| "notification_timeout")?
}
pub(super) fn signature(secret: &str, timestamp: &str, id: &str, body: &[u8]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let mut message = format!("{timestamp}.{id}.").into_bytes();
    message.extend_from_slice(body);
    ring::hmac::sign(&key, &message)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
