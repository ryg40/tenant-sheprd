//! `FORGEJO_TOKEN` reaches only the host `FORGEJO_HOST` names, and only
//! over HTTPS; every other Forgejo is read anonymously.
use crate::forge::{Credential, Scheme};
use crate::forgejo::{
    Error, account_host_with, account_token, authorization, environment_token, host_named,
};
use secrecy::{ExposeSecret, SecretString};
use std::{collections::HashMap, ffi::OsString};

fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs: HashMap<String, OsString> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
        .collect();
    move |name| pairs.get(name).cloned()
}

fn token(host: &str, pairs: &[(&str, &str)]) -> Option<String> {
    environment_token(host, environment(pairs)).map(|token| token.expose_secret().to_owned())
}

#[test]
fn the_token_reaches_only_the_host_forgejo_host_names() {
    let named = [
        ("FORGEJO_HOST", "forgejo.example.test"),
        ("FORGEJO_TOKEN", "abc123"),
    ];
    assert_eq!(
        token("forgejo.example.test", &named).as_deref(),
        Some("abc123")
    );
    assert_eq!(
        token("FORGEJO.example.test", &named).as_deref(),
        Some("abc123"),
        "host names compare without case"
    );
    assert_eq!(token("gitea.example.test", &named), None);
    assert_eq!(token("codeberg.org", &named), None);
    assert_eq!(token("forgejo.example.test:8443", &named), None);
}

/// Unlike `GITLAB_HOST`, `FORGEJO_HOST` has no default: without it no host
/// gets the token, codeberg.org included.
#[test]
fn without_forgejo_host_no_host_gets_the_token() {
    let alone = [("FORGEJO_TOKEN", "abc123")];
    for host in ["codeberg.org", "forgejo.example.test", "gitea.com"] {
        assert_eq!(token(host, &alone), None, "{host}");
    }
    let empty = [("FORGEJO_HOST", " "), ("FORGEJO_TOKEN", "abc123")];
    assert_eq!(token("codeberg.org", &empty), None);
}

#[test]
fn forgejo_host_may_be_a_bare_host_or_a_url() {
    let named = |value| host_named(environment(&[("FORGEJO_HOST", value)]));
    assert_eq!(
        named("forgejo.example.test").as_deref(),
        Some("forgejo.example.test")
    );
    assert_eq!(
        named("https://Forgejo.Example.test/").as_deref(),
        Some("forgejo.example.test")
    );
    assert_eq!(
        named("https://git.example.com:3000/forgejo").as_deref(),
        Some("git.example.com:3000")
    );
    assert_eq!(
        named("git.example.com:3000").as_deref(),
        Some("git.example.com:3000")
    );
    assert_eq!(host_named(environment(&[])), None);

    let url = [
        ("FORGEJO_HOST", "https://forgejo.example.test"),
        ("FORGEJO_TOKEN", " abc123\n"),
    ];
    assert_eq!(
        token("forgejo.example.test", &url).as_deref(),
        Some("abc123"),
        "the value is trimmed"
    );
}

#[test]
fn a_token_no_header_can_carry_is_not_sent() {
    for broken in ["has space", "", "line\nbreak", "é"] {
        let pairs = [
            ("FORGEJO_HOST", "forgejo.example.test"),
            ("FORGEJO_TOKEN", broken),
        ];
        assert_eq!(token("forgejo.example.test", &pairs), None, "{broken:?}");
    }
}

/// The token is never sent over plain HTTP, where anyone watching could
/// take it, even to the host it is meant for.
#[test]
fn the_token_travels_only_over_https() {
    let found = environment_token(
        "forgejo.example.test",
        environment(&[
            ("FORGEJO_HOST", "forgejo.example.test"),
            ("FORGEJO_TOKEN", "abc123"),
        ]),
    );
    assert!(matches!(
        Credential::new(Scheme::Https, found.as_ref()),
        Credential::Token(sent) if sent.expose_secret() == "abc123"
    ));
    assert!(matches!(
        Credential::new(Scheme::Http, found.as_ref()),
        Credential::Withheld
    ));
    assert!(matches!(
        Credential::new(Scheme::Https, None),
        Credential::Anonymous
    ));
    assert_eq!(
        Scheme::of_remote("http://forgejo.example.test/user/sheprd.git"),
        Scheme::Http
    );
    for remote in [
        "https://forgejo.example.test/user/sheprd.git",
        "ssh://git@forgejo.example.test:2222/user/sheprd.git",
        "git@forgejo.example.test:user/sheprd.git",
    ] {
        assert_eq!(Scheme::of_remote(remote), Scheme::Https, "{remote}");
    }
}

#[test]
fn the_token_goes_in_forgejos_header_marked_sensitive() {
    let header = authorization(&SecretString::from("abc123")).unwrap();
    assert_eq!(header.to_str().unwrap(), "token abc123");
    assert!(header.is_sensitive(), "a sensitive header is never logged");
}

/// The account search reads the root of the host `FORGEJO_HOST` names, so a
/// setting with a path gives the account no token, and it still shows, with
/// the refusal as its error. A checkout's source reads the address its remote
/// gives, so a Forgejo in a folder still gets the token.
#[test]
fn a_forgejo_host_with_a_path_gives_the_account_no_token_and_says_why() {
    for setting in [
        "https://git.example.com/forgejo",
        "https://git.example.com:3000/forgejo/",
        "git.example.com/forgejo",
    ] {
        let pairs = [("FORGEJO_HOST", setting), ("FORGEJO_TOKEN", "abc123")];
        let host = account_host_with(environment(&pairs));
        assert!(host.is_some(), "{setting}");
        let host = host.unwrap();
        let refused = account_token(&host, environment(&pairs));
        assert!(
            matches!(&refused, Err(Error::HostPath(named)) if named == setting),
            "{setting}: {refused:?}"
        );
        assert!(refused.unwrap_err().needs_user());
        assert_eq!(token(&host, &pairs).as_deref(), Some("abc123"), "{setting}");
    }
    // Another host is not refused, only left without the token.
    let pairs = [
        ("FORGEJO_HOST", "https://git.example.com/forgejo"),
        ("FORGEJO_TOKEN", "abc123"),
    ];
    assert!(matches!(
        account_token("codeberg.org", environment(&pairs)),
        Ok(None)
    ));
    for root in ["https://git.example.com/", "git.example.com:3000"] {
        let pairs = [("FORGEJO_HOST", root), ("FORGEJO_TOKEN", "abc123")];
        let host = host_named(environment(&pairs)).unwrap();
        let found = account_token(&host, environment(&pairs)).unwrap();
        assert_eq!(
            found.as_ref().map(|token| token.expose_secret()),
            Some("abc123"),
            "{root}"
        );
    }
}
