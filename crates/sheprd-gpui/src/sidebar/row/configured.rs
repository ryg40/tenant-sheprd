//! Rows drawn from the daemon's configured sidebar tokens rather than the
//! built-in name and detail lines.

use super::{
    RowIcon, RowKind, Sizing, agent_icon_size, agent_mark, fair_line, label_text, left_behind,
    place_line, row_text, shared_label_text,
};
use crate::{
    config::Theme,
    sidebar::{
        ICON_RESERVE,
        agents::status_mark,
        cell::RowContext,
        glyph_width,
        layout::Face,
        line_height, scaled,
        tokens::{ResolvedToken, TextRole, TokenKind, budgets, separator},
    },
};
use gpui::{prelude::*, *};
use sheprd_client::protocol::AgentStatus;
use unicode_width::UnicodeWidthStr;

/// What colors a configured row's tokens: the row they paint and its state.
#[derive(Clone, Copy)]
pub(in crate::sidebar) struct TokenLook {
    pub(in crate::sidebar) kind: RowKind,
    pub(in crate::sidebar) status: AgentStatus,
    pub(in crate::sidebar) focused: bool,
    /// A teleported checkout fades its name, as the native rows do.
    pub(in crate::sidebar) teleported: bool,
}

fn token_appearance(kind: &TokenKind, look: TokenLook, cx: &RowContext<'_>) -> (u32, FontWeight) {
    let theme = cx.theme;
    let (name, weight, secondary) = row_text(look.kind, look.focused, theme);
    match kind {
        TokenKind::StateIcon | TokenKind::Text(_, TextRole::Status) => {
            (cx.indicators.color(look.status), FontWeight::NORMAL)
        }
        TokenKind::Text(_, TextRole::Workspace) if look.teleported => {
            (left_behind(name, theme), weight)
        }
        TokenKind::Text(_, TextRole::Workspace) => (name, weight),
        TokenKind::Text(_, TextRole::Secondary | TextRole::Agent) => {
            (secondary, FontWeight::NORMAL)
        }
        _ => (theme.muted, FontWeight::NORMAL),
    }
}

/// A token's base color and weight with its configured `fg`, `dim`, and `bold`.
pub(crate) fn styled(
    (color, weight): (u32, FontWeight),
    style: crate::config::TokenStyle,
    theme: &Theme,
) -> (u32, FontWeight) {
    let color = style.fg.unwrap_or(color);
    let color = if style.dim == Some(true) {
        theme.dimmed(color)
    } else {
        color
    };
    let weight = match style.bold {
        Some(true) => FontWeight::BOLD,
        Some(false) => FontWeight::NORMAL,
        None => weight,
    };
    (color, weight)
}

/// Status icons and git counters retain their full width when text truncates.
fn fixed_glyphs(kind: &TokenKind, glyph: f32, status_width: f32) -> usize {
    match kind {
        TokenKind::StateIcon => (status_width / glyph).ceil() as usize,
        TokenKind::GitStatus { ahead, behind } => {
            let count = |n: &usize| format!("{n}").chars().count() + 1;
            usize::from(*ahead > 0) * count(ahead)
                + usize::from(*behind > 0) * count(behind)
                + usize::from(*ahead > 0 && *behind > 0)
        }
        _ => 0,
    }
}

/// The git counters a `git_status` token shows, each with the palette entry
/// its arrow is inked in.
fn git_counts(ahead: usize, behind: usize) -> impl Iterator<Item = (String, usize)> {
    [("↑", ahead, 2), ("↓", behind, 1)]
        .into_iter()
        .filter(|(_, count, _)| *count > 0)
        .map(|(arrow, count, palette)| (format!("{arrow}{count}"), palette))
}

/// How a token's cell takes its width in a proportional face: its text
/// shares the line, an icon or the git counters keep their own.
fn token_sizing(token: &ResolvedToken, look: TokenLook, glyph: f32, cx: &RowContext<'_>) -> Sizing {
    // The weight its cell paints in; the counters' is the token's `bold`.
    let (_, weight) = styled(
        token_appearance(&token.kind, look, cx),
        token.style,
        cx.theme,
    );
    match &token.kind {
        TokenKind::Text(text, _) => Sizing::Share(text.clone(), Some(weight)),
        TokenKind::StateIcon => Sizing::Keep(cx.indicators.width(cx.font), Vec::new()),
        TokenKind::GitStatus { ahead, behind } => {
            let counts: Vec<_> = git_counts(*ahead, *behind)
                .map(|(text, _)| (SharedString::from(text), Some(weight)))
                .collect();
            // The cell's gap between its counters.
            Sizing::Keep(glyph * counts.len().saturating_sub(1) as f32, counts)
        }
    }
}

