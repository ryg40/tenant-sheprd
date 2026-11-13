//! A Forgejo (or Gitea) repository's open issues and pull requests, read
//! through the REST API under `/api/v1` in GitHub's row format:
//! `native_id` is an issue's number and `pr/<number>` a pull request's,
//! both shown `#<number>`, since Forgejo numbers them in one sequence. Only
//! complete active listings replace active rows. `history` keeps bounded
//! settled samples separately. `moves` supplies approved issue deltas; the
//! native confirmation entry point remains disabled.
//!
//! A remote is Forgejo when its host's name holds `forgejo` or `gitea`, is
//! `codeberg.org` (`named_by`), or is the host `FORGEJO_HOST` names
//! (`recognize`). The login shell's `FORGEJO_TOKEN` is sent to that host
//! alone, and only over HTTPS (`environment_token`); any other Forgejo is
//! read anonymously, which public repositories allow.

mod error;
pub mod history;
pub mod issue;
pub mod moves;
pub mod pull_request;
pub mod search;

pub use error::Error;
pub use issue::IssueNode;
pub use pull_request::PullRequestNode;
pub use search::SearchNode;

use super::{
    Checkpoint, Item, Provider, SourceKey,
    forge::{Credential, Listing, Scheme, agent, configured_host, retry_after, valid},
};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use std::{
    collections::HashSet,
    ffi::OsString,
    io::Read,
    time::{Duration, SystemTime},
};
use zeroize::Zeroizing;

/// The page size asked for: Forgejo's default `MAX_RESPONSE_ITEMS`, the most
/// it answers unless its administrator changed that.
pub const PAGE: usize = 50;
/// The most open issues, and as many pull requests, one sync reads. A
/// repository with more is shown but not cached, since a capped listing is
/// no full sync.
pub const MAX_OPEN: usize = 2_000;
/// The most pages one listing reads: [`MAX_OPEN`] items at Forgejo's
/// default page size, with room for an instance whose `MAX_RESPONSE_ITEMS`
/// is smaller, or one whose pages hold entries of another kind. A listing
/// with more is capped.
pub const MAX_PAGES: u32 = 100;
const TIMEOUT: Duration = Duration::from_secs(30);
/// One page of 50 issues or pull requests, bodies and repositories included.
const LIMIT: u64 = 4 * 1024 * 1024;
/// Names the one host [`TOKEN_VARIABLE`] is sent to.
pub const HOST_VARIABLE: &str = "FORGEJO_HOST";
pub const TOKEN_VARIABLE: &str = "FORGEJO_TOKEN";

/// Whether a remote's lowercase `host` reads as Forgejo by its name alone.
pub fn named_by(host: &str) -> bool {
    host.contains("forgejo") || host.contains("gitea") || host == "codeberg.org"
}

/// `source` as a Forgejo repository when `FORGEJO_HOST` names its host,
/// though its name alone reads as GitHub or GitLab. Only the provider
/// changes; the host and path stay as agent-launcher keys them. `named`
/// reads the setting, only when needed. Returns whether `source` changed.
pub fn recognize(source: &mut SourceKey, named: impl FnOnce() -> Option<String>) -> bool {
    if !matches!(source.provider, Provider::Github | Provider::Gitlab)
        || matches!(source.host.as_str(), "github.com" | "gitlab.com")
    {
        return false;
    }
    let known = named().is_some_and(|host| host.eq_ignore_ascii_case(&source.host));
    if known {
        source.provider = Provider::Forgejo;
    }
    known
}

/// The host `FORGEJO_HOST` names in the login shell (or the app's own
/// environment). Blocks the first time on Unix: call it from a worker.
pub fn named_host() -> Option<String> {
    host_named(sheprd_client::login_env::var)
}

/// The lowercase host `variable("FORGEJO_HOST")` names, a bare host or a
/// URL, if it names one.
pub fn host_named(variable: impl Fn(&str) -> Option<OsString>) -> Option<String> {
    host_of(&variable(HOST_VARIABLE)?.into_string().ok()?)
}

