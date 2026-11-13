//! Reading a Forgejo source, with the login shell's `FORGEJO_TOKEN` when
//! `FORGEJO_HOST` names the source's host, else anonymously.

use super::{Result, SourceKey, Worker, forgejo};
use crate::{
    Checkpoint,
    forge::{Listing, Scheme},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) fn history(
    repository: (&SourceKey, Scheme),
    cancelled: &AtomicBool,
    cooldown: &mut Option<Duration>,
) -> Result<crate::kanban::history::Read> {
    let token = forgejo::environment_token(&repository.0.host, sheprd_client::login_env::var);
    forgejo::history::repository(
        repository,
        token.as_ref(),
        forgejo::history::cutoff(chrono::Utc::now()),
        &|| cancelled.load(Ordering::Relaxed),
        cooldown,
    )
}

impl super::Moves {
    /// Resolve the credential and connect only on bounded action threads.
    pub fn prepare_forgejo_dialog(
        &self,
        proposal: crate::kanban::moves::Proposal,
        context: crate::kanban::moves::Context,
        scheme: Scheme,
        observed: crate::kanban::history::Record,
    ) -> Result<super::kanban::moves::MoveTicket> {
        self.configure_dialog(proposal.card.lane.clone(), context.clone())?;
        let card = proposal.card.clone();
        self.prepare_session(proposal, observed, move |cancelled| {
            if cancelled.load(Ordering::Acquire) {
                return Err(crate::Error::Cancelled);
            }
            let session =
                forgejo::moves::Session::new(card, context, scheme, sheprd_client::login_env::var)?;
            let (session, fresh) = session.connect()?;
            Ok((fresh, Some(session)))
        })
    }

    /// Use only the session retained by this preparation; callers cannot replace it.
    pub fn submit_forgejo_dialog(
        &self,
        confirmed: crate::kanban::moves::ConfirmedMove,
    ) -> Result<()> {
        let session = self.take_session(confirmed.plan())?;
        self.submit_forgejo(confirmed, session)
    }

    /// The connected session carries fresh account evidence, never cached permission.
    /// This prepares only; the returned ticket must still be confirmed.
    pub fn prepare_forgejo(
        &self,
        proposal: crate::kanban::moves::Proposal,
        session: forgejo::moves::Session,
        observed: crate::kanban::history::Record,
    ) -> Result<super::kanban::moves::MoveTicket> {
        if &proposal.card != session.card() {
            return Err(crate::kanban::Error::Route.into());
        }
        self.configure(proposal.card.lane.clone(), session.context().clone())?;
        self.prepare(proposal, observed, move |cancelled| {
            if cancelled.load(Ordering::Relaxed) {
                return Err(crate::Error::Cancelled);
            }
            session.read()
        })
    }

    /// A raw proposal cannot call the writer: this consumes opaque one-shot approval.
    pub fn submit_forgejo(
        &self,
        confirmed: crate::kanban::moves::ConfirmedMove,
        session: forgejo::moves::Session,
    ) -> Result<()> {
        let writer = session.clone();
        let mut revalidate = true;
        self.submit(
            confirmed,
            move || {
                if std::mem::take(&mut revalidate) {
                    session.read()
                } else {
                    session.observe()
                }
            },
            move |plan, mutation| writer.mutate(plan, mutation),
        )
    }
}

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
