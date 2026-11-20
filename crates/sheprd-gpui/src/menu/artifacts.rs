//! Asks before `sheprd artifacts start` or `off` starts, restarts, or stops
//! the local artifact server. Those commands open this question and nothing
//! else; only a yes here runs `artifacts start` or `artifacts off` for them.
//! A click on the Artifacts button starts or restarts the server without it
//! (`artifacts/launch.rs`). When another program holds the server's port,
//! the Ports question asks to stop it, and its yes goes on to the start.

// i18n: migrated
use super::Page;
use crate::{
    SheprdWindow,
    artifacts::{Ask, Freeing, NotAsked, Question},
    fonts::StyledFont,
    i18n::fl,
    local_ports::Pending,
};
use gpui::{prelude::*, *};
use std::path::PathBuf;

impl SheprdWindow {
    /// Asks to start the server whose page is `url` with `program`, the
    /// file that answered the status.
    pub(crate) fn ask_to_start_artifacts(
        &mut self,
        url: crate::browser::WebUrl,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        self.ask_artifacts(Ask::Start(url), program, window, cx)
    }

    /// Asks to stop the server whose page is `url` and does not answer,
    /// then start it, with `program`, the file that answered the status.
    pub(crate) fn ask_to_restart_artifacts(
        &mut self,
        url: crate::browser::WebUrl,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        self.ask_artifacts(Ask::Restart(url), program, window, cx)
    }

    /// Shows the Ports question for `pending`, the processes that hold the
    /// artifact server's port; its yes stops them and then runs `start`.
    /// Whether it is shown.
    pub(crate) fn ask_to_free_artifacts_port(
        &mut self,
        pending: Pending,
        start: Freeing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Opening the question resets the menu, so the start comes after.
        if !self.ask_to_signal_ports(pending, window, cx) {
            return false;
        }
        self.menu.port_signal_start = Some(start);
        true
    }

    /// Asks to stop each local server with `program`, the file that
    /// answered the status.
    pub(crate) fn ask_to_stop_artifacts(
        &mut self,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        self.ask_artifacts(Ask::Stop, program, window, cx)
    }

    /// Opens the question for `program`, unless the app's program is
    /// another file now or none, as after a config reload: the question
    /// names the file that answered the status, and its yes runs that file.
    fn ask_artifacts(
        &mut self,
        ask: Ask,
        program: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        if crate::artifacts::program_path(cx).as_ref() != Some(&program) {
            return Err(NotAsked::NoProgram);
        }
        if self.menu.page.is_some() || !self.open_menu(window, cx) {
            return Err(NotAsked::DialogOpen);
        }
        let home = sheprd_client::local_path::home_dir();
        let shown = crate::local_ports::home_relative(&program, home.as_deref());
        self.menu.artifacts = Some(Question {
            ask,
            program,
            shown,
        });
        self.menu.page = Some(Page::Artifacts);
        cx.notify();
        Ok(())
    }

    /// The user said yes: closes the question, then runs the program.
    pub(in crate::menu) fn confirm_artifacts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let question = self.menu.artifacts.take();
        self.dismiss_menu(window, cx);
        let Some(Question { ask, program, .. }) = question else {
            return;
        };
        match ask {
            Ask::Start(url) => self.start_artifacts(url, program, window, cx),
            Ask::Restart(url) => self.restart_artifacts(url, program, window, cx),
            Ask::Stop => self.stop_artifacts(program, window, cx),
        }
    }

    pub(in crate::menu) fn render_artifacts(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let Some(Question { ask, shown, .. }) = &self.menu.artifacts else {
            return div();
        };
        let program = shown.as_str();
        let (title, note, yes) = match ask {
            Ask::Start(_) => (
                fl!("artifacts-start-title"),
                fl!("artifacts-start-note", program = program),
                fl!("artifacts-start-yes"),
            ),
            Ask::Restart(_) => (
                fl!("artifacts-restart-title"),
                fl!("artifacts-restart-note", program = program),
                fl!("artifacts-restart-yes"),
            ),
            Ask::Stop => (
                fl!("artifacts-stop-title"),
                fl!("artifacts-stop-note", program = program),
                fl!("artifacts-stop-yes"),
            ),
        };
        let button = |id: &'static str, label: String, primary: bool| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .p(px(8.))
                .rounded(px(crate::config::corners::CONTROL))
                .when(primary, |button| button.bg(rgb(theme.active)))
                .when(!primary, |button| button.hover(|s| s.bg(rgb(theme.active))))
                .cursor_pointer()
                .child(label)
        };
        let address = match ask {
            Ask::Start(url) | Ask::Restart(url) => Some(url.as_str().to_owned()),
            Ask::Stop => None,
        };
        div()
            .debug_selector(|| "artifacts-ask".into())
            .child(
                div()
                    .p(px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .children(address.map(|address| {
                div().px(px(8.)).child(
                    div()
                        .debug_selector(|| "artifacts-ask-url".into())
                        .rounded(px(crate::config::corners::CONTROL))
                        .bg(rgb(theme.active))
                        .px(px(10.))
                        .py(px(6.))
                        .overflow_hidden()
                        .text_font(&self.config.terminal)
                        .child(address),
                )
            }))
            .child(div().p(px(8.)).text_color(rgb(theme.muted)).child(note))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .p(px(8.))
                    .child(button("artifacts-ask-yes", yes, true).on_click(cx.listener(
                        |this, _, window, cx| {
                            cx.stop_propagation();
                            this.confirm_artifacts(window, cx);
                        },
                    )))
                    .child(
                        button("artifacts-ask-no", fl!("artifacts-no"), false).on_click(
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.dismiss_menu(window, cx);
                            }),
                        ),
                    ),
            )
    }
}
