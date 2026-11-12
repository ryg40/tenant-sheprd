//! The state labels share one grid cell so only their opacity changes.
use super::{Connectivity, section::title};
use crate::motion::{self, Toggle};
use gpui::{prelude::*, *};

/// What the header names. A page on another origin has no state of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Label {
    State(Connectivity),
    OtherPage(SharedString),
    /// A pane holds the run and its server answers, or is being asked.
    Live,
    /// No pane holds the run; its page stays for a rating.
    Ended,
    /// The server of the run does not answer.
    NoAnswer,
}

impl Label {
    /// Whether the label itself names a host, so the header adds none.
    pub fn names_host(&self) -> bool {
        !matches!(self, Self::State(state) if *state != Connectivity::Ready)
    }
}

#[derive(Debug, Default)]
pub(super) struct Header {
    layers: Vec<(Label, Toggle)>,
    revision: usize,
}

impl Header {
    pub fn update(&mut self, state: Label) {
        let now = motion::now();
        self.layers
            .retain(|(_, fade)| fade.is_open() || fade.moving(now));
        if self
            .layers
            .iter()
            .any(|(known, fade)| *known == state && fade.is_open())
        {
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        let first = self.layers.is_empty();
        if !self.layers.iter().any(|(known, _)| *known == state) {
            // Keep at most two outgoing labels during rapid settings changes.
            if self.layers.len() == 3 {
                self.layers.remove(0);
            }
            self.layers.push((state.clone(), Toggle::default()));
        }
        for (known, fade) in &mut self.layers {
            fade.set(*known == state, now, first);
        }
    }

    pub fn render(&self, state: &Label, host: &str) -> AnyElement {
        let now = motion::now();
        if !self.layers.iter().any(|(_, fade)| fade.moving(now)) {
            return div().child(title(state, host)).into_any_element();
        }
        let layers = self.layers.clone();
        let host = host.to_owned();
        div()
            .grid()
            .grid_cols(1)
            .with_animation(
                ("walkr-state-fade", self.revision),
                Animation::new(motion::duration(motion::ENTER.max(motion::LEAVE))),
                move |mut element, progress| {
                    for (state, fade) in &layers {
                        let opacity = if progress >= 1. {
                            if fade.is_open() { 1. } else { 0. }
                        } else {
                            fade.shown(motion::now())
                        };
                        element = element.child(
                            div()
                                .col_start(1)
                                .row_start(1)
                                .opacity(opacity)
                                .child(title(state, &host)),
                        );
                    }
                    element
                },
            )
            .into_any_element()
    }
}
