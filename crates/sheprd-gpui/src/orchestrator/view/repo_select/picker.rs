//! The Repositories control above every section's list or board, and its
//! popover: a filter field, All and None, and one checkbox row per listed
//! repository with its host. The rows are a virtual list over the prepared
//! roster, so hundreds draw as cheaply as a few. P, or Enter or Space on the
//! focused control, opens it. Up and Down move the focused row, Enter (or
//! Space outside the field) flips it, Tab moves between the field, All, and
//! None, and Escape closes the popover. While it is open its keys reach
//! nothing else, and keys an input method composes stay with the method.

use super::{Choice, Entry, OrchestratorView};
use crate::{
    config::corners,
    i18n::fl,
    motion,
    search_input::{self, SearchInput},
};
use gpui::{prelude::*, *};

/// The height of one row.
const ROW: f32 = 28.;
/// The most rows the list shows before it scrolls.
const SHOWN_ROWS: usize = 12;
const WIDTH: f32 = 340.;

/// The popover's state; the field is made the first time it opens.
pub(crate) struct Picker {
    motion: motion::Toggle,
    input: Option<Entity<SearchInput>>,
    query: String,
    /// The roster's entries the filter matches, by index.
    matched: Vec<usize>,
    focused: usize,
    scroll: UniformListScrollHandle,
    /// The control, and the All and None buttons, as keyboard stops.
    control: FocusHandle,
    all: FocusHandle,
    none: FocusHandle,
    _changed: Option<Subscription>,
}

impl Picker {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            motion: motion::Toggle::default(),
            input: None,
            query: String::new(),
            matched: Vec::new(),
            focused: 0,
            scroll: UniformListScrollHandle::new(),
            control: cx.focus_handle(),
            all: cx.focus_handle(),
            none: cx.focus_handle(),
            _changed: None,
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.motion.is_open()
    }

    /// Matches the filter against `entries` again.
    pub(super) fn refilter(&mut self, entries: &[Entry]) {
        let query = &self.query;
        self.matched = (0..entries.len())
            .filter(|index| matches(&entries[*index], query))
            .collect();
        self.focused = self.focused.min(self.matched.len().saturating_sub(1));
    }
}

