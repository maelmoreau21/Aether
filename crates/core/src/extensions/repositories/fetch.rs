use super::*;

pub(super) fn checked_url(raw: &str, maximum: usize) -> Result<url::Url, Error> {
    let url =
        url::Url::parse(raw).map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_repository_url"))?;
    if raw.len() > maximum
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(url.host(),Some(url::Host::Domain(host)) if !host.eq_ignore_ascii_case("localhost") && !host.ends_with(".localhost"))
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "repository_https_public_host_required",
        ));
    }
    Ok(url)
}

pub(super) fn safe_url(raw: &str) -> Result<url::Url, Error> {
    checked_url(raw, 2048)
}

/// Accept the public GitHub pages administrators copy from their browser, while
/// storing the actual JSON endpoint. Package and redirect URL rules are unchanged.
pub(super) fn catalogue_url(raw: &str) -> Result<url::Url, Error> {
    let url = safe_url(raw)?;
    if url.host_str() != Some("github.com") || url.query().is_some() {
        return Ok(url);
    }
    let segments: Vec<_> = url.path().trim_matches('/').split('/').collect();
    let (owner, repository, branch, path) = match segments.as_slice() {
        [owner, repository] => (
            *owner,
            *repository,
            "main",
            "repository/manifest.json".into(),
        ),
        [owner, repository, "tree", branch] => (
            *owner,
            *repository,
            *branch,
            "repository/manifest.json".into(),
        ),
        [owner, repository, "blob", branch, path @ ..] if !path.is_empty() => {
            (*owner, *repository, *branch, path.join("/"))
        }
        _ => return Ok(url),
    };
    safe_url(&format!(
        "https://raw.githubusercontent.com/{owner}/{repository}/{branch}/{path}"
    ))
}

/// GitHub serves release assets through a redirect to its content hosts. Any
/// other cross-host redirect is refused; every hop is validated again.
pub(super) fn redirect_allowed(from: &url::Url, to: &url::Url) -> bool {
    let (Some(from), Some(to)) = (from.host_str(), to.host_str()) else {
        return false;
    };
    from.eq_ignore_ascii_case(to)
        || ((from.eq_ignore_ascii_case("github.com")
            || from.eq_ignore_ascii_case("api.github.com"))
            && [
                "objects.githubusercontent.com",
                "release-assets.githubusercontent.com",
                "codeload.github.com",
            ]
            .iter()
            .any(|host| to.eq_ignore_ascii_case(host)))
}

pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, _, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_broadcast()
                && !v.is_documentation()
                && !v.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (b == 18 || b == 19))
        }
        // Permit only global unicast; reject mapped IPv4, ULA, local and documentation ranges.
        IpAddr::V6(v) => {
            let s = v.segments();
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] <= 0x1ff || s[1] == 0xdb8))
        }
    }
}

#[cfg(test)]
pub(super) fn test_document(raw: &str, maximum: usize) -> Option<Result<Vec<u8>, Error>> {
    let bytes = test_documents().lock().unwrap().get(raw).cloned()?;
    Some(if bytes.len() <= maximum {
        Ok(bytes)
    } else {
        Err(err(
            StatusCode::BAD_GATEWAY,
            "repository_response_too_large",
        ))
    })
}

pub(super) fn user_agent() -> String {
    format!(
        "Aether-Extensions/{}",
        media_contracts::version::current_version()
    )
}

/// Bounded HTTPS GET of a public document (catalogue, GitHub API, collection files).
pub(crate) async fn fetch(raw: &str, maximum: usize) -> Result<Vec<u8>, Error> {
    safe_url(raw)?;
    #[cfg(test)]
    if let Some(result) = test_document(raw, maximum) {
        return result;
    }
    let _permit = DOWNLOADS
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "repository_busy"))?;
    download(raw, maximum, "application/json", Duration::from_secs(10)).await
}

pub(super) async fn fetch_asset(raw: &str, maximum: usize) -> Result<Vec<u8>, Error> {
    safe_url(raw)?;
    #[cfg(test)]
    if let Some(result) = test_document(raw, maximum) {
        return result;
    }
    let _permit = DOWNLOADS
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "repository_busy"))?;
    let Some((owner, repository, tag, name)) = github_release_asset(raw) else {
        return download(raw, maximum, "*/*", Duration::from_secs(60)).await;
    };
    // github.com (web pages and releases/download) can hang or answer 504 on some
    // networks while api.github.com and the asset CDN stay reachable. Both routes end
    // on the same CDN object and the checksum and signature still decide trust, so the
    // API goes first (one bounded attempt), then the direct link, staying well inside
    // standard 60 s reverse-proxy timeouts and the 70 s admin request budget.
    let deadline = Instant::now() + Duration::from_secs(40);
    let left = || deadline.saturating_duration_since(Instant::now());
    let api_budget = left().min(Duration::from_secs(15));
    match github_api_asset(&owner, &repository, &tag, &name, maximum, api_budget).await {
        Ok(bytes) => return Ok(bytes),
        Err(failure) => eprintln!(
            "GitHub API release download failed ({owner}/{repository} {tag}/{name}): {}",
            error_code(&failure)
        ),
    }
    let timeout = left().max(Duration::from_secs(10));
    download(raw, maximum, "*/*", timeout)
        .await
        .inspect_err(|failure| {
            eprintln!("Release download failed ({raw}): {}", error_code(failure));
        })
}

