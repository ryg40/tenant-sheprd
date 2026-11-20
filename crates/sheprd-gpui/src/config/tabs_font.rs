//! The face and the tab layout tab strips draw in: `[tabs]` and
//! `tab_layout`, or the `[ui]` face and pills in a layout that reads as a
//! desktop app. The twin of [`super::sidebar_font`].

use super::{
    Config, FontConfig, TabLayoutMode,
    sidebar_font::{self, FontKeys},
};
use std::borrow::Cow;

/// Which tab keys the user's own file sets. The managed defaults under it
/// set `[tabs] size` and `tab_layout` too, so the merged table cannot tell
/// a choice from a default; the desktop layout yields only to a choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabKeys {
    /// `[tabs] family`, `size`, and `fallback`.
    pub font: FontKeys,
    /// `tab_layout`, or a pick from View > Tab Layout since the last load.
    pub layout: bool,
}

impl TabKeys {
    /// The tab keys `text`, the user's overrides file, sets.
    pub(super) fn set_in(text: Option<&str>) -> Self {
        let table = sidebar_font::user_table(text);
        Self {
            font: FontKeys::of(table.as_ref(), "tabs"),
            layout: table
                .as_ref()
                .is_some_and(|table| table.contains_key("tab_layout")),
        }
    }
}

impl Config {
    /// The face tab text draws in: the strip's tabs, their drag card, and
    /// the panes of a split tab. `[tabs]` in most layouts; in one that
    /// reads as a desktop app ([`super::LayoutMode::desktop_chrome`]), the
    /// `[ui]` family at 13 px, unless the user's file sets `[tabs] family`,
    /// `size`, or `fallback`, which still win. Borrowed whenever `[tabs]`
    /// applies whole.
    pub(crate) fn tabs_font(&self) -> Cow<'_, FontConfig> {
        if !self.layout.mode.desktop_chrome() {
            return Cow::Borrowed(&self.tabs);
        }
        self.tab_keys.font.face(&self.tabs, &self.ui)
    }

    /// The tab layout strips draw: `tab_layout`, except that the desktop
    /// layout draws pills while the user has not chosen one.
    pub(crate) fn tab_layout_in_use(&self) -> TabLayoutMode {
        if self.layout.mode.desktop_chrome() && !self.tab_keys.layout {
            return TabLayoutMode::Pills;
        }
        self.tab_layout
    }

    /// Takes `mode` as the user's choice, which the desktop layout then
    /// keeps too, as it keeps one the file names.
    pub(crate) fn choose_tab_layout(&mut self, mode: TabLayoutMode) {
        self.tab_layout = mode;
        self.tab_keys.layout = true;
    }
}