/// The lowercase host `value`, a bare host or a URL, names, as
/// `FORGEJO_HOST` would give it.
pub fn host_of(value: &str) -> Option<String> {
    let host = configured_host(value).to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// The host whose open issues the Issues section lists across every
/// repository the token reads (`search`): the one `FORGEJO_HOST` names, when
/// `FORGEJO_TOKEN` holds a token for it. Blocks the first time on Unix, as
/// [`named_host`] does: call it off the UI thread.
pub fn account_host() -> Option<String> {
    account_host_with(sheprd_client::login_env::var)
}

/// [`account_host`] from `variable`.
/// A `FORGEJO_HOST` that names a folder still gives its host, so the
/// listing shows why it reads nothing.
pub fn account_host_with(variable: impl Fn(&str) -> Option<OsString>) -> Option<String> {
    let host = host_named(&variable)?;
    match account_token(&host, variable) {
        Ok(None) => None,
        Ok(Some(_)) | Err(_) => Some(host),
    }
}

/// `FORGEJO_TOKEN`, for `host` alone: the one `FORGEJO_HOST` names. With no
/// `FORGEJO_HOST`, no host gets it, so a token never reaches an instance it
/// was not made for. A checkout's source sends it to the address its remote
/// gives, so a `FORGEJO_HOST` with a path, an instance in a folder, works.
pub fn environment_token(
    host: &str,
    variable: impl Fn(&str) -> Option<OsString>,
) -> Option<SecretString> {
    if !host_named(&variable)?.eq_ignore_ascii_case(host) {
        return None;
    }
    valid(variable(TOKEN_VARIABLE)?.into_string().ok()?.trim())
}

/// [`environment_token`] for the account search, which builds its address
/// from `FORGEJO_HOST` and reads the host's root (`search_url`). A
/// `FORGEJO_HOST` URL with a path is refused ([`Error::HostPath`]): the
/// search would send the token to the root, not to the instance in the
/// folder.
pub fn account_token(
    host: &str,
    variable: impl Fn(&str) -> Option<OsString>,
) -> Result<Option<SecretString>, Error> {
    let Some(token) = environment_token(host, &variable) else {
        return Ok(None);
    };
    match variable(HOST_VARIABLE).and_then(|value| value.into_string().ok()) {
        Some(setting) if names_a_folder(&setting) => {
            Err(Error::HostPath(setting.trim().to_owned()))
        }
        _ => Ok(Some(token)),
    }
}

/// Whether `value`, a bare host or a URL as `FORGEJO_HOST` holds, names a
/// path after its host.
fn names_a_folder(value: &str) -> bool {
    let value = value.trim();
    url::Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .or_else(|| url::Url::parse(&format!("https://{value}")).ok())
        .is_none_or(|url| !matches!(url.path(), "" | "/"))
}

/// A kind of item Forgejo lists a page at a time.
pub trait Listed: DeserializeOwned {
    /// The collection's path after `/repos/{owner}/{repo}/`.
    const COLLECTION: &'static str;
    /// The query that narrows it to open items of this kind.
    const FILTER: &'static [(&'static str, &'static str)];

    fn number(&self) -> u64;
    /// The item, or `None` for an entry of another kind.
    fn into_item(self, source: &SourceKey) -> Option<Item>;
}

/// One page of a listing, and how many items the whole listing holds when
/// Forgejo said (`X-Total-Count`).
pub struct Page<N> {
    pub nodes: Vec<N>,
    pub total: Option<u64>,
}

/// What Forgejo answered for one page.
pub enum Reply<N> {
    Page(Page<N>),
    /// The collection is not there: a repository with its issues or pull
    /// requests turned off, or an outside issue tracker, answers 404 for it.
    Missing,
}

/// Lists `source`'s open issues and pull requests over `scheme`, reading
/// Forgejo as `token` when given and the scheme keeps it private.
/// `previous` keeps agent-launcher's markers, which herdr-gpui never adds.
/// Blocks on the network: call it from a worker.
pub fn sync(
    (source, scheme): (&SourceKey, Scheme),
    token: Option<&SecretString>,
    previous: Option<&Checkpoint>,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> super::Result<Listing> {
    let started = Utc::now();
    let agent = agent(TIMEOUT);
    let credential = Credential::new(scheme, token);
    let repository = (source, scheme);
    let issues = read::<IssueNode>(&agent, repository, credential, cancelled, cooldown)?;
    let pulls = read::<PullRequestNode>(&agent, repository, credential, cancelled, cooldown)?;
    let (items, complete) = combine(source, credential, issues, pulls)?;
    let previous = previous.cloned().unwrap_or_default();
    Ok(Listing {
        items,
        checkpoint: Checkpoint {
            updated_at: Some(started),
            etag: None,
            last_full_at: Some(Utc::now()),
            pr_details: previous.pr_details,
            pr_cursor: previous.pr_cursor,
        },
        complete,
    })
}

/// [`list`] over the network, stopping when `cancelled`.
fn read<N: Listed>(
    agent: &ureq::Agent,
    (source, scheme): (&SourceKey, Scheme),
    credential: Credential<'_>,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> super::Result<Option<(Vec<Item>, bool)>> {
    list::<N>(source, &mut |number| {
        if cancelled() {
            return Err(super::Error::Cancelled);
        }
        Ok(fetch_page(
            agent,
            (source, scheme),
            credential,
            number,
            cooldown,
        )?)
    })
}

/// Every open item of kind `N`, oldest first, and whether the listing is
/// complete; `None` when Forgejo answers a page of it as missing. A page
/// shorter than [`PAGE`] is the last, unless the count Forgejo gave says
/// more are open, as when its `MAX_RESPONSE_ITEMS` is below [`PAGE`]. It is
/// capped past [`MAX_OPEN`] items or [`MAX_PAGES`] pages. Pages
/// are numbered, so an item closed while later pages are read can move
/// another onto a page already read; the next sync lists it again.
pub fn list<N: Listed>(
    source: &SourceKey,
    fetch: &mut impl FnMut(u32) -> super::Result<Reply<N>>,
) -> super::Result<Option<(Vec<Item>, bool)>> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for number in 1..=MAX_PAGES {
        let Reply::Page(page) = fetch(number)? else {
            return Ok(None);
        };
        let read = page.nodes.len();
        let before = seen.len();
        items.extend(
            page.nodes
                .into_iter()
                .filter(|node| seen.insert(node.number()))
                .filter_map(|node| node.into_item(source)),
        );
        let more = read >= PAGE || page.total.is_some_and(|total| (seen.len() as u64) < total);
        if read == 0 || !more {
            return Ok(Some((items, true)));
        }
        // A page with nothing new would be asked for again and again.
        if items.len() >= MAX_OPEN || seen.len() == before {
            return Ok(Some((items, false)));
        }
    }
    // Full pages of entries of another kind, as from a server that ignores
    // the filter, would otherwise be read without end.
    Ok(Some((items, false)))
}

/// One listing of both collections. A collection Forgejo answered as
/// missing is empty, unless both were: then the repository itself is
/// missing, or hidden from this reader.
pub fn combine(
    source: &SourceKey,
    credential: Credential<'_>,
    issues: Option<(Vec<Item>, bool)>,
    pulls: Option<(Vec<Item>, bool)>,
) -> Result<(Vec<Item>, bool), Error> {
    if issues.is_none() && pulls.is_none() {
        return Err(status_error(404, source, credential));
    }
    // A missing collection holds nothing, so its listing is complete.
    let (mut items, issues_complete) = issues.unwrap_or((Vec::new(), true));
    let (pulls, pulls_complete) = pulls.unwrap_or((Vec::new(), true));
    items.extend(pulls);
    Ok((items, issues_complete && pulls_complete))
}

/// The API address of page `number` of `source`'s open items of kind `N`,
/// oldest first, so items opened while the listing is read only add pages.
pub fn page_url<N: Listed>(
    source: &SourceKey,
    scheme: Scheme,
    number: u32,
) -> Result<url::Url, Error> {
    let mut url = collection_url(source, scheme, N::COLLECTION)?;
    url.query_pairs_mut()
        .extend_pairs(N::FILTER.iter().copied())
        .append_pair("sort", "oldest")
        .append_pair("limit", &PAGE.to_string())
        .append_pair("page", &number.to_string());
    Ok(url)
}

/// The API address of `source`'s `collection`: its path ends in
/// `owner/name`, after any folder the instance is served under.
fn collection_url(source: &SourceKey, scheme: Scheme, collection: &str) -> Result<url::Url, Error> {
    let invalid = || Error::Address(source.repository.clone());
    let mut folders: Vec<&str> = source.repository.split('/').collect();
    let (Some(name), Some(owner)) = (folders.pop(), folders.pop()) else {
        return Err(invalid());
    };
    if folders
        .iter()
        .chain([&owner, &name])
        .any(|part| part.is_empty())
    {
        return Err(invalid());
    }
    let mut url = url::Url::parse(&format!("{}://{}", scheme.as_str(), source.host))
        .map_err(|_| invalid())?;
    url.path_segments_mut()
        .map_err(|()| invalid())?
        .pop_if_empty()
        .extend(folders)
        .extend(["api", "v1", "repos", owner, name, collection]);
    Ok(url)
}

fn fetch_page<N: Listed>(
    agent: &ureq::Agent,
    (source, scheme): (&SourceKey, Scheme),
    credential: Credential<'_>,
    number: u32,
    cooldown: &mut Option<Duration>,
) -> Result<Reply<N>, Error> {
    let url = page_url::<N>(source, scheme, number)?;
    fetch(
        agent,
        &url,
        (source, scheme),
        credential,
        N::COLLECTION,
        cooldown,
    )
}

/// One page of a listing at `url`, read for `source`, which names the host
/// in failures; `collection` names the listing in the log.
fn fetch<T: DeserializeOwned>(
    agent: &ureq::Agent,
    url: &url::Url,
    (source, scheme): (&SourceKey, Scheme),
    credential: Credential<'_>,
    collection: &str,
    cooldown: &mut Option<Duration>,
) -> Result<Reply<T>, Error> {
    let mut request = agent
        .get(url.as_str())
        .header("User-Agent", "Sheprd")
        .header("Accept", "application/json");
    if let Credential::Token(token) = credential {
        request = request.header("Authorization", authorization(token)?);
    }
    let mut reply = request.call().map_err(|error| Error::Network {
        origin: format!("{}://{}", scheme.as_str(), source.host),
        source: Box::new(error),
    })?;
    let status = reply.status().as_u16();
    if status == 404 {
        return Ok(Reply::Missing);
    }
    if status != 200 {
        *cooldown = retry_after(status, reply.headers(), SystemTime::now());
        tracing::warn!(
            category = "orchestrator_forgejo",
            status,
            host = source.host.as_str(),
            collection,
            "Forgejo refused a listing"
        );
        return Err(status_error(status, source, credential));
    }
    let total = reply
        .headers()
        .get("x-total-count")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok());
    let nodes = serde_json::from_slice(&body(&mut reply, LIMIT)?).map_err(Error::Json)?;
    Ok(Reply::Page(Page { nodes, total }))
}

