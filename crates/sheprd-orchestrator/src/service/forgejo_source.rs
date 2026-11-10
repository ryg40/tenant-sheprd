//! Reading a Forgejo source, with the login shell's `FORGEJO_TOKEN` when
//! `FORGEJO_HOST` names the source's host, else anonymously.

use super::{Result, SourceKey, Worker, forgejo};
use crate::{
    Checkpoint,
    forge::{Listing, Scheme},
};
use std::sync::atomic::Ordering;

impl Worker {
    /// Lists Forgejo source `key`, continuing from `previous`.
    pub(super) fn forgejo_listing(
        &mut self,
        key: &SourceKey,
        previous: Option<&Checkpoint>,
    ) -> Result<Listing> {
        let scheme = Scheme::of_source(self.snapshot.repo.as_ref(), key);
        // Looked up each sync: the login shell is probed once per run, so a
        // change there takes a restart, as `AZURE_DEVOPS_EXT_PAT`'s does.
        let token = forgejo::environment_token(&key.host, sheprd_client::login_env::var);
        let cancelled = &self.cancelled;
        forgejo::sync(
            (key, scheme),
            token.as_ref(),
            previous,
            &|| cancelled.load(Ordering::Relaxed),
            &mut self.cooldown,
        )
    }
}
