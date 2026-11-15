//! The artifacts page address on the Sidebar page. It puts an Artifacts
//! button on the activity bar, which the user's `artifacts` program also
//! does without it (`crate::artifacts`). As with px0's address, an edit stays in the
//! field until Apply, and saving goes through the window's serial
//! persistence path.
// i18n: migrated
use super::*;
use crate::{browser::WebUrl, i18n::fl};

#[derive(Default)]
pub(in crate::settings_window) struct ArtifactsSettings {
    input: Option<Entity<SearchInput>>,
    shown: String,
    invalid: bool,
    /// Exact submitted text, including whitespace, until its save completes.
    saving: Option<String>,
}

impl SettingsWindow {
    pub(in crate::settings_window) fn open_artifacts_settings(&mut self, cx: &mut Context<Self>) {
        if self.artifacts.input.is_none() {
            self.artifacts.input = Some(cx.new(SearchInput::new));
        }
        self.sync_artifacts_field(cx);
    }

    pub(in crate::settings_window) fn sync_artifacts_field(&mut self, cx: &mut Context<Self>) {
        let Some(input) = &self.artifacts.input else {
            return;
        };
        let saved = self
            .config
            .artifacts
            .url
            .as_ref()
            .map_or("", WebUrl::as_str);
        if !self.artifacts.invalid
            && !input.read(cx).is_composing()
            && input.read(cx).text() == self.artifacts.shown
        {
            input.update(cx, |input, cx| input.set_text_selected(saved, cx));
            self.artifacts.shown = saved.to_owned();
        }
        input.update(cx, |input, cx| {
            input.set_placeholder_with(
                || {
                    fl!(
                        "settings-sidebar-artifacts-placeholder",
                        example = "http://127.0.0.1:8080/"
                    )
                },
                cx,
            );
            input.set_appearance(self.config.ui.clone(), self.theme.clone(), cx);
        });
    }

    fn apply_artifacts_address(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let Some(input) = &self.artifacts.input else {
            return;
        };
        if input.read(cx).is_composing() {
            return;
        }
        let text = input.read(cx).text().trim().to_owned();
        let url = if text.is_empty() {
            Some(None)
        } else {
            WebUrl::from_typed(&text).ok().map(Some)
        };
        let Some(url) = url else {
            self.artifacts.invalid = true;
            cx.notify();
            return;
        };
        self.artifacts.invalid = false;
        self.artifacts.saving = Some(input.read(cx).text().to_owned());
        // Keep a failed write as an edit, rather than replacing it on reload.
        self.save_native(
            move || match url {
                Some(url) => Config::set_key("artifacts.url", url.as_str().into()),
                None => Config::unset_key("artifacts.url").map(drop),
            },
            cx,
        );
    }

    /// A successful write acknowledges only the submitted text. A newer edit
    /// or composition stays a draft; failures never advance the baseline.
    pub(in crate::settings_window) fn finish_artifacts_save(&mut self, saved: bool, cx: &App) {
        let Some(submitted) = self.artifacts.saving.take() else {
            return;
        };
        let Some(input) = &self.artifacts.input else {
            return;
        };
        if saved && !input.read(cx).is_composing() && input.read(cx).text() == submitted {
            self.artifacts.shown = submitted;
        }
    }

    pub(in crate::settings_window) fn render_artifacts_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Div {
        self.control_card(&fl!("settings-sidebar-artifacts"))
            .child(self.control_note(fl!("settings-sidebar-artifacts-note")))
            .children(self.artifacts.input.as_ref().map(|input| {
                div()
                    .debug_selector(|| "settings-artifacts-url".into())
                    .child(input.clone())
            }))
            .when(self.artifacts.invalid, |card| {
                card.child(self.control_note(fl!("settings-sidebar-artifacts-invalid")))
            })
            .child(
                self.control_choice(
                    "settings-artifacts-apply",
                    fl!("settings-sidebar-artifacts-apply"),
                    false,
                    !self.busy(),
                )
                .role(Role::Button)
                .aria_label(fl!("settings-sidebar-artifacts-apply"))
                .when(!self.busy(), |button| {
                    button.on_click(cx.listener(|this, _, _, cx| this.apply_artifacts_address(cx)))
                }),
            )
    }
}
