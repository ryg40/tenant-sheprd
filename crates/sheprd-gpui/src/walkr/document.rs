//! A saved session as an HTML document for the section's page, made from
//! the record the reader already bounded. Every field is untrusted text:
//! each one is escaped, and only this file's own literals are markup. The
//! document runs no script and holds no link, form, or remote resource,
//! and its Content-Security-Policy refuses everything but inline style.
// i18n: migrated
use super::{
    history::{Detail, OptionBody},
    history_view::{local_time, stars, status_text},
};
use crate::i18n::fl;

const STYLE: &str = include_str!("document.css");
/// Nothing loads or runs; the page's own `<style>` and `style` attributes
/// apply.
// i18n: data
pub(crate) const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'";

/// The fragment of the `index`th rating shown, newest first, so that a
/// document with earlier ratings can open where they start.
pub(crate) fn rating_anchor(index: usize) -> String {
    // i18n: data
    format!("rating-{index}")
}

/// The document of `detail`. `theme` is the app's stylesheet of the
/// window's colors (`browser::kit::theme_css`), which the page's own colors
/// stand in for where it is empty.
pub(crate) fn document(detail: &Detail, theme: &str) -> String {
    let mut html = Html::default();
    let title = if detail.title.trim().is_empty() {
        fl!("walkr-row-untitled")
    } else {
        detail.title.clone()
    };
    let rtl = crate::i18n::direction() == sheprd_i18n::Direction::Rtl;
    html.raw(if rtl {
        "<!doctype html>\n<html dir=\"rtl\"><head>" // i18n: data
    } else {
        "<!doctype html>\n<html dir=\"ltr\"><head>" // i18n: data
    });
    // i18n: data
    html.raw("<meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"");
    html.raw(CSP);
    html.raw("\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>"); // i18n: data
    html.text(&title);
    html.raw("</title><style>");
    // The theme is the app's own text, made from colors; no `<` can end
    // the element early.
    html.0.extend(theme.chars().filter(|c| *c != '<'));
    html.raw(STYLE);
    html.raw("</style></head><body><main>");
    header(&mut html, detail, &title);
    html.section(&fl!("walkr-detail-brief"));
    html.block("brief", &detail.brief);
    html.raw("</section>");
    clarifications(&mut html, detail);
    options(&mut html, detail);
    pick(&mut html, detail);
    if let Some(outcome) = &detail.outcome {
        html.section(&fl!("walkr-detail-outcome"));
        html.block("text", outcome);
        html.raw("</section>");
    }
    ratings(&mut html, detail);
    html.raw("</main></body></html>\n");
    html.0
}

fn header(html: &mut Html, detail: &Detail, title: &str) {
    let mut meta = vec![local_time(detail.created)];
    meta.extend(status_text(detail.status).map(String::from));
    if !detail.workflow.is_empty() {
        meta.push(detail.workflow.clone());
    }
    html.raw("<header><h1>");
    html.text(title);
    html.raw("</h1>");
    html.line("meta", &meta.join(" · "));
    html.raw("</header>");
}

fn clarifications(html: &mut Html, detail: &Detail) {
    if detail.clarifications.is_empty() && detail.clarifications_omitted == 0 {
        return;
    }
    html.section(&fl!("walkr-detail-questions"));
    html.raw("<dl>");
    for (question, answer) in &detail.clarifications {
        html.raw("<dt>");
        html.text(question);
        html.raw("</dt><dd>");
        html.text(answer);
        html.raw("</dd>");
    }
    html.raw("</dl>");
    if detail.clarifications_omitted > 0 {
        let count = detail.clarifications_omitted;
        html.line(
            "muted",
            &fl!("walkr-detail-questions-omitted", count = count),
        );
    }
    html.raw("</section>");
}

fn options(html: &mut Html, detail: &Detail) {
    let picked = detail
        .result
        .as_ref()
        .and_then(|result| result.pick.as_deref());
    html.section(&fl!("walkr-detail-options"));
    html.raw("<div class=\"cards\">"); // i18n: data
    for option in &detail.options {
        let is_pick = picked == Some(option.id.as_str());
        html.raw(if is_pick {
            "<article class=\"card picked\"><div class=\"label\"><h3>" // i18n: data
        } else {
            "<article class=\"card\"><div class=\"label\"><h3>" // i18n: data
        });
        html.text(&option.label);
        html.raw("</h3>");
        if is_pick {
            html.line("badge", &fl!("walkr-detail-picked"));
        }
        html.raw("</div>");
        if let Some(summary) = &option.summary {
            html.line("muted", summary);
        }
        match &option.body {
            // Markdown options show their source: the browser's Markdown
            // page keeps web images and links, which this page must not
            // load or follow.
            OptionBody::Text { text, cut } => {
                html.block("body", text);
                if *cut {
                    html.line("muted", &fl!("walkr-detail-cut"));
                }
            }
            OptionBody::NotDrawn { bytes } => {
                let size = bytes.div_ceil(1024);
                html.line("muted", &fl!("walkr-detail-html", size = size));
            }
        }
        let comment = detail.result.as_ref().and_then(|result| {
            result
                .comments
                .iter()
                .find(|(id, _)| *id == option.id)
                .map(|(_, comment)| comment)
        });
        if let Some(comment) = comment {
            html.raw("<div class=\"comment\"><h4>"); // i18n: data
            html.text(&fl!("walkr-detail-comment"));
            html.raw("</h4>");
            html.block("text", comment);
            html.raw("</div>");
        }
        html.raw("</article>");
    }
    html.raw("</div>");
    if detail.options_omitted > 0 {
        let count = detail.options_omitted;
        html.line("muted", &fl!("walkr-detail-options-omitted", count = count));
    }
    html.raw("</section>");
}

fn pick(html: &mut Html, detail: &Detail) {
    html.section(&fl!("walkr-detail-pick"));
    match &detail.result {
        None => html.line("muted", &fl!("walkr-detail-waiting")),
        Some(result) => {
            let label = result.pick.as_ref().map(|pick| {
                detail
                    .options
                    .iter()
                    .find(|option| option.id == *pick)
                    .map_or(pick, |option| &option.label)
            });
            match label {
                Some(label) => html.line("chosen", label),
                None => html.line("muted", &fl!("walkr-detail-pick-none")),
            }
            if result.comments_omitted > 0 {
                let count = result.comments_omitted;
                html.line(
                    "muted",
                    &fl!("walkr-detail-comments-omitted", count = count),
                );
            }
            if let Some(note) = &result.note {
                html.raw("<h3>");
                html.text(&fl!("walkr-detail-note"));
                html.raw("</h3>");
                html.block("text", note);
            }
        }
    }
    html.raw("</section>");
}

fn ratings(html: &mut Html, detail: &Detail) {
    if detail.ratings.is_empty() {
        return;
    }
    html.section(&fl!("walkr-detail-ratings"));
    html.raw("<ol class=\"ratings\">"); // i18n: data
    for (index, rating) in detail.ratings.iter().rev().enumerate() {
        html.raw("<li id=\""); // i18n: data
        html.text(&rating_anchor(index));
        // Ten half stars fill the row; Walkr writes 1 to 10.
        let filled = rating.value.clamp(0, 10) * 10;
        // i18n: data
        html.raw("\"><span class=\"stars\" aria-hidden=\"true\">★★★★★<span class=\"fill\" style=\"width: ");
        html.text(&filled.to_string());
        html.raw("%\">★★★★★</span></span>");
        html.line(
            "value",
            &fl!("walkr-detail-rating", stars = stars(rating.value)),
        );
        if let Some(note) = &rating.note {
            html.block("note", note);
        }
        html.raw("</li>");
    }
    html.raw("</ol></section>");
}

/// The document as it is written. Record text goes in only through
/// [`Html::text`], which escapes it; markup is only this file's literals.
#[derive(Default)]
struct Html(String);

impl Html {
    fn raw(&mut self, markup: &'static str) {
        self.0.push_str(markup);
    }

    fn text(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '&' => self.0.push_str("&amp;"),
                '<' => self.0.push_str("&lt;"),
                '>' => self.0.push_str("&gt;"),
                '"' => self.0.push_str("&quot;"),
                '\'' => self.0.push_str("&#39;"),
                c => self.0.push(c),
            }
        }
    }

    /// Opens a section with its heading; the caller closes it.
    fn section(&mut self, heading: &str) {
        self.raw("<section><h2>");
        self.text(heading);
        self.raw("</h2>");
    }

    /// One line of `class`.
    fn line(&mut self, class: &'static str, text: &str) {
        self.raw("<p class=\"");
        self.raw(class);
        self.raw("\">");
        self.text(text);
        self.raw("</p>");
    }

    /// Text that keeps its line breaks, of `class`.
    fn block(&mut self, class: &'static str, text: &str) {
        // i18n: data
        self.raw("<div class=\"pre ");
        self.raw(class);
        self.raw("\">");
        self.text(text);
        self.raw("</div>");
    }
}

#[cfg(test)]
#[path = "tests/document.rs"]
mod tests;
