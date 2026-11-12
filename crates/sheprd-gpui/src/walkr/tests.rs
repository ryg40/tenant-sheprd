#![allow(clippy::unwrap_used)]
use super::probe::{Page, Reach, classify, connectivity};
use super::*;

mod bounds;
mod history_detail;
mod history_list;
mod history_live;
mod history_root;
#[cfg(embedded_pages)]
mod page;
mod paths;
mod probe;
mod run;
mod section;
mod state;

impl Walkr {
    /// The section goes to `shown`, as a tick sees it, without a probe.
    #[cfg(embedded_pages)]
    pub(crate) fn test_show(&mut self, shown: bool, now: Instant) {
        self.show(shown, now);
    }

    pub(crate) fn test_connectivity(&mut self, config: &WalkrConfig, state: Connectivity) {
        let reach = match state {
            Connectivity::Reachable => Some(Reach::Answers),
            Connectivity::NetworkRefused => Some(Reach::NetworkRefused),
            Connectivity::Down { status } => Some(Reach::Down(status)),
            Connectivity::Unexpected { status } => Some(Reach::Unexpected(status)),
            Connectivity::Unreachable { cause } => Some(Reach::Unreachable(cause)),
            _ => None,
        };
        self.test_reach(config, reach);
    }

    fn test_reach(&mut self, config: &WalkrConfig, reach: Option<Reach>) {
        self.config = config.clone();
        self.last = reach.map(|reach| Update {
            generation: self.generation,
            reach,
            checked: Instant::now(),
        });
    }
}