/// Fixed token widths preserve GPUI's text truncation during layout. In a
/// proportional face the budgets only choose which tokens show: the text
/// ones then share the line by the same rule in measured pixels
/// ([`fair_line`]), and each truncates at its share.
fn token_line(row: &[ResolvedToken], look: TokenLook, width: f32, cx: &RowContext<'_>) -> Div {
    let status = look.status;
    let (font, theme) = (cx.font, cx.theme);
    let face = cx.look.density.face;
    let flexed = face == Face::Proportional;
    let glyph = glyph_width(font);
    let budgets = budgets(
        row,
        |kind| fixed_glyphs(kind, glyph, cx.indicators.width(font)),
        (width / glyph).floor() as usize,
    );
    let visible: Vec<_> = row
        .iter()
        .zip(budgets)
        .filter_map(|(token, width)| Some((token, width?)))
        .collect();
    let used = visible.iter().map(|(_, width)| width).sum::<usize>()
        + visible
            .windows(2)
            .map(|pair| separator(pair[0].0, pair[1].0).chars().count())
            .sum::<usize>();
    let slack = (width - used as f32 * glyph).max(0.);
    let last_text = visible
        .iter()
        .rposition(|(token, _)| token.kind.text().is_some());
    let parts = visible.iter().enumerate().map(|(index, (token, budget))| {
        let gap = if index == 0 {
            ""
        } else {
            separator(visible[index - 1].0, token)
        };
        let advance = *budget as f32 * glyph + if last_text == Some(index) { slack } else { 0. };
        let (color, weight) = styled(token_appearance(&token.kind, look, cx), token.style, theme);
        let cell = div();
        let cell = match &token.kind {
            TokenKind::StateIcon => cell
                .h(px(line_height(font)))
                .flex()
                .justify_center()
                .debug_selector(|| "inline-status-cell".into())
                .child(
                    status_mark(
                        status,
                        font,
                        cx.status_indicators(),
                        color,
                        weight == FontWeight::BOLD,
                    )
                    .debug_selector(|| "inline-status-mark".into()),
                ),
            TokenKind::GitStatus { ahead, behind } => {
                cell.flex()
                    .gap(px(glyph))
                    .children(git_counts(*ahead, *behind).map(|(count, palette)| {
                        let (color, weight) = styled(
                            (theme.ink(theme.palette[palette]), FontWeight::NORMAL),
                            token.style,
                            theme,
                        );
                        div()
                            .flex_none()
                            .text_color(rgb(color))
                            .font_weight(weight)
                            .child(label_text(&count))
                    }))
            }
            // Only text the budget cuts short truncates. An emoji's font is wider
            // than two estimated glyphs, so a label that fits by its display
            // width may overhang its cell slightly rather than collapse to "…".
            // A proportional face's widths are not its glyph counts, so there
            // every text truncates at the width its share gives it.
            TokenKind::Text(text, _) if flexed => cell
                .truncate()
                .font_weight(weight)
                .text_color(rgb(color))
                .child(shared_label_text(text.clone())),
            TokenKind::Text(text, _) if text.width() <= *budget => cell
                .whitespace_nowrap()
                .font_weight(weight)
                .text_color(rgb(color))
                .child(shared_label_text(text.clone())),
            TokenKind::Text(text, _) => cell
                .truncate()
                .font_weight(weight)
                .text_color(rgb(color))
                .child(shared_label_text(text.clone())),
        };
        (gap, advance, cell)
    });
    if flexed {
        let sizings = visible
            .iter()
            .map(|(token, _)| token_sizing(token, look, glyph, cx));
        let parts = parts
            .zip(sizings)
            .map(|((gap, _, cell), sizing)| (gap, cell, sizing));
        return fair_line(parts, width, font, theme.muted);
    }
    place_line(parts, width, font, theme.muted)
}

/// The `state_icon` leading a configured row's first line. It takes the row's
/// status slot; a configured row without one shows no status of its own.
pub(in crate::sidebar) fn leading_status(lines: &[Vec<ResolvedToken>]) -> Option<&ResolvedToken> {
    lines
        .first()
        .and_then(|line| line.first())
        .filter(|token| matches!(token.kind, TokenKind::StateIcon))
}

