//! Which remotes read as Forgejo: a host named for Forgejo or Gitea, or
//! codeberg.org, while GitHub and GitLab hosts read as before.
use super::*;
use crate::{forgejo, repo::parse_remote_url};

fn parsed(url: &str) -> (Provider, String, String) {
    let key = parse_remote_url(url).unwrap();
    (key.provider, key.host, key.repository)
}

#[test]
fn forgejo_and_gitea_hosts_read_as_forgejo_in_every_remote_form() {
    let cases = [
        // An SSH remote on its own port: the API stays on HTTPS's.
        (
            "ssh://git@forgejo.example.test:2222/user/sheprd.git",
            "forgejo.example.test",
            "user/sheprd",
        ),
        (
            "https://forgejo.example.test/user/sheprd.git",
            "forgejo.example.test",
            "user/sheprd",
        ),
        (
            "git@forgejo.example.test:user/sheprd.git",
            "forgejo.example.test",
            "user/sheprd",
        ),
        (
            "https://Gitea.Example.test:3000/team/app",
            "gitea.example.test:3000",
            "team/app",
        ),
        ("git@codeberg.org:team/app.git", "codeberg.org", "team/app"),
        (
            "https://codeberg.org/team/app.git",
            "codeberg.org",
            "team/app",
        ),
        // A host whose name holds `ghe` by chance is still Forgejo's.
        (
            "git@forgejo.ghent.example:team/app.git",
            "forgejo.ghent.example",
            "team/app",
        ),
        // An instance served under a folder keeps it in the path.
        (
            "https://forgejo.example.com/git/team/app.git",
            "forgejo.example.com",
            "git/team/app",
        ),
    ];
    for (url, host, repository) in cases {
        let (provider, parsed_host, parsed_repository) = parsed(url);
        assert_eq!(
            (provider, parsed_host.as_str(), parsed_repository.as_str()),
            (Provider::Forgejo, host, repository),
            "{url}"
        );
    }
}

#[test]
fn github_and_gitlab_hosts_are_unchanged() {
    let cases = [
        ("git@github.com:sheprd-labs/sheprd.git", Provider::Github),
        ("https://github.example.com/team/app", Provider::Github),
        ("git@ghe.example.com:team/app.git", Provider::Github),
        ("git@gitlab.com:group/sub/app.git", Provider::Gitlab),
        ("https://gitlab.example.com/team/app", Provider::Gitlab),
        // A host naming both keeps its GitLab or GitHub reading.
        (
            "git@gitlab-gitea.example.com:team/app.git",
            Provider::Gitlab,
        ),
        (
            "git@github-gitea.example.com:team/app.git",
            Provider::Github,
        ),
        // An unnamed host keeps the old guesses.
        ("git@git.example.com:team/app.git", Provider::Github),
        ("git@git.example.com:group/sub/app.git", Provider::Gitlab),
    ];
    for (url, provider) in cases {
        assert_eq!(parsed(url).0, provider, "{url}");
    }
}

#[test]
fn a_forgejo_key_round_trips_through_the_shared_database() {
    let key = parse_remote_url("git@codeberg.org:team/app.git").unwrap();
    assert_eq!(key.canonical(), "forgejo:codeberg.org:team/app");
    assert_eq!(
        "forgejo".parse::<Provider>().unwrap(),
        Provider::Forgejo,
        "the provider word reads back"
    );
    assert_eq!(Provider::Forgejo.as_str(), "forgejo");
    let item = ItemKey {
        source: key.clone(),
        native_id: "pr/12".into(),
    };
    assert_eq!(item.canonical().parse::<ItemKey>().unwrap(), item);
    assert_eq!(Item::stand_in(item).identifier, "#12", "Forgejo writes #N");
}

#[test]
fn only_a_named_host_reads_as_forgejo_by_its_name() {
    for host in ["forgejo.example.com", "git.gitea.io", "codeberg.org"] {
        assert!(forgejo::named_by(host), "{host}");
    }
    for host in [
        "github.com",
        "gitlab.com",
        "codeberg.org.example.com",
        "git.example.com",
    ] {
        assert!(!forgejo::named_by(host), "{host}");
    }
}

/// `FORGEJO_HOST` makes a host whose name reads as GitHub or GitLab a
/// Forgejo one; github.com and gitlab.com themselves never change.
#[test]
fn the_host_forgejo_host_names_is_recognized() {
    let mut source = parse_remote_url("git@git.example.com:team/app.git").unwrap();
    assert_eq!(source.provider, Provider::Github);
    assert!(!forgejo::recognize(&mut source, || None));
    assert!(!forgejo::recognize(&mut source, || Some(
        "other.example.com".into()
    )));
    assert_eq!(source.provider, Provider::Github);
    assert!(forgejo::recognize(&mut source, || Some(
        "Git.Example.com".into()
    )));
    assert_eq!(source.provider, Provider::Forgejo);
    assert_eq!(
        (source.host.as_str(), source.repository.as_str()),
        ("git.example.com", "team/app"),
        "the key keeps its host and path"
    );
    // Once Forgejo, the setting is not read again.
    assert!(!forgejo::recognize(&mut source, || unreachable!()));

    let mut nested = parse_remote_url("https://git.example.com/git/team/app").unwrap();
    assert_eq!(nested.provider, Provider::Gitlab);
    assert!(forgejo::recognize(&mut nested, || Some(
        "git.example.com".into()
    )));
    assert_eq!(nested.provider, Provider::Forgejo);

    for url in ["git@github.com:team/app.git", "git@gitlab.com:team/app.git"] {
        let mut source = parse_remote_url(url).unwrap();
        let before = source.provider;
        assert!(!forgejo::recognize(&mut source, || unreachable!()), "{url}");
        assert_eq!(source.provider, before, "{url}");
    }
    let mut azure = parse_remote_url("https://dev.azure.com/org/project/_git/repo").unwrap();
    assert!(!forgejo::recognize(&mut azure, || unreachable!()));
}
