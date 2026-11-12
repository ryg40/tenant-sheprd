//! One reusable lookup thread for all Walkr agents, including retired workers.
use std::{
    io::ErrorKind,
    net::IpAddr,
    sync::{Arc, Mutex, OnceLock, mpsc},
    thread,
};
use ureq::{
    config::Config,
    http::Uri,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::NextTimeout,
    },
};

type Lookup = Result<ResolvedSocketAddrs, ureq::Error>;

#[derive(Clone, Debug, Default)]
pub(super) struct Dns(Arc<Mutex<Option<Service>>>);

#[derive(Debug)]
struct Service {
    requests: mpsc::SyncSender<(Uri, Config)>,
    replies: mpsc::Receiver<Lookup>,
    outstanding: bool,
}

pub(super) fn shared() -> Dns {
    static DNS: OnceLock<Dns> = OnceLock::new();
    DNS.get_or_init(Dns::default).clone()
}

impl Service {
    fn start(
        lookup: impl Fn(&Uri, &Config) -> Lookup + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let (requests, received) = mpsc::sync_channel::<(Uri, Config)>(1);
        let (sender, replies) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("herdr-walkr-dns".into())
            .spawn(move || {
                while let Ok((uri, config)) = received.recv() {
                    if sender.send(lookup(&uri, &config)).is_err() {
                        return;
                    }
                }
            })?;
        Ok(Self {
            requests,
            replies,
            outstanding: false,
        })
    }
}

impl Resolver for Dns {
    fn resolve(&self, uri: &Uri, config: &Config, timeout: NextTimeout) -> Lookup {
        if uri
            .host()
            .is_some_and(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok())
        {
            // Numeric addresses do not use the system name service.
            return DefaultResolver::default().resolve(uri, config, NextTimeout::default());
        }
        // Another probe may be waiting here. Never queue behind its lookup.
        let Ok(mut state) = self.0.try_lock() else {
            return Err(ureq::Error::Timeout(timeout.reason));
        };
        if state.is_none() {
            *state = Some(Service::start(|uri, config| {
                // No timeout here: DefaultResolver must not spawn a detached thread.
                DefaultResolver::default()
                    .resolve(uri, config, NextTimeout::default())
                    .map_err(|error| match error {
                        ureq::Error::Io(error) if error.kind() != ErrorKind::TimedOut => {
                            ureq::Error::HostNotFound
                        }
                        error => error,
                    })
            })?);
        }
        let Some(service) = state.as_mut() else {
            return Err(ureq::Error::HostNotFound);
        };
        if service.outstanding {
            match service.replies.try_recv() {
                // A timed-out lookup belongs to an older request. Discard it.
                Ok(_) => service.outstanding = false,
                Err(mpsc::TryRecvError::Empty) => {
                    return Err(ureq::Error::Timeout(timeout.reason));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(ureq::Error::HostNotFound);
                }
            }
        }
        service
            .requests
            .try_send((uri.clone(), config.clone()))
            .map_err(|_| ureq::Error::HostNotFound)?;
        service.outstanding = true;
        match service.replies.recv_timeout(*timeout.after) {
            Ok(result) => {
                service.outstanding = false;
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ureq::Error::Timeout(timeout.reason)),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ureq::Error::HostNotFound),
        }
    }
}

#[cfg(test)]
mod tests;
