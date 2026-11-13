//! A worker that reads every open issue the Forgejo token for one host
//! reads, across all of its repositories, with no checkout behind it
//! (`forgejo::search`). It answers the same [`Service`] the checkout worker
//! does, so the Issues section lists these issues beside the checkouts'.
//!
//! Unlike Azure DevOps's remote worker it keeps a database of its own, keyed
//! by the host (`Repository::ForgejoAccount`) in the same state directory:
//! a restart shows the last complete listing at once, and `sheprd
//! orchestrator issues --forgejo HOST` reads it. A capped listing is shown
//! and not written. It reads no branches and refuses checkout actions. The
//! shared move lifecycle accepts only the narrow Forgejo issue adapter, whose
//! preflight checks the current repository listing and access independently.
//! Cached rows grant no tracker permission. Without a token it reads nothing
//! and stores nothing.

use super::{
    COMMANDS, Command, Error, Notice, RemoteRequest, Result, Service, Snapshot, SyncState, Timing,
    push, retry_wait, schedule,
};
use crate::{
    Access, Repository, Store, database_path, forge::Listing, forgejo, location::database_path_in,
};
use chrono::Utc;
use secrecy::SecretString;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    time::{Duration, Instant},
};

impl Service {
    /// Starts a worker reading the open issues of every repository the
    /// token for `host` reads.
    pub(super) fn start_forgejo_account(
        host: String,
        request: RemoteRequest,
    ) -> std::io::Result<Self> {
        let (service, worker, receiver) = Self::prepare_forgejo_account(host, request);
        std::thread::Builder::new()
            .name("herdr-orchestrator-forgejo".into())
            .spawn(move || worker.run(&receiver))?;
        Ok(service)
    }