/// A reply's body, refused when it is larger than `limit`.
fn body(reply: &mut ureq::http::Response<ureq::Body>, limit: u64) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    reply
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::Read)?;
    if bytes.len() as u64 > limit {
        return Err(Error::Size);
    }
    Ok(bytes)
}

/// The header Forgejo reads a token from, marked sensitive so it is never
/// logged.
pub(crate) fn authorization(token: &SecretString) -> Result<ureq::http::HeaderValue, Error> {
    let text = Zeroizing::new(format!("token {}", token.expose_secret()));
    let mut header = ureq::http::HeaderValue::from_str(&text).map_err(|_| Error::Token)?;
    header.set_sensitive(true);
    Ok(header)
}

/// What a refusal means. Forgejo answers a private repository as missing to
/// a reader without a token, so that asks for one rather than blaming the
/// remote.
pub fn status_error(status: u16, source: &SourceKey, credential: Credential<'_>) -> Error {
    let repository = source.repository.clone();
    match (status, credential) {
        (401 | 403 | 404, Credential::Anonymous) => Error::SignIn {
            repository,
            host: source.host.clone(),
        },
        (401 | 403 | 404, Credential::Withheld) => Error::InsecureRemote { repository },
        (401, Credential::Token(_)) => Error::Authentication,
        (403, Credential::Token(_)) => Error::Forbidden { repository },
        (404, Credential::Token(_)) => Error::NotFound { repository },
        (429, _) => Error::RateLimit,
        (status, _) => Error::Status(status),
    }
}