pub(super) fn error_code(failure: &Error) -> &str {
    failure.1.0["error"].as_str().unwrap_or("unknown")
}

/// `https://github.com/{owner}/{repo}/releases/download/{tag}/{asset}`.
pub(super) fn github_release_asset(raw: &str) -> Option<(String, String, String, String)> {
    let url = url::Url::parse(raw).ok()?;
    if !url.host_str()?.eq_ignore_ascii_case("github.com") || url.query().is_some() {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.collect();
    let [owner, repository, "releases", "download", tag, name] = segments.as_slice() else {
        return None;
    };
    let plain = |s: &str| {
        !s.is_empty()
            && s.len() <= 200
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
    };
    [owner, repository, tag, name]
        .iter()
        .all(|s| plain(s))
        .then(|| {
            (
                owner.to_string(),
                repository.to_string(),
                tag.to_string(),
                name.to_string(),
            )
        })
}

pub(super) async fn github_api_asset(
    owner: &str,
    repository: &str,
    tag: &str,
    name: &str,
    maximum: usize,
    budget: Duration,
) -> Result<Vec<u8>, Error> {
    let started = Instant::now();
    let release = download(
        &format!("https://api.github.com/repos/{owner}/{repository}/releases/tags/{tag}"),
        MAX_MANIFEST,
        "application/vnd.github+json",
        budget.min(Duration::from_secs(10)),
    )
    .await?;
    let release: Value = serde_json::from_slice(&release)
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_http_error"))?;
    let asset = release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == name))
        .and_then(|asset| asset["url"].as_str())
        .filter(|url| {
            url.starts_with(&format!(
                "https://api.github.com/repos/{owner}/{repository}/releases/assets/"
            ))
        })
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "repository_http_error"))?
        .to_owned();
    let left = budget
        .saturating_sub(started.elapsed())
        .max(Duration::from_secs(5));
    download(&asset, maximum, "application/octet-stream", left).await
}

pub(super) async fn download(
    raw: &str,
    maximum: usize,
    accept: &str,
    timeout: Duration,
) -> Result<Vec<u8>, Error> {
    let mut url = safe_url(raw)?;
    for hop in 0..=MAX_REDIRECTS {
        let host = url
            .host_str()
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_repository_url"))?
            .to_owned();
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::lookup_host((host.as_str(), 443)),
        )
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
        .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses.iter().any(|address| !public_ip(address.ip()))
        {
            return Err(err(StatusCode::BAD_REQUEST, "repository_address_forbidden"));
        }
        // Redirects are followed by hand: each hop is pinned to its validated addresses.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(3))
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?;
        let mut response = client
            .get(url.clone())
            .header("accept", accept)
            .header("user-agent", user_agent())
            .send()
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?;
        if response.status().is_redirection() {
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|location| location.to_str().ok())
                .and_then(|location| url.join(location).ok())
                .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "repository_redirect_forbidden"))?;
            // Signed asset URLs carry long query strings; the host policy is unchanged.
            let next = checked_url(next.as_str(), 8192)
                .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_redirect_forbidden"))?;
            if hop == MAX_REDIRECTS || !redirect_allowed(&url, &next) {
                return Err(err(
                    StatusCode::BAD_GATEWAY,
                    "repository_redirect_forbidden",
                ));
            }
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(err(StatusCode::BAD_GATEWAY, "repository_http_error"));
        }
        if response
            .content_length()
            .is_some_and(|size| size > maximum as u64)
        {
            return Err(err(
                StatusCode::BAD_GATEWAY,
                "repository_response_too_large",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
        {
            if bytes.len().saturating_add(chunk.len()) > maximum {
                return Err(err(
                    StatusCode::BAD_GATEWAY,
                    "repository_response_too_large",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err(err(
        StatusCode::BAD_GATEWAY,
        "repository_redirect_forbidden",
    ))
}
