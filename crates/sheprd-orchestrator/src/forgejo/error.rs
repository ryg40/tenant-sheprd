//! Why a Forgejo repository could not be read.

/// Why a Forgejo listing failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("Forgejo's address for {0:?} is not valid")]
    Address(String),
    #[error(
        "Forgejo refused FORGEJO_TOKEN; change it, or unset it to read public repositories, and restart Sheprd"
    )]
    Authentication,
    #[error(
        "The token cannot read {repository}; it needs the read:issue and read:repository scopes"
    )]
    Forbidden { repository: String },
    /// `FORGEJO_HOST` names a folder on its host, which the token, matched
    /// to the host alone, would reach beyond.
    #[error(
        "FORGEJO_HOST {0:?} names a path; the list of every repository's issues reads only a Forgejo at the root of its host, so set FORGEJO_HOST to the host alone and restart Sheprd"
    )]
    HostPath(String),
    #[error(
        "Forgejo shows {repository} only when signed in, and a token is never sent over plain HTTP: use an https:// remote"
    )]
    InsecureRemote { repository: String },
    #[error("Forgejo's answer is not the expected JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("Forgejo could not be reached at {origin}: {source}")]
    Network {
        /// The scheme and host tried, so a wrong one shows.
        origin: String,
        #[source]
        source: Box<ureq::Error>,
    },
    /// An action asked of an issue the account listing alone shows.
    #[error("Open a checkout of the repository as a workspace to act on its issues from {host}")]
    NoCheckout { host: String },
    #[error("Forgejo shows no repository {repository} to this token")]
    NotFound { repository: String },
    #[error("Forgejo rate limit reached; it is read again later")]
    RateLimit,
    #[error("Forgejo's answer could not be read: {0}")]
    Read(#[source] std::io::Error),
    #[error(
        "Forgejo shows {repository} only when signed in: set FORGEJO_TOKEN in your shell, with FORGEJO_HOST naming {host}, and restart Sheprd"
    )]
    SignIn { repository: String, host: String },
    #[error("Forgejo's answer exceeded the size limit")]
    Size,
    #[error("Forgejo request failed (HTTP {0})")]
    Status(u16),
    #[error("FORGEJO_TOKEN is not one an HTTP header can carry")]
    Token,
    // keep-sorted end
}

impl Error {
    /// A failure only the user can fix, such as a missing or refused token,
    /// which waits longer before it is tried again.
    pub fn needs_user(&self) -> bool {
        matches!(
            self,
            Self::Authentication
                | Self::Forbidden { .. }
                | Self::HostPath(_)
                | Self::InsecureRemote { .. }
                | Self::NotFound { .. }
                | Self::SignIn { .. }
        )
    }
}
