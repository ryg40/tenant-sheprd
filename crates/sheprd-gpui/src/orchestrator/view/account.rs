//! A Forgejo account's open issues beyond the open checkouts. When
//! `FORGEJO_HOST` names a host and `FORGEJO_TOKEN` holds a token for it, the
//! view lists one more project: every open issue of every repository that
//! token reads, from a worker that needs no checkout, which the checkouts'
//! cap never counts. An issue of a repository a checkout lists is that
//! checkout's row, never the account's, so it shows once and keeps its
//! actions; the account's own rows are named and picked by their repository
//! (`projects`), and offer none. Without the setting or a token, nothing is
//! added.

use super::{
    OrchestratorView,
    repo::{Repo, remote_key},
};
use crate::orchestrator::{Provider, RemoteScope, SourceKey, forgejo};
use gpui::Context;
use std::collections::HashSet;

/// Forgejo repositories as [`held`] names them.
pub(super) type Held = HashSet<(String, String)>;

impl OrchestratorView {
    /// Looks for the Forgejo account off the UI thread, since reading the
    /// login shell's environment blocks the first time, and lists it once
    /// found.
    pub(super) fn find_forgejo_account(cx: &mut Context<Self>) {
        let found = cx
            .background_executor()
            .spawn(async move { forgejo::account_host() });
        cx.spawn(async move |this, cx| {
            let Some(host) = found.await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                view.add_forgejo_account(host);
                cx.notify();
            });
        })
        .detach();
    }

    /// Lists every open issue the token for `host` reads, once.
    pub(super) fn add_forgejo_account(&mut self, host: String) {
        let scope = RemoteScope::ForgejoAccount { host };
        let key = remote_key(&scope);
        if self.repos.iter().any(|repo| repo.key == key) {
            return;
        }
        self.repos.push(Repo::start_remote(scope, self.stores()));
        self.refresh_rows();
    }

    /// The Forgejo repositories the checkouts list, which an account leaves
    /// to them.
    pub(super) fn checkout_repositories(&self) -> Held {
        self.repos
            .iter()
            .filter(|repo| repo.remote.is_none())
            .flat_map(|repo| &repo.snapshot.sources)
            .filter(|status| status.key.provider == Provider::Forgejo)
            .map(|status| held(&status.key))
            .collect()
    }
}

/// How `source` is found among [`Held`] repositories: Forgejo matches hosts
/// and names without regard to case, and a remote's path keeps the case it
/// was cloned with.
pub(super) fn held(source: &SourceKey) -> (String, String) {
    (
        source.host.to_ascii_lowercase(),
        source.repository.to_ascii_lowercase(),
    )
}
