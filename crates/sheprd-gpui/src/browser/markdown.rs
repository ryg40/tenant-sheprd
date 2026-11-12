//! Markdown files as rendered pages, treated the way Obsidian and GitHub
//! treat notes: safe HTML renders, but the page runs no script of its own. A
//! local page can read every file in its folder, so the rendered HTML is
//! sanitized, and a CSP blocks script in case the sanitizer misses anything.
use crate::review::highlight;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::Path,
};

const STYLE: &str = include_str!("markdown.css");
// i18n: data
const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; \
    img-src herdr-preview: http://herdr-preview.localhost https://herdr-preview.localhost https:";
/// What one page colors at most; the rest of its code stays plain text. A
/// page is made while its request waits, and coloring is the slow part:
/// about 3 ms per KiB of dense code in a release build, so these bounds
/// keep a page near 150 ms whatever a file holds.
const MAX_COLOURED_BYTES: usize = 48 * 1024;
const MAX_COLOURED_LINES: usize = 1_000;
/// Lines one fenced block colors at most, so one long listing does not
/// leave every later block plain.
const MAX_BLOCK_COLOURED_LINES: usize = 400;
/// Schemes a link or image may use; relative URLs are always kept.
const URL_SCHEMES: [&str; 3] = ["http", "https", "mailto"];
/// Put before every id, as GitHub does, so a note cannot shadow a page global
/// the host's own script relies on.
const ID_PREFIX: &str = "user-content-";
/// CSS a `style` attribute may keep: text styling only. Layout properties
/// could stretch an invisible link over the page, and `url()` values would
/// load what the image rules refuse.
const STYLE_PROPERTIES: [&str; 9] = [
    "color",
    "background-color",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "text-align",
    "text-decoration",
    "vertical-align",
];

pub(super) use super::file_view::is_markdown;

/// A markdown file as a complete, self-contained HTML page. `theme` is the
/// app's own stylesheet of the window's colors (`kit::theme_css`), written
/// into the page; the page's own colors stand in where it is empty.
pub(super) fn page(source: &str, file_name: &str, theme: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let mut events: Vec<Event<'_>> = Parser::new_ext(source, options).collect();
    let heading = add_heading_ids(&mut events);
    let events = colour_code_blocks(events);
    let mut rendered = String::with_capacity(source.len() + source.len() / 2);
    html::push_html(&mut rendered, events.into_iter());
    let body = sanitizer().clean(&rendered).to_string();
    let title = heading
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(file_name);
    format!(
        // i18n: data
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\
         <title>{title}</title><style>{theme}{STYLE}</style></head>\
         <body><main class=\"markdown\">\n{body}</main></body></html>\n",
        title = escape(title),
    )
}

/// Ammonia's safe defaults, plus what rendered markdown and Obsidian notes
/// use: task-list checkboxes, and `class`, `id`, and `style` on any tag.
fn sanitizer() -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::default();
    builder
        .add_tags(["input"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_generic_attributes(["class", "id", "style"])
        .filter_style_properties(HashSet::from(STYLE_PROPERTIES))
        .url_schemes(HashSet::from(URL_SCHEMES))
        .id_prefix(Some(ID_PREFIX))
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("input", "type") if value != "checkbox" => None,
            ("a", "href") => Some(link(value)),
            _ => Some(Cow::Borrowed(value)),
        });
    builder
}

/// Replaces each fenced block that names a language with its code already
/// colored: one `span` per token, classed by what the token is. The code is
/// escaped here and the result still passes the sanitizer, so a block's
/// text can add no markup.
fn colour_code_blocks(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut coloured = Vec::with_capacity(events.len());
    let mut budget = Budget {
        bytes: MAX_COLOURED_BYTES,
        lines: MAX_COLOURED_LINES,
    };
    let mut events = events.into_iter();
    while let Some(event) = events.next() {
        let language = match &event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => info
                .split([' ', ',', '\t', '{'])
                .next()
                .filter(|language| !language.is_empty())
                .map(str::to_ascii_lowercase),
            _ => None,
        };
        let Some(language) = language else {
            coloured.push(event);
            continue;
        };
        let mut code = String::new();
        for inner in events.by_ref() {
            match inner {
                Event::End(TagEnd::CodeBlock) => break,
                Event::Text(text) => code.push_str(&text),
                _ => {}
            }
        }
        let spans = match budget.take(&code) {
            0 => Vec::new(),
            lines => highlight::colour_snippet(&language, &code, lines),
        };
        coloured.push(Event::Html(code_block(&language, &code, &spans).into()));
    }
    coloured
}

