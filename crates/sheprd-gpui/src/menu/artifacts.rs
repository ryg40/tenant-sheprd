//! Asks before Sheprd starts or stops the local artifact server. Only a yes
//! here runs `artifacts start` or `artifacts off`; the Artifacts button and
//! `sheprd artifacts start|off` open this question and nothing else.

// i18n: migrated
use super::Page;
use crate::{
    SheprdWindow,
    artifacts::{Ask, NotAsked, Question},
    fonts::StyledFont,
    i18n::fl,
};
use gpui::{prelude::*, *};

impl SheprdWindow {
    /// Asks to start the server whose page is `url`.
    pub(crate) fn ask_to_start_artifacts(
        &mut self,
        url: crate::browser::WebUrl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        self.ask_artifacts(Ask::Start(url), window, cx)
    }

    /// Asks to stop each local server.
    pub(crate) fn ask_to_stop_artifacts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        self.ask_artifacts(Ask::Stop, window, cx)
    }

    fn ask_artifacts(
        &mut self,
        ask: Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NotAsked> {
        let program = crate::artifacts::program_path(cx).ok_or(NotAsked::NoProgram)?;
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
            Ask::Start(url) => Some(url.as_str().to_owned()),
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
