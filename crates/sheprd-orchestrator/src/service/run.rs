//! The existing command and reload loop, shared with history scheduling.

use super::*;

impl Worker {
    pub(super) fn run(mut self, commands: mpsc::Receiver<Command>) {
        if let Err(error) = self.open() {
            self.snapshot.error = Some(Arc::new(error));
            self.publish();
            return;
        }
        // The host was just found unreachable: the next sync looks again.
        if !self.unreached {
            let token = self.request.token.clone();
            self.sync(token.as_ref());
        }
        self.sync_history();
        self.publish();
        let Timing { reload, sync } = self.request.timing;
        let mut last_sync = Instant::now();
        while !self.cancelled() {
            let visible = self.visible.load(Ordering::Relaxed);
            let next_sync = last_sync + schedule::period(sync, visible);
            let wait = reload.min(next_sync.saturating_duration_since(Instant::now()));
            match commands.recv_timeout(wait) {
                Ok(Command::Refresh(token)) => {
                    self.refreshed(token);
                    let token = self.request.token.clone();
                    self.sync(token.as_ref());
                    last_sync = Instant::now();
                }
                Ok(Command::Stores(stores)) => {
                    self.stores_changed(stores);
                    let token = self.request.token.clone();
                    self.sync(token.as_ref());
                    last_sync = Instant::now();
                }
                Ok(Command::Act(action)) => self.act(action),
                Ok(Command::LoadPullRequest(number)) => self.load_pull_request(number),
                Ok(Command::LoadAgents { endpoint, target }) => {
                    self.snapshot.installed_endpoint = endpoint;
                    self.snapshot.installed = None;
                    self.snapshot.installed_error = None;
                    self.publish();
                    let result = Host::new(&target)
                        .map_err(|_| Error::UnsupportedHost)
                        .and_then(|host| {
                            ssh_slots::run(&target, &self.cancelled, || {
                                installed(Some(&host), &self.cancelled)
                            })
                        });
                    match result {
                        Ok(agents) => self.snapshot.installed = Some(Arc::new(agents)),
                        Err(error) => self.snapshot.installed_error = Some(Arc::new(error)),
                    }
                    self.publish();
                }
                Ok(Command::Acted { resync }) => {
                    if resync {
                        let token = self.request.token.clone();
                        self.sync(token.as_ref());
                    } else {
                        self.reload_if_changed();
                    }
                }
                Err(RecvTimeoutError::Timeout) if Instant::now() >= next_sync => {
                    let token = self.request.token.clone();
                    self.sync(token.as_ref());
                    last_sync = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => self.reload_if_changed(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if !self.moves.take_invalidated().is_empty() {
                let token = self.request.token.clone();
                self.sync(token.as_ref());
            }
            if self.sync_history() | self.moves.take_changed() {
                self.publish();
            }
        }
    }
}
