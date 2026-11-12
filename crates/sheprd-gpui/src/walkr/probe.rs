//! One bounded GET, with no redirect, credential, or response-body read.
use super::{Connectivity, Unreachable, resolver};
use crate::browser::WebUrl;
use std::{io::ErrorKind, time::Duration};
use ureq::unversioned::transport::DefaultConnector;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Answers,
    Unreachable(Unreachable),
    NetworkRefused,
    Down(u16),
    Unexpected(u16),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Unseen,
    #[cfg_attr(
        not(any(embedded_pages, test)),
        expect(dead_code, reason = "this build has no embedded page")
    )]
    OnWalkr,
    #[cfg_attr(
        not(any(embedded_pages, test)),
        expect(dead_code, reason = "this build has no embedded page")
    )]
    /// Another origin, by its host alone: a path or query may hold a secret.
    Elsewhere(String),
}

pub(super) fn classify(outcome: Result<u16, Unreachable>) -> Reach {
    match outcome {
        Ok(200..=399) => Reach::Answers,
        Ok(403) => Reach::NetworkRefused,
        Ok(status @ 500..=599) => Reach::Down(status),
        Ok(status) => Reach::Unexpected(status),
        Err(cause) => Reach::Unreachable(cause),
    }
}

pub(super) fn connectivity(configured: bool, reach: Option<Reach>, page: &Page) -> Connectivity {
    if !configured {
        return Connectivity::NotConfigured;
    }
    match reach {
        None => Connectivity::Checking,
        Some(Reach::Answers) => match page {
            Page::Unseen | Page::Elsewhere(_) => Connectivity::Reachable,
            Page::OnWalkr => Connectivity::Ready,
        },
        Some(Reach::Unreachable(cause)) => Connectivity::Unreachable { cause },
        Some(Reach::NetworkRefused) => Connectivity::NetworkRefused,
        Some(Reach::Down(status)) => Connectivity::Down { status },
        Some(Reach::Unexpected(status)) => Connectivity::Unexpected { status },
    }
}

pub(super) fn agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        // Do not inherit proxy credentials from the app's environment.
        .proxy(None)
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), resolver::shared())
}

pub(super) fn probe(agent: &ureq::Agent, url: &WebUrl, path: &str) -> Reach {
    // Append to the origin, not URL::join: // in a configured path must
    // never select a different authority (or supply user info).
    let address = format!("{}{path}", url.origin());
    let outcome = agent
        .get(&address)
        .header("Connection", "close")
        .call()
        .map(|response| response.status().as_u16())
        .map_err(|error| unreachable(error, url.as_str().starts_with("https:")));
    classify(outcome)
}

fn unreachable(error: ureq::Error, https: bool) -> Unreachable {
    match error {
        ureq::Error::HostNotFound => Unreachable::Dns,
        ureq::Error::Timeout(_) => Unreachable::Timeout,
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => Unreachable::Tls,
        ureq::Error::Io(error) => match error.kind() {
            ErrorKind::TimedOut | ErrorKind::WouldBlock => Unreachable::Timeout,
            // Rustls reports a rejected handshake through std::io::Error.
            ErrorKind::InvalidData if https => Unreachable::Tls,
            _ => Unreachable::Connect,
        },
        _ => Unreachable::Connect,
    }
}