/// The color, and whether bold, a leading `state_icon` paints the status in.
pub(in crate::sidebar) fn configured_status_style(
    token: &ResolvedToken,
    status: AgentStatus,
    cx: &RowContext<'_>,
) -> (u32, bool) {
    let (color, weight) = styled(
        (cx.indicators.color(status), FontWeight::NORMAL),
        token.style,
        cx.theme,
    );
    (color, weight == FontWeight::BOLD)
}

/// The status mark a leading `state_icon` styles, offset onto the first line.
pub(in crate::sidebar) fn configured_status(
    token: &ResolvedToken,
    status: AgentStatus,
    cx: &RowContext<'_>,
) -> Div {
    let (color, bold) = configured_status_style(token, status, cx);
    status_mark(status, cx.font, cx.status_indicators(), color, bold)
}

/// A line's tokens, less the leading `state_icon` the status slot draws.
fn line_tokens(index: usize, line: &[ResolvedToken]) -> &[ResolvedToken] {
    match line.split_first() {
        Some((first, rest)) if index == 0 && matches!(first.kind, TokenKind::StateIcon) => rest,
        _ => line,
    }
}

fn line_selector(key: &str, index: usize) -> String {
    match index {
        0 => format!("name-{key}"),
        1 => format!("detail-{key}"),
        index => format!("line-{key}-{index}"),
    }
}

/// Configured lines stacked at `width`, for layouts that draw their own
/// status and icons beside the text.
pub(in crate::sidebar) fn token_column(
    key: &str,
    lines: &[Vec<ResolvedToken>],
    look: TokenLook,
    width: f32,
    cx: &RowContext<'_>,
) -> Div {
    lines.iter().enumerate().fold(
        div().w(px(width)).flex_none().flex().flex_col(),
        |column, (index, line)| {
            column.child(
                token_line(line_tokens(index, line), look, width, cx)
                    .debug_selector(|| line_selector(key, index)),
            )
        },
    )
}

pub(super) fn configured_lines(
    mut column: Div,
    key: &str,
    lines: &[Vec<ResolvedToken>],
    look: TokenLook,
    (label_width, detail_limit): (f32, f32),
    mut workspace_icon: RowIcon,
    cx: &RowContext<'_>,
) -> Div {
    let (font, theme) = (cx.font, cx.theme);
    let agent_icon = match look.kind {
        RowKind::Agent(icon) => Some(icon),
        RowKind::Workspace => None,
    };
    let agent_at = lines.iter().position(|line| {
        line.iter()
            .any(|token| matches!(token.kind, TokenKind::Text(_, TextRole::Agent)))
    });
    let (agent_size, agent_reserve) = agent_icon_size(font);
    let workspace_reserve = if matches!(workspace_icon, RowIcon::None) {
        0.
    } else {
        scaled(font, ICON_RESERVE)
    };
    for (index, line) in lines.iter().enumerate() {
        // The second line sits beside the badge's counts, so it may have
        // less room than the first.
        let label_width = if index == 1 {
            detail_limit
        } else {
            label_width
        };
        let agent_here = agent_at == Some(index);
        let reserve = if index == 0 { workspace_reserve } else { 0. }
            + if agent_here { agent_reserve } else { 0. };
        let text_width = (label_width - reserve).max(0.);
        let selector = line_selector(key, index);
        let icon_color = line
            .iter()
            .find(|token| matches!(token.kind, TokenKind::Text(_, TextRole::Agent)))
            .map(|token| styled(token_appearance(&token.kind, look, cx), token.style, theme).0)
            .unwrap_or(theme.muted);
        let mut text = div().relative().w(px(label_width)).h(px(line_height(font)));
        if agent_here && let Some(icon) = agent_icon {
            text = text.child(agent_mark(key, icon, agent_size, icon_color, font));
        }
        if index == 0 && workspace_icon.draws() {
            text = text.child(std::mem::replace(&mut workspace_icon, RowIcon::None).slot(
                key,
                font,
                theme.muted,
            ));
        }
        let density = cx.look.density;
        text = text.child(
            token_line(line_tokens(index, line), look, text_width, cx)
                .debug_selector(|| selector.clone())
                .ml(px(reserve.min(label_width)))
                .when(index > 0 && density.detail_step > 0., |line| {
                    line.text_size(px(density.detail_size(font)))
                }),
        );
        column = column.child(text);
    }
    column
}

#[cfg(test)]
mod tests;