    /// The service and the worker it will talk to, not yet started.
    pub(super) fn prepare_forgejo_account(
        host: String,
        request: RemoteRequest,
    ) -> (Self, AccountWorker, mpsc::Receiver<Command>) {
        let (commands, receiver) = mpsc::sync_channel(COMMANDS);
        let mailbox = Arc::new(Mutex::new(None));
        let notices = Arc::new(Mutex::new(VecDeque::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let visible = Arc::new(AtomicBool::new(true));
        let snapshot = request.scope.snapshot();
        let moves = super::Moves::new(super::kanban::moves::shared_gate(), Arc::default());
        let worker = AccountWorker {
            cancelled: cancelled.clone(),
            cooldown: None,
            failures: 0,
            host,
            history: super::kanban::History::default(),
            mailbox: mailbox.clone(),
            moves: moves.clone(),
            notices: notices.clone(),
            opened: false,
            retry_until: None,
            request,
            snapshot,
            store: None,
            visible: visible.clone(),
        };
        let service = Self {
            moves,
            commands,
            mailbox,
            notices,
            cancelled,
            visible,
        };
        (service, worker, receiver)
    }
}

pub(super) struct AccountWorker {
    cancelled: Arc<AtomicBool>,
    cooldown: Option<Duration>,
    /// Failed syncs in a row, which lengthen the wait.
    failures: u32,
    host: String,
    history: super::kanban::History,
    mailbox: Arc<Mutex<Option<Snapshot>>>,
    moves: super::Moves,
    notices: Arc<Mutex<VecDeque<Notice>>>,
    /// Whether opening the database was tried; it is not tried again.
    opened: bool,
    request: RemoteRequest,
    /// A failed sync waits until then before the timer reads again.
    retry_until: Option<Instant>,
    snapshot: Snapshot,
    store: Option<Store>,
    visible: Arc<AtomicBool>,
}

impl AccountWorker {
    fn run(mut self, commands: &mpsc::Receiver<Command>) {
        self.publish();
        self.sync();
        self.sync_history();
        let Timing { reload, sync } = self.request.timing;
        let mut last_sync = Instant::now();
        while !self.cancelled.load(Ordering::Relaxed) {
            let visible = self.visible.load(Ordering::Relaxed);
            let next_sync = last_sync + schedule::period(sync, visible);
            let wait = reload.min(next_sync.saturating_duration_since(Instant::now()));
            match commands.recv_timeout(wait) {
                Ok(Command::Refresh(_)) => {
                    self.moves.cancel_unconfirmed();
                    self.retry_until = None;
                    self.history.refresh();
                    self.sync();
                    last_sync = Instant::now();
                }
                Ok(Command::Act(action)) => self.act(action),
                // Forgejo keeps no pasted token, and nothing here has a
                // GitHub pull request, agents, or actions.
                Ok(
                    Command::Stores(_)
                    | Command::Acted { .. }
                    | Command::LoadPullRequest(_)
                    | Command::LoadAgents { .. },
                ) => {}
                Err(RecvTimeoutError::Timeout) if Instant::now() >= next_sync => {
                    if self.retry_until.is_none_or(|until| Instant::now() >= until) {
                        self.sync();
                    }
                    last_sync = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if !self.moves.take_invalidated().is_empty() {
                self.sync();
            }
            self.sync_history();
            if self.moves.take_changed() {
                self.publish();
            }
        }
    }

    fn act(&self, _action: crate::actions::Action) {
        push(
            &self.notices,
            Notice {
                outcome: Err(Arc::new(Error::Forgejo(forgejo::Error::NoCheckout {
                    host: self.host.clone(),
                }))),
            },
        );
    }

    fn publish(&self) {
        if let Ok(mut mailbox) = self.mailbox.lock() {
            let mut snapshot = self.snapshot.clone();
            snapshot.moves = Arc::new(self.moves.snapshot());
            *mailbox = Some(snapshot);
        }
    }

    fn set_state(&mut self, state: SyncState) {
        for status in &mut self.snapshot.sources {
            status.state = state.clone();
        }
    }

    /// `FORGEJO_TOKEN`, when `FORGEJO_HOST` names this host. Looked up each
    /// sync: the login shell is probed once per run, so a change there
    /// takes a restart, as a checkout's Forgejo source's does. A
    /// `FORGEJO_HOST` that names a path is refused, and nothing is sent.
    fn token(host: &str, look_up: bool) -> Result<SecretString, forgejo::Error> {
        let token = if look_up {
            forgejo::account_token(host, sheprd_client::login_env::var)?
        } else {
            None
        };
        token.ok_or_else(|| forgejo::Error::SignIn {
            repository: host.to_owned(),
            host: host.to_owned(),
        })
    }

    /// Opens the account's database and shows what its last complete
    /// listing stored. Without one, listings are shown and not stored.
    pub(super) fn open(&mut self) {
        self.opened = true;
        let repository = Repository::ForgejoAccount {
            host: self.host.clone(),
        };
        let path = match &self.request.data_root {
            Some(root) => Some(database_path_in(root, &repository)),
            None => database_path(&repository),
        };
        let Some(path) = path else {
            tracing::warn!(
                category = "orchestrator",
                "No state directory for the Forgejo account's issues"
            );
            return;
        };
        let opened = Store::open(&path).and_then(|store| Ok((store.items()?, store)));
        match opened {
            Ok((items, store)) => {
                self.snapshot.items = Arc::new(items);
                self.snapshot.history_error = self.history.load(&store).err().map(Arc::new);
                self.store = Some(store);
            }
            Err(error) => tracing::warn!(
                category = "orchestrator",
                %error,
                "Cannot open the Forgejo account's database"
            ),
        }
    }

    pub(super) fn sync_history(&mut self) {
        let collection = crate::kanban::history::Collection::ForgejoAccount {
            host: self.host.clone(),
        };
        let changed = self.history.read(
            collection,
            super::kanban::ReadContext {
                active: &self.snapshot.items,
                blocked_until: self.retry_until,
                now: Instant::now(),
                observed_at: Utc::now(),
                store: self.store.as_mut(),
                visible: self.visible.load(Ordering::Relaxed),
            },
            |cooldown| {
                let token = Self::token(&self.host, self.request.look_up_token)?;
                forgejo::history::account(
                    &self.host,
                    &token,
                    forgejo::history::cutoff(Utc::now()),
                    &|| self.cancelled.load(Ordering::Relaxed),
                    cooldown,
                )
                .map(Some)
            },
        );
        if changed {
            self.history
                .reconcile(self.store.as_mut(), &self.snapshot.items, Utc::now());
            self.history.publish(&mut self.snapshot, Utc::now());
            self.publish();
        }
    }

    fn sync(&mut self) {
        let collection = crate::kanban::history::Collection::ForgejoAccount {
            host: self.host.clone(),
        };
        if self
            .history
            .provider_until(&collection)
            .is_some_and(|until| Instant::now() < until)
        {
            return;
        }
        let token = match Self::token(&self.host, self.request.look_up_token) {
            Ok(token) => token,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        if !self.opened {
            self.open();
        }
        for status in &mut self.snapshot.sources {
            status.state = SyncState::Syncing {
                count: status.state.count(),
            };
        }
        self.publish();
        self.cooldown = None;
        let started = Instant::now();
        let fences = self.moves.read_fences();
        let cancelled = &self.cancelled;
        let read = forgejo::search::sync(
            &self.host,
            &token,
            &|| cancelled.load(Ordering::Relaxed),
            &mut self.cooldown,
        );
        let state = match read.and_then(|listing| self.keep(listing)) {
            Ok(state) => {
                self.failures = 0;
                self.retry_until = None;
                tracing::debug!(
                    category = "orchestrator_sync",
                    host = self.host.as_str(),
                    elapsed_ms = started.elapsed().as_millis(),
                    state = ?state,
                    "Forgejo account synced"
                );
                state
            }
            Err(Error::Cancelled) => return,
            Err(error) => {
                self.failures = self.failures.saturating_add(1);
                let wait =
                    schedule::backoff(retry_wait(&error, self.cooldown.take()), self.failures);
                tracing::warn!(
                    category = "orchestrator_sync",
                    host = self.host.as_str(),
                    retry_in_secs = wait.as_secs(),
                    error = %error,
                    "Forgejo account sync failed"
                );
                self.retry_until = Some(Instant::now() + wait);
                SyncState::Failed(Arc::new(error))
            }
        };
        if matches!(state, SyncState::Synced { .. } | SyncState::Capped { .. }) {
            for fence in fences {
                self.moves.observe_items(&fence, &self.snapshot.items);
            }
        }
        self.set_state(state);
        self.history.publish(&mut self.snapshot, Utc::now());
        self.publish();
    }

    /// Shows that nothing could be read, without asking Forgejo.
    fn fail(&mut self, error: forgejo::Error) {
        self.set_state(SyncState::Failed(Arc::new(Error::Forgejo(error))));
        self.publish();
    }

    /// Shows `listing`, and stores it when it is complete and the database
    /// is writable. A capped one is shown and not written, so the last
    /// complete listing is what the next start shows.
    pub(super) fn keep(&mut self, listing: Listing) -> Result<SyncState> {
        let count = listing.items.len();
        let items = Arc::new(listing.items);
        self.snapshot.items = items.clone();
        self.history
            .reconcile(self.store.as_mut(), &items, Utc::now());
        if !listing.complete {
            return Ok(SyncState::Capped { count });
        }
        if let Some(store) = self
            .store
            .as_mut()
            .filter(|store| store.access() == Access::ReadWrite)
        {
            let source = forgejo::search::account_source(&self.host);
            store.replace_items(&source, &items, &listing.checkpoint)?;
        }
        Ok(SyncState::Synced {
            at: Utc::now(),
            count,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
