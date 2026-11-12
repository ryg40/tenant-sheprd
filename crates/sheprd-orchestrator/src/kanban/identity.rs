//! Tracker identity is separate from the concrete source and worker route.

use super::{Error, Result};
use crate::{ItemKey, Provider, SourceKey};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackerRepository {
    host: String,
    repository: String,
}

impl TrackerRepository {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }
}

/// Only tracker repositories supported by the board can be lanes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LaneKey {
    // keep-sorted start
    Forgejo(TrackerRepository),
    Github(TrackerRepository),
    // keep-sorted end
}

impl TryFrom<&SourceKey> for LaneKey {
    type Error = Error;

    fn try_from(source: &SourceKey) -> Result<Self> {
        if !matches!(source.provider, Provider::Github | Provider::Forgejo) {
            return Err(Error::UnsupportedProvider(source.provider));
        }
        if source.host.is_empty()
            || source
                .host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == ',')
            || source.host.contains('/')
            || source.repository.split('/').count() < 2
            || source
                .repository
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || source
                .repository
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == ',')
        {
            return Err(Error::Repository);
        }
        // Matches Forgejo's account comparison and GitHub's private
        // pull_request::remote::same_repository rule, not persisted ItemKey spelling.
        let normalized = normalized_source(source);
        let repository = TrackerRepository {
            host: normalized.host,
            repository: normalized.repository,
        };
        if source.provider == Provider::Forgejo {
            Ok(Self::Forgejo(repository))
        } else {
            Ok(Self::Github(repository))
        }
    }
}

pub(super) fn normalized_source(source: &SourceKey) -> SourceKey {
    SourceKey {
        provider: source.provider,
        host: source.host.to_ascii_lowercase(),
        repository: source.repository.to_ascii_lowercase(),
    }
}

impl LaneKey {
    pub fn source(&self) -> SourceKey {
        let (provider, repository) = match self {
            Self::Forgejo(repository) => (Provider::Forgejo, repository),
            Self::Github(repository) => (Provider::Github, repository),
        };
        SourceKey {
            provider,
            host: repository.host.clone(),
            repository: repository.repository.clone(),
        }
    }

    pub fn canonical(&self) -> String {
        self.source().canonical()
    }
}

/// Keep the selected record's full original key; no stored key is migrated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CardKey {
    pub item: ItemKey,
    pub lane: LaneKey,
}

impl TryFrom<&ItemKey> for CardKey {
    type Error = Error;

    fn try_from(item: &ItemKey) -> Result<Self> {
        Ok(Self {
            item: item.clone(),
            lane: LaneKey::try_from(&item.source)?,
        })
    }
}

/// The caller owns the stable worker handle and its generation in `worker`.
/// Never replace that handle with a display row or lane index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route<R> {
    pub origin: Origin,
    pub worker: R,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    // keep-sorted start
    Checkout(SourceKey),
    ForgejoAccount { host: String },
    // keep-sorted end
}

impl<R> Route<R> {
    pub fn validate(&self, source: &SourceKey) -> Result<()> {
        let lane = LaneKey::try_from(source)?;
        let matches = match &self.origin {
            Origin::Checkout(held) => LaneKey::try_from(held)? == lane,
            Origin::ForgejoAccount { host } => {
                source.provider == Provider::Forgejo && source.host.eq_ignore_ascii_case(host)
            }
        };
        if matches { Ok(()) } else { Err(Error::Route) }
    }

    pub(super) fn is_checkout(&self) -> bool {
        matches!(self.origin, Origin::Checkout(_))
    }
}
