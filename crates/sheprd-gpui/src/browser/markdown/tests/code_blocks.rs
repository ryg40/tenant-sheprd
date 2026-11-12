//! Fenced code is colored by the app, and stays text.
use super::*;

#[test]
fn a_fenced_block_is_colored_and_its_text_stays_text() {
    let html = body(
        "```rust\nfn main() { let s = \"<script>alert(1)</script>\"; }\n```\n\n\
         ```html\n<img src=x onerror=alert(2)>\n<a href=\"javascript:alert(3)\">x</a>\n```\n",
    );
    assert!(html.contains("tok-keyword"), "{html}");
    assert!(html.contains("class=\"language-rust\""));
    // The code's own markup is escaped, never an element of the page.
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    for live in ["<script", "<img", "<a href", "onerror=alert(2)>"] {
        assert!(!html.contains(live), "{live}: {html}");
    }
}

#[test]
fn a_language_name_cannot_add_markup() {
    let html = body("```\"><script>alert(1)</script>\ncode\n```\n");
    assert!(!html.contains("<script"), "{html}");
    assert!(html.contains("code"));
}

#[test]
fn the_theme_is_written_into_the_page_and_its_policy_stays() {
    let html = page("# T\n", "t.md", ":root { --hk-bg: #010203; }");
    assert!(html.contains("--hk-bg: #010203"));
    assert!(html.contains("default-src 'none'"));
    assert!(!html.contains("script-src"));
}
