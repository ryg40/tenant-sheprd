//! The face the sidebar draws its text in: `[sidebar]`, or the `[ui]` face
//! in a layout that reads as a desktop app's sidebar. The tab strips take
//! their face by the same rule ([`super::tabs_font`]), through
//! [`FontKeys::face`].

use super::{Config, FontConfig};
use std::borrow::Cow;

/// The size sidebar and tab text take in the `[ui]` face: a desktop app's
/// list rows and tabs, a step above the 12 px its menus and status bar
/// default to.
const UI_FACE_SIZE: f32 = 13.;

/// Which font keys of one section, `[sidebar]` or `[tabs]`, the user's own
/// file sets. The managed defaults under it set `size` too, so the merged
/// table cannot tell a choice from a default; a layout that draws in
/// another face yields only to a choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontKeys {
    pub fallback: bool,
    pub family: bool,
    pub size: bool,
}

impl FontKeys {
    /// The `[sidebar]` font keys `text`, the user's overrides file, sets.
    pub(super) fn set_in(text: Option<&str>) -> Self {
        Self::of(user_table(text).as_ref(), "sidebar")
    }

    /// The font keys `table`, the user's parsed file, sets under `section`.
    pub(super) fn of(table: Option<&toml::Table>, section: &str) -> Self {
        let section = table
            .and_then(|table| table.get(section))
            .and_then(toml::Value::as_table);
        let set = |key| section.is_some_and(|section| section.contains_key(key));
        Self {
            fallback: set("fallback"),
            family: set("family"),
            size: set("size"),
        }
    }

    /// `own`, the face of the section these keys came from, as a layout
    /// that reads as a desktop app draws it: the `[ui]` face `ui` at 13 px,
    /// except that a family, size, or fallback the user set in the section
    /// still wins. Borrowed when the section's face applies whole.
    pub(super) fn face<'a>(self, own: &'a FontConfig, ui: &FontConfig) -> Cow<'a, FontConfig> {
        if self.family && self.size {
            return Cow::Borrowed(own);
        }
        let mut font = if self.family { own } else { ui }.clone();
        font.size = if self.size { own.size } else { UI_FACE_SIZE };
        // The section's family already brought its own fallbacks.
        if self.fallback && !self.family {
            font.fallbacks.clone_from(&own.fallbacks);
        }
        Cow::Owned(font)
    }
}

/// `text`, the user's overrides file, as a table. Text that does not parse
/// sets no key; loading reports it on its own.
pub(super) fn user_table(text: Option<&str>) -> Option<toml::Table> {
    text.and_then(|text| text.parse::<toml::Table>().ok())
}

impl Config {
    /// The face sidebar text draws in: the sidebar's rows and headings, its
    /// rail, its activity bar, and its sections. `[sidebar]` in most
    /// layouts; in one that reads as a desktop app
    /// ([`super::LayoutMode::desktop_chrome`]), the `[ui]` family at 13 px,
    /// unless the user's file sets `[sidebar] family`, `size`, or
    /// `fallback`, which still win. Borrowed whenever `[sidebar]` applies
    /// whole.
    pub(crate) fn sidebar_font(&self) -> Cow<'_, FontConfig> {
        if !self.layout.mode.desktop_chrome() {
            return Cow::Borrowed(&self.sidebar);
        }
        self.sidebar_font_keys.face(&self.sidebar, &self.ui)
    }
}
