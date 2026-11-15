#![allow(clippy::unwrap_used)]
use super::*;
use crate::walkr::history::{RATINGS_PAGE, parse};

const SESSION: &str = "20261101T120000Z-0123456789ab";
const HOSTILE: &str = r#"<script>alert(1)</script><img src=x onerror=\"alert(2)\">"#;

/// A record whose every shown field holds markup.
fn hostile() -> Detail {
    let text = format!(
        r#"{{"schema":"walkr-compare-session","version":1,"sessionId":"{SESSION}","createdAt":"2026-11-01T12:00:00.000Z","status":"submitted","workflow":"{HOSTILE}","title":"{HOSTILE}","brief":"{HOSTILE}","clarifications":[{{"question":"{HOSTILE}","answer":"{HOSTILE}"}}],"options":[{{"optionId":"a","label":"{HOSTILE}","summary":"{HOSTILE}","format":"markdown","content":"{HOSTILE} [link](https://example.test) ![x](https://example.test/x.png)"}},{{"optionId":"b","label":"B","format":"html","content":"<p>INVENTEDHTMLMARK</p>"}}],"result":{{"pick":"a","comments":{{"a":"{HOSTILE}"}},"note":"{HOSTILE}"}},"outcome":{{"summary":"{HOSTILE}"}},"ratings":[{{"value":7,"note":"{HOSTILE}"}}]}}"#
    );
    parse(SESSION, text.as_bytes(), RATINGS_PAGE).unwrap()
}

/// The names of the elements `html` opens and closes.
fn tags(html: &str) -> Vec<String> {
    html.split('<')
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '!' | '/'))
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .collect()
}

#[test]
fn record_text_comes_out_inert_under_a_strict_policy() {
    let html = document(&hostile(), ":root { --hk-bg: #000000; }");
    // Only the document's own elements: no script, image, link, or form.
    const OWN: &[&str] = &[
        // keep-sorted start
        "!doctype", "article", "body", "dd", "div", "dl", "dt", "h1", "h2", "h3", "h4", "head",
        "header", "html", "li", "main", "meta", "ol", "p", "section", "span", "style",
        "title",
        // keep-sorted end
    ];
    for tag in tags(&html) {
        let name = tag.trim_start_matches('/');
        assert!(OWN.contains(&name), "{tag}");
    }
    // No element carries an address: record text only shows escaped.
    for markup in html.split('<').skip(1) {
        let markup = markup
            .split('>')
            .next()
            .unwrap_or(markup)
            .to_ascii_lowercase();
        assert!(
            !markup.contains("href=") && !markup.contains("src="),
            "{markup}"
        );
    }
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(html.contains("&lt;img src=x onerror=&quot;alert(2)&quot;&gt;"));
    assert!(!html.contains("INVENTEDHTMLMARK"));
    assert!(html.contains(
        r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'">"#
    ));
    // The policy comes before anything a record wrote.
    assert!(html.find("Content-Security-Policy") < html.find("&lt;script"));
}
