//! Backend policy only; the GUI config loader is wired by the board view.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{Error, Result, identity::LaneKey};
use crate::SourceKey;

pub const MAX_LABEL_BYTES: usize = 128;
pub const MAX_SOURCE_OVERRIDES: usize = 128;

/// Raw `[kanban]` settings. Validate before using any mapping.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub in_progress_label: String,
    pub in_review_label: String,
    pub sources: Vec<SourceOverride>,
}

impl Default for Config {
    fn default() -> Self {
        let labels = LabelMap::default();
        Self {
            in_progress_label: labels.in_progress_label,
            in_review_label: labels.in_review_label,
            sources: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceOverride {
    pub in_progress_label: String,
    pub in_review_label: String,
    pub source: String,
}

/// Construction is private so projection cannot use an unvalidated label map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelMap {
    in_progress_label: String,
    in_review_label: String,
}

impl Default for LabelMap {
    fn default() -> Self {
        Self {
            in_progress_label: "status: in-progress".into(),
            in_review_label: "status: in-review".into(),
        }
    }
}

impl LabelMap {
    pub fn in_progress_label(&self) -> &str {
        &self.in_progress_label
    }

    pub fn in_review_label(&self) -> &str {
        &self.in_review_label
    }

    fn new(in_progress_label: String, in_review_label: String) -> Result<Self> {
        for name in [&in_progress_label, &in_review_label] {
            if name.trim().is_empty()
                || name.len() > MAX_LABEL_BYTES
                || name.chars().any(|c| c.is_control() || c == ',')
            {
                return Err(Error::Label {
                    max: MAX_LABEL_BYTES,
                });
            }
        }
        if in_progress_label.eq_ignore_ascii_case(&in_review_label) {
            return Err(Error::IdenticalLabels);
        }
        Ok(Self {
            in_progress_label,
            in_review_label,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mappings {
    defaults: LabelMap,
    sources: HashMap<SourceKey, LabelMap>,
}

impl TryFrom<Config> for Mappings {
    type Error = Error;

    fn try_from(config: Config) -> Result<Self> {
        if config.sources.len() > MAX_SOURCE_OVERRIDES {
            return Err(Error::TooManySources {
                max: MAX_SOURCE_OVERRIDES,
            });
        }
        let defaults = LabelMap::new(config.in_progress_label, config.in_review_label)?;
        let mut sources = HashMap::new();
        let mut lanes = HashSet::new();
        for entry in config.sources {
            let source: SourceKey = entry
                .source
                .parse()
                .map_err(|error| Error::Selector(Box::new(error)))?;
            if source.canonical() != entry.source {
                return Err(Error::SelectorCanonical);
            }
            if !lanes.insert(LaneKey::try_from(&source)?) {
                return Err(Error::DuplicateSource);
            }
            sources.insert(
                source,
                LabelMap::new(entry.in_progress_label, entry.in_review_label)?,
            );
        }
        Ok(Self { defaults, sources })
    }
}

impl Mappings {
    /// Overrides select the concrete canonical source, not a display name.
    pub fn for_source(&self, source: &SourceKey) -> Result<&LabelMap> {
        LaneKey::try_from(source)?;
        Ok(self.sources.get(source).unwrap_or(&self.defaults))
    }
}
