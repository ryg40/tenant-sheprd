//! Bounded board reads and native dialog proposals. No tracker approval is exposed.

use serde::{Deserialize, Serialize};

pub const MAX_PAGE_BYTES: usize = 192 * 1024;
pub const MAX_PAGE_CARDS: usize = 100;
pub const MAX_CURSOR_BYTES: usize = 80;
/// The most repository keys a selection or a `repos set` request holds.
pub const MAX_REPO_KEYS: usize = 512;
/// The longest repository key, in UTF-8 bytes.
pub const MAX_REPO_KEY_BYTES: usize = 1024;
/// The most key bytes one `repos set` request carries, under the transport's ceiling.
pub const MAX_REPO_REQUEST_BYTES: usize = 192 * 1024;
/// Items one `board rank` names: a lane ranks at most this many.
pub const MAX_RANK_ITEMS: usize = 100;

/// Fixed display order, independent of tracker and agent states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoardColumn {
    Open,
    InProgress,
    InReview,
    Done,
}

impl BoardColumn {
    pub const ALL: &[Self] = &[Self::Open, Self::InProgress, Self::InReview, Self::Done];
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum OrchestratorOp {
    Board {
        cursor: Option<String>,
    },
    BoardOrder {
        lane: String,
        before: Option<String>,
    },
    /// Opens native confirmation only. No wire operation can approve a move.
    BoardMove {
        item: String,
        lane: String,
        to: BoardColumn,
    },
    /// Replaces a lane's local order of implementation; no tracker write.
    BoardRank {
        lane: String,
        items: Vec<String>,
    },
    /// The listed repositories, and which the sections show.
    Repos,
    /// Shows only `keys`, or every repository (listed later too) for `None`.
    ReposSet {
        keys: Option<Vec<String>>,
    },
}

/// Whether `keys` fits a `repos set` request.
pub fn repo_keys_bounded(keys: &[String]) -> bool {
    keys.len() <= MAX_REPO_KEYS
        && keys
            .iter()
            .all(|key| !key.is_empty() && key.len() <= MAX_REPO_KEY_BYTES)
        && keys.iter().map(String::len).sum::<usize>() <= MAX_REPO_REQUEST_BYTES
}

/// One repository the orchestrator lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub key: String,
    pub name: String,
    pub host: String,
    pub active: bool,
    /// Listed since the selection was saved, and not picked.
    pub new: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repos {
    /// Whether a selection is saved; without one every repository is active.
    pub selected: bool,
    pub repos: Vec<Repo>,
    /// More repositories are listed than one response carries.
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub displayed: usize,
    pub loaded: usize,
    pub matching: usize,
    pub tracker_total: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub in_progress_label: String,
    pub in_review_label: String,
    pub source: String,
}

/// Safe classifications only; provider errors and worker requests never leave the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    pub source: String,
    pub state: String,
    pub last_success: Option<String>,
}

/// History bounds describe a sample by update time, not a complete closure archive.
/// Account-wide limits apply to every lane contributed by that collection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCoverage {
    pub collection: String,
    pub collection_loaded: usize,
    pub cutoff: Option<String>,
    pub failed: bool,
    pub last_success: Option<String>,
    pub limit: usize,
    pub loaded: usize,
    pub origin: Option<String>,
    pub scope: String,
    pub supported: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    pub key: String,
    pub position: usize,
    pub cells: Vec<Coverage>,
    pub mappings: Vec<Mapping>,
    pub active_sources: Vec<Freshness>,
    pub history: Vec<HistoryCoverage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub key: String,
    pub lane: String,
    pub identifier: String,
    pub title: String,
    pub column: BoardColumn,
    pub native_state: String,
    pub native_state_provenance: Option<String>,
    pub label_completeness: Option<String>,
    pub warnings: Vec<String>,
    /// A proposal can prepare native confirmation; this never proves permission.
    pub can_move: bool,
    pub read_only_reason: String,
    /// The card's place in its lane's order of implementation, counted from
    /// 1 over the lane's ranked cards that are not in Done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoardReply {
    Pending,
    RestartRequired,
    Page(Box<BoardPage>),
    Repos(Repos),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardPage {
    pub revision: u64,
    pub columns: Vec<BoardColumn>,
    pub roster: Coverage,
    pub coverage: Coverage,
    /// Only this page's lane metadata, not the whole roster on every page.
    pub lanes: Vec<Lane>,
    pub cards: Vec<Card>,
    pub next_cursor: Option<String>,
    /// At least one prepared lane has a history reader; coverage remains per lane.
    pub expanded_done_history: bool,
}
