//! Why a host's sessions that wait for a pick could not be counted, or the
//! page of one could not open. No variant holds text of a session or any
//! part of a page address: session files are untrusted data, and the
//! address holds the secret of a Walkr server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Windows has no `/bin/sh`, which the reader of this computer's Walkr
    /// server runs in, so the page of a session of this computer does not
    /// open there.
    #[cfg(windows)]
    #[error("The page of a Walkr session of this computer cannot open on Windows")]
    LocalUnsupported,
    /// The reader of a host's output met a line it does not know.
    #[error("The host answered with output that is not a list of Walkr sessions")]
    Malformed,
    #[error("No home folder is set, so no saved Walkr session is found")]
    NoHome,
    #[error("This build of Sheprd draws no pages")]
    NoPages,
    #[error("The host has no python3, which the read of its Walkr sessions needs")]
    NoPython,
    #[error("No Walkr server record is on the host")]
    NoServer,
    #[error("The Walkr server on the host serves another Compare folder, or gave no page")]
    OtherServer,
    /// The page address of the host is not `http` on a loopback host with a
    /// port, or names another session.
    #[error("The host gave a Walkr page address that Sheprd does not open")]
    PageRefused,
    /// The tunnel could not take the Walkr server's own port on this
    /// computer. The server answers only a loopback `Host` with its own
    /// port, so a page through any other port would be refused.
    #[error("Port {0} is busy on this computer, and the Walkr page needs it")]
    PortBusy(u16),
    #[error("Could not read the folder of the Walkr sessions")]
    Read(#[source] std::io::Error),
    #[error("Could not read the Walkr sessions on the host")]
    Script(#[from] sheprd_client::Error),
    #[error("The Walkr server on the host does not answer")]
    ServerStopped,
    #[error("The Walkr sessions could not be read in time")]
    TimedOut,
    #[error("Could not open an SSH tunnel to the Walkr server on the host")]
    Tunnel(#[source] Box<crate::Error>),
    #[error("Could not start a background thread for the Walkr count")]
    Worker(#[source] std::io::Error),
}