/// Whether `entry` matches every word of `query`.
fn matches(entry: &Entry, query: &str) -> bool {
    let text = format!("{} {} {}", entry.name, entry.host, entry.key).to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

impl OrchestratorView {
    /// The `index`th repository the filter field matches.
    fn matched_entry(&self, index: usize) -> Option<&Entry> {
        let at = *self.repo_select.picker.matched.get(index)?;
        self.repo_entries().get(at)
    }

    pub(in crate::orchestrator::view) fn open_repo_picker(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repo_select.picker.is_open() == open {
            return;
        }
        self.repo_select
            .picker
            .motion
            .set(open, motion::now(), false);
        if !open {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let look = self.look.clone();
        let input = self.repo_select.picker.input.get_or_insert_with(|| {
            cx.new(|cx| {
                let mut input = SearchInput::new(cx);
                input.set_placeholder_with(|| fl!("orchestrator-repos-filter"), cx);
                input.set_frameless(cx);
                input.set_appearance(look.ui.clone(), look.theme.clone(), cx);
                input
            })
        });
        let input = input.clone();
        if self.repo_select.picker._changed.is_none() {
            self.repo_select.picker._changed = Some(cx.subscribe(
                &input,
                |this, input, _: &search_input::Changed, cx| {
                    let select = &mut this.repo_select;
                    select.picker.query = input.read(cx).text().to_owned();
                    select.picker.focused = 0;
                    select.picker.refilter(select.roster.entries());
                    cx.notify();
                },
            ));
        }
        self.prepare_repo_roster(cx);
        self.repo_select.picker.focused = 0;
        window.focus(&input.read(cx).focus.clone(), cx);
        cx.notify();
    }

    /// The popover's keys while it is open; whether one was used. The
    /// caller passes no key of an open popover on to the list or board.
    pub(in crate::orchestrator::view) fn repo_picker_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let picker = &self.repo_select.picker;
        let input = picker.input.as_ref().map(|input| input.read(cx));
        let typing = input.is_some_and(|input| input.focus.is_focused(window));
        // Enter, Escape, and the arrows may belong to a composition.
        if typing && input.is_some_and(SearchInput::is_composing) {
            return false;
        }
        let modifiers = &event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        let on_all = picker.all.is_focused(window);
        let on_none = picker.none.is_focused(window);
        let count = picker.matched.len();
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => self.open_repo_picker(false, window, cx),
            "tab" => self.step_picker_focus(modifiers.shift, window, cx),
            "enter" | "space" if on_all => {
                let _ = self.choose_repos(Choice::All, cx);
            }
            "enter" | "space" if on_none => {
                let _ = self.choose_repos(Choice::Only(Vec::new()), cx);
            }
            "down" | "up" => {
                let picker = &mut self.repo_select.picker;
                picker.focused = if key == "down" {
                    (picker.focused + 1).min(count.saturating_sub(1))
                } else {
                    picker.focused.saturating_sub(1)
                };
                picker
                    .scroll
                    .scroll_to_item(picker.focused, ScrollStrategy::Nearest);
                cx.notify();
            }
            "enter" => self.toggle_focused_repo(cx),
            "space" if !typing => self.toggle_focused_repo(cx),
            _ => return false,
        }
        true
    }

    /// Tab and Shift-Tab: the field, All, None, and round again.
    fn step_picker_focus(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let picker = &self.repo_select.picker;
        let Some(input) = picker
            .input
            .as_ref()
            .map(|input| input.read(cx).focus.clone())
        else {
            return;
        };
        let stops = [input, picker.all.clone(), picker.none.clone()];
        let at = stops
            .iter()
            .position(|stop| stop.is_focused(window))
            .unwrap_or(0);
        let next = if back { at + stops.len() - 1 } else { at + 1 } % stops.len();
        window.focus(&stops[next], cx);
        cx.notify();
    }

    /// P opens the popover while the list or board itself holds the keys.
    pub(in crate::orchestrator::view) fn repo_picker_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.keystroke.key != "p"
            || event.keystroke.modifiers != Modifiers::default()
            || !self.focus.is_focused(window)
        {
            return false;
        }
        self.open_repo_picker(true, window, cx);
        true
    }

    fn toggle_focused_repo(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.matched_entry(self.repo_select.picker.focused) {
            let key = entry.key.clone();
            self.toggle_repo(&key, cx);
        }
    }

    /// Keeps frames coming while the popover opens or closes.
    pub(in crate::orchestrator::view) fn repo_picker_frame(&self, window: &mut Window) {
        if self.repo_select.picker.motion.moving(motion::now()) {
            window.request_animation_frame();
        }
    }

    /// The board under a row holding the control. The board places its
    /// own overlays, so the row moves none of them.
    pub(in crate::orchestrator::view) fn render_board_with_repos(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = self.render_repo_control(cx);
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_end()
                    .px_4()
                    .pt_2()
                    .child(control),
            )
            .child(self.render_kanban(window, cx))
            .into_any_element()
    }

    /// The control, with the popover under it, shared by every section.
    pub(in crate::orchestrator::view) fn render_repo_control(&self, cx: &mut Context<Self>) -> Div {
        let roster = &self.repo_select.roster;
        let (count, shown, new) = (roster.entries().len(), roster.shown(), roster.new_count());
        let look = &self.look;
        let theme = &look.theme;
        let open = self.repo_select.picker.is_open();
        let label = if self.repo_select.active().is_none() {
            fl!("orchestrator-repos-all", count = count)
        } else {
            fl!("orchestrator-repos-some", shown = shown, count = count)
        };
        let control = div()
            .id("orchestrator-repos")
            .debug_selector(|| "orchestrator-repos".into())
            .role(Role::Button)
            .aria_label(label.clone())
            .track_focus(&self.repo_select.picker.control)
            .tab_index(0)
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py(px(2.))
            .rounded(px(corners::CONTROL))
            .border_1()
            .border_color(rgb(if open { theme.primary() } else { theme.active }))
            .focus(|style| style.border_color(rgb(theme.primary())))
            .cursor_pointer()
            .text_size(look.small())
            .text_color(rgb(theme.subtext()))
            .child(look.icon("icons/git-branch.svg", 11., theme.subtext()))
            .child(label)
            .when(new > 0, |el| {
                el.child(
                    div()
                        .px(px(5.))
                        .rounded_full()
                        .bg(rgb(theme.primary()))
                        .text_color(rgb(theme.text_on(theme.primary())))
                        .text_size(look.tiny())
                        .child(fl!("orchestrator-repos-new-count", count = new)),
                )
            })
            .child(look.icon("icons/chevron-down.svg", 10., theme.muted))
            .on_click(cx.listener(|this, _, window, cx| {
                let open = !this.repo_select.picker.is_open();
                this.open_repo_picker(open, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let plain = event.keystroke.modifiers == Modifiers::default();
                if plain && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    let open = !this.repo_select.picker.is_open();
                    this.open_repo_picker(open, window, cx);
                    cx.stop_propagation();
                }
            }));
        let warn = |text: String| {
            div()
                .text_size(look.small())
                .text_color(rgb(look.hue(super::super::look::RED)))
                .child(text)
        };
        let failed = self
            .repo_select
            .save_failed
            .then(|| warn(fl!("orchestrator-repos-unsaved")));
        let full = self
            .repo_select
            .full
            .then(|| warn(fl!("orchestrator-repos-full")));
        let popover = self.render_repo_popover(cx);
        div()
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .children(failed)
            .children(full)
            .child(control)
            .children(popover)
    }

    fn render_repo_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let now = motion::now();
        let picker = &self.repo_select.picker;
        if !picker.motion.is_open() && !picker.motion.moving(now) {
            return None;
        }
        let shown = picker.motion.shown(now);
        let count = picker.matched.len();
        let focused = picker.focused.min(count.saturating_sub(1));
        let look = &self.look;
        let theme = &look.theme;
        let input = picker.input.clone()?;
        let pick = |id: &'static str, label: String, choice: Choice, focus: &FocusHandle| {
            look.button(id, label, false)
                .debug_selector(move || id.to_owned())
                .track_focus(focus)
                .focus(|style| style.border_color(rgb(theme.primary())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let _ = this.choose_repos(choice.clone(), cx);
                }))
        };
        let height = px(ROW * count.clamp(1, SHOWN_ROWS) as f32);
        let list = if count == 0 {
            div()
                .h(height)
                .px_2()
                .flex()
                .items_center()
                .text_color(rgb(theme.muted))
                .child(fl!("orchestrator-repos-none-found"))
                .into_any_element()
        } else {
            uniform_list(
                "orchestrator-repos-list",
                count,
                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .filter_map(|index| {
                            let entry = this.matched_entry(index)?;
                            Some(this.render_repo_row(index, entry, index == focused, cx))
                        })
                        .collect()
                }),
            )
            .track_scroll(&self.repo_select.picker.scroll)
            .h(height)
            .into_any_element()
        };
        Some(
            deferred(
                div()
                    .id("orchestrator-repos-popover")
                    .debug_selector(|| "orchestrator-repos-popover".into())
                    .occlude()
                    .absolute()
                    .top(px(26. - (1. - shown) * 4.))
                    .right_0()
                    .w(px(WIDTH))
                    .opacity(shown)
                    .p_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .rounded(px(corners::CONTROL))
                    .bg(rgb(theme.surface))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        this.open_repo_picker(false, window, cx);
                    }))
                    .child(
                        look.search_frame("orchestrator-repos-filter", true)
                            .flex_none()
                            .child(div().flex_1().min_w_0().child(input)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(pick(
                                "orchestrator-repos-select-all",
                                fl!("orchestrator-repos-select-all"),
                                Choice::All,
                                &picker.all,
                            ))
                            .child(pick(
                                "orchestrator-repos-select-none",
                                fl!("orchestrator-repos-select-none"),
                                Choice::Only(Vec::new()),
                                &picker.none,
                            )),
                    )
                    .child(list),
            )
            .into_any_element(),
        )
    }

    fn render_repo_row(
        &self,
        index: usize,
        entry: &Entry,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let look = &self.look;
        let theme = &look.theme;
        let key = entry.key.clone();
        let check = div()
            .size(px(14.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(corners::SMALL))
            .border_1()
            .border_color(rgb(if entry.active {
                theme.primary()
            } else {
                theme.muted
            }))
            .when(entry.active, |el| {
                el.child(look.icon("icons/check.svg", 11., theme.primary()))
            });
        self.menu_row(format!("orchestrator-repos-row-{index}").into(), focused)
            .h(px(ROW))
            .role(Role::CheckBox)
            .aria_label(entry.name.clone())
            .child(check)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(entry.name.clone()),
            )
            .when(entry.new, |el| {
                el.child(
                    div()
                        .text_size(look.tiny())
                        .text_color(rgb(theme.primary()))
                        .child(fl!("orchestrator-repos-new")),
                )
            })
            .child(
                div()
                    .max_w(px(140.))
                    .truncate()
                    .text_size(look.small())
                    .text_color(rgb(theme.muted))
                    .child(entry.host.clone()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.repo_select.picker.focused = index;
                this.toggle_repo(&key, cx);
            }))
            .into_any_element()
    }
}
