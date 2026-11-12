use super::config::{Config, Mappings, SourceOverride};
use super::facts::{LabelCompleteness, MoveCapability, NativeStateProvenance, WorkflowFacts};
use super::identity::{CardKey, LaneKey, Origin, Route};
use super::projection::{Board, CardInput, LaneInput, project};
use super::*;
use crate::{Item, ItemKey, Provider, SourceKey};

mod config;
mod history_store;
mod identity;
mod move_outcomes;
pub(crate) mod move_scope;
mod projection;

fn source(provider: Provider, host: &str, repository: &str) -> SourceKey {
    SourceKey {
        provider,
        host: host.into(),
        repository: repository.into(),
    }
}

fn issue(source: &SourceKey, id: &str, state: &str, labels: &[&str]) -> Item {
    let mut item = Item::stand_in(ItemKey {
        source: source.clone(),
        native_id: id.into(),
    });
    item.state = state.into();
    item.labels = labels.iter().map(|label| (*label).into()).collect();
    item
}

fn complete() -> WorkflowFacts {
    WorkflowFacts {
        labels: Some(LabelCompleteness::Complete),
        native_state: Some(NativeStateProvenance::Tracker),
        settled_at: None,
    }
}

fn checkout(source: &SourceKey, worker: u64) -> Route<u64> {
    Route {
        origin: Origin::Checkout(source.clone()),
        worker,
    }
}

fn cards(board: &Board<u64>) -> impl Iterator<Item = &super::projection::Card<u64>> {
    board
        .lanes
        .iter()
        .flat_map(|lane| &lane.cells)
        .flat_map(|cell| &cell.cards)
}
