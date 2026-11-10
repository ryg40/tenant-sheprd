use super::*;
use sheprd_orchestrator::{Error, forgejo};

#[test]
fn every_forgejo_variant_has_a_message() {
    let repository = || "team/app".to_owned();
    assert_localized(&[
        forgejo::Error::Address("team".into()),
        forgejo::Error::Authentication,
        forgejo::Error::Forbidden {
            repository: repository(),
        },
        forgejo::Error::HostPath("https://git.example.com/forgejo".into()),
        forgejo::Error::InsecureRemote {
            repository: repository(),
        },
        forgejo::Error::Json(serde_json::from_str::<u8>("x").unwrap_err()),
        forgejo::Error::Network {
            origin: "https://codeberg.org".into(),
            source: Box::new(ureq::Error::HostNotFound),
        },
        forgejo::Error::NoCheckout {
            host: "codeberg.org".into(),
        },
        forgejo::Error::NotFound {
            repository: repository(),
        },
        forgejo::Error::RateLimit,
        forgejo::Error::Read(std::io::Error::other("pipe closed")),
        forgejo::Error::SignIn {
            repository: repository(),
            host: "codeberg.org".into(),
        },
        forgejo::Error::Size,
        forgejo::Error::Status(500),
        forgejo::Error::Token,
    ]);
}

#[test]
fn the_orchestrator_shows_a_forgejo_failure_as_its_own() {
    assert_localized(&[Error::Forgejo(forgejo::Error::RateLimit)]);
}
