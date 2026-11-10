//! Forgejo failures from `error-orchestrator-forgejo.ftl`. Network, parser,
//! and I/O text stays a placeable.

use super::Localize;
use crate::i18n::fl;
use sheprd_orchestrator::forgejo::Error;

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            // keep-sorted start
            Self::Address(repository) => fl!(
                "error-orchestrator-forgejo-address",
                repository = format!("{repository:?}")
            ),
            Self::Authentication => fl!("error-orchestrator-forgejo-authentication"),
            Self::Forbidden { repository } => fl!(
                "error-orchestrator-forgejo-forbidden",
                repository = repository.as_str()
            ),
            Self::HostPath(setting) => fl!(
                "error-orchestrator-forgejo-host-path",
                setting = format!("{setting:?}")
            ),
            Self::InsecureRemote { repository } => fl!(
                "error-orchestrator-forgejo-insecure-remote",
                repository = repository.as_str()
            ),
            Self::Json(error) => fl!("error-orchestrator-forgejo-json", error = error.to_string()),
            Self::Network { origin, source } => fl!(
                "error-orchestrator-forgejo-network",
                origin = origin.as_str(),
                error = source.to_string()
            ),
            Self::NoCheckout { host } => fl!(
                "error-orchestrator-forgejo-no-checkout",
                host = host.as_str()
            ),
            Self::NotFound { repository } => fl!(
                "error-orchestrator-forgejo-not-found",
                repository = repository.as_str()
            ),
            Self::RateLimit => fl!("error-orchestrator-forgejo-rate-limit"),
            Self::Read(error) => fl!("error-orchestrator-forgejo-read", error = error.to_string()),
            Self::SignIn { repository, host } => fl!(
                "error-orchestrator-forgejo-sign-in",
                repository = repository.as_str(),
                host = host.as_str()
            ),
            Self::Size => fl!("error-orchestrator-forgejo-size"),
            Self::Status(status) => fl!("error-orchestrator-forgejo-status", status = *status),
            Self::Token => fl!("error-orchestrator-forgejo-token"),
            // keep-sorted end
        }
    }
}
