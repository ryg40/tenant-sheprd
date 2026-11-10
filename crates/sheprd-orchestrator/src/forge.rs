//! What the GitLab and Forgejo readers share: how a remote's host is
//! reached, what a request carries, and the bounds every request keeps. A
//! token never travels over plain HTTP, nor through a redirect.

use super::{Checkpoint, Item, SourceKey, repo::RepoInfo};
use secrecy::SecretString;
use std::time::{Duration, SystemTime};

/// One sync's result. Only a `complete` listing may replace the cache.
pub struct Listing {
    pub items: Vec<Item>,
    pub checkpoint: Checkpoint,
    pub complete: bool,
}

/// How the API is reached. The shared source key names only the host, as
/// agent-launcher keys it, so the scheme comes from the remote's own URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    /// HTTP for an `http://` remote, whose host may serve nothing else;
    /// HTTPS for every other remote, SSH ones included.
    pub fn of_remote(url: &str) -> Self {
        match url::Url::parse(url) {
            Ok(url) if url.scheme() == "http" => Self::Http,
            _ => Self::Https,
        }
    }

    /// How `source` is read: over its repository's remote's scheme when that
    /// remote is the source's, else HTTPS.
    pub fn of_source(info: Option<&RepoInfo>, source: &SourceKey) -> Self {
        info.and_then(|info| info.remote.as_ref())
            .filter(|remote| remote.source == *source)
            .map_or(Self::Https, |remote| Self::of_remote(&remote.url))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

/// What a request carries: a token only over HTTPS, since anyone who can
/// watch a plain HTTP connection could take it. Over HTTP the token is
/// withheld and the read is anonymous, which public repositories allow.
#[derive(Clone, Copy, Debug)]
pub enum Credential<'a> {
    Anonymous,
    Withheld,
    Token(&'a SecretString),
}

impl<'a> Credential<'a> {
    pub fn new(scheme: Scheme, token: Option<&'a SecretString>) -> Self {
        match (scheme, token) {
            (_, None) => Self::Anonymous,
            (Scheme::Http, Some(_)) => Self::Withheld,
            (Scheme::Https, Some(token)) => Self::Token(token),
        }
    }
}

/// An agent for a forge's API: bounded by `timeout`, following no redirect,
/// so a token is never carried to another address, and returning refusals
/// as statuses to read.
pub(crate) fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

/// How long a rate-limited repository waits: `retry-after` seconds, else
/// until `ratelimit-reset`, bounded to between five minutes and a day.
pub fn retry_after(
    status: u16,
    headers: &ureq::http::HeaderMap,
    now: SystemTime,
) -> Option<Duration> {
    if status != 429 {
        return None;
    }
    let seconds = |name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    let epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let wait = seconds("retry-after")
        .or_else(|| seconds("ratelimit-reset").map(|reset| reset.saturating_sub(epoch)))
        .unwrap_or(3600);
    Some(Duration::from_secs(wait.clamp(300, 86400)))
}

/// A host setting such as `GITLAB_HOST` or `FORGEJO_HOST` may be a bare
/// host or a URL, as the GitLab CLI accepts.
pub(crate) fn configured_host(value: &str) -> String {
    let value = value.trim();
    url::Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| {
            let host = url.host_str()?.to_owned();
            Some(
                url.port()
                    .map_or(host.clone(), |port| format!("{host}:{port}")),
            )
        })
        .unwrap_or_else(|| value.trim_end_matches('/').to_owned())
}

/// A token is a short run of visible ASCII; anything else would corrupt the
/// header or is not a token at all.
pub(crate) fn valid(token: &str) -> Option<SecretString> {
    (!token.is_empty() && token.len() <= 1024 && token.bytes().all(|byte| byte.is_ascii_graphic()))
        .then(|| token.into())
}