/// What is left of a page's coloring.
struct Budget {
    bytes: usize,
    lines: usize,
}

impl Budget {
    /// How many of `code`'s first lines may be colored, taken from what is
    /// left. The lines after them stay plain.
    fn take(&mut self, code: &str) -> usize {
        let mut lines = 0;
        for line in code
            .split_inclusive('\n')
            .take(MAX_BLOCK_COLOURED_LINES.min(self.lines))
        {
            if line.len() > self.bytes {
                break;
            }
            self.bytes -= line.len();
            lines += 1;
        }
        self.lines -= lines;
        lines
    }
}

/// A fenced block as HTML, each line's `spans` wrapped by their class.
fn code_block(language: &str, code: &str, spans: &[Vec<highlight::Span>]) -> String {
    let mut block = String::with_capacity(code.len() * 2);
    // i18n: data
    block.push_str("<pre><code class=\"language-");
    block.push_str(&escape(language));
    block.push_str("\">");
    for (index, line) in code.split_inclusive('\n').enumerate() {
        let mut at = 0;
        for span in spans.get(index).map(Vec::as_slice).unwrap_or_default() {
            let (Some(before), Some(token)) =
                (line.get(at..span.start), line.get(span.start..span.end))
            else {
                break;
            };
            block.push_str(&escape(before));
            // i18n: data
            block.push_str("<span class=\"tok-");
            block.push_str(span.token.css_name());
            block.push_str("\">");
            block.push_str(&escape(token));
            block.push_str("</span>");
            at = span.end;
        }
        block.push_str(&escape(line.get(at..).unwrap_or_default()));
    }
    block.push_str("</code></pre>\n");
    block
}

/// Gives each heading a GitHub-style id made from its text, numbered when
/// the text repeats and never the same as an earlier heading's, so `#section`
/// links reach it once the sanitizer adds the prefix. Returns the first
/// heading's text, without any HTML inside it.
fn add_heading_ids(events: &mut [Event<'_>]) -> Option<String> {
    let mut first = None;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    for index in 0..events.len() {
        if !matches!(events[index], Event::Start(Tag::Heading { id: None, .. })) {
            continue;
        }
        let text = heading_text(&events[index + 1..]);
        let slug = slug(&text);
        first.get_or_insert(text);
        if slug.is_empty() {
            continue;
        }
        let count = seen.entry(slug.clone()).or_default();
        let mut id = match *count {
            0 => slug.clone(),
            count => format!("{slug}-{count}"),
        };
        while used.contains(&id) {
            *count += 1;
            id = format!("{slug}-{count}");
        }
        *count += 1;
        used.insert(id.clone());
        if let Event::Start(Tag::Heading { id: slot, .. }) = &mut events[index] {
            *slot = Some(id.into());
        }
    }
    first
}

/// The plain text of a heading, from the events after its start.
fn heading_text(events: &[Event<'_>]) -> String {
    let mut text = String::new();
    for event in events {
        match event {
            Event::End(TagEnd::Heading(_)) => break,
            Event::Text(part) | Event::Code(part) => text.push_str(part),
            _ => {}
        }
    }
    text
}

/// A heading's anchor as GitHub makes it: lowercase letters, digits, `-` and
/// `_`, with each space turned into `-`.
fn slug(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|character| match character {
            ' ' => Some('-'),
            '-' | '_' => Some(character),
            character if character.is_alphanumeric() => Some(character),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// A link's target with its fragment following the prefixed ids, on this page
/// and in other markdown files of the folder. Fragments of other pages and
/// sites, an empty `#`, and fragments that already carry the prefix are kept:
/// the sanitizer leaves an id that already starts with it unchanged too.
fn link(href: &str) -> Cow<'_, str> {
    let Some((target, fragment)) = href.split_once('#') else {
        return Cow::Borrowed(href);
    };
    if fragment.is_empty() || fragment.starts_with(ID_PREFIX) {
        return Cow::Borrowed(href);
    }
    let path = target.split('?').next().unwrap_or_default();
    let has_scheme = path
        .split('/')
        .next()
        .is_some_and(|part| part.contains(':'));
    let local_markdown = !has_scheme && !path.starts_with("//") && is_markdown(Path::new(path));
    if path.is_empty() || local_markdown {
        Cow::Owned(format!("{target}#{ID_PREFIX}{fragment}"))
    } else {
        Cow::Borrowed(href)
    }
}

/// Escapes text for an HTML element or a quoted attribute.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests;
