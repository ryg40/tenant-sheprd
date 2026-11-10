#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn pages_only_navigate_to_web_addresses() {
    for allowed in [
        "https://a.test/",
        "http://localhost:3000/x",
        "about:blank",
        "herdr-preview://localhost/index.html",
    ] {
        assert!(navigable(allowed), "{allowed}");
    }
    for denied in [
        "file:///etc/passwd",
        "vscode://file/x",
        "mailto:a@b.test",
        "data:text/html,x",
        "herdr-preview://elsewhere/x",
    ] {
        assert!(!navigable(denied), "{denied}");
    }
}

fn url(text: &str) -> url::Url {
    url::Url::parse(text).unwrap()
}

#[test]
fn a_servo_page_loads_no_file_of_this_computer() {
    for allowed in [
        "https://a.test/app.js",
        "http://localhost:3000/x.png",
        "data:image/png;base64,AA==",
        "about:blank",
        "herdr-preview://localhost/css/app.css",
    ] {
        assert!(loadable(&url(allowed)), "{allowed}");
    }
    for denied in [
        "file:///etc/passwd",
        "file:///home/me/site/app.js",
        "herdr-preview://elsewhere/x",
        "resource://rippy.png",
        "ftp://a.test/x",
    ] {
        assert!(!loadable(&url(denied)), "{denied}");
    }
}

#[test]
fn the_preview_scheme_answers_only_a_local_page() {
    let page = "herdr-preview://localhost/index.html";
    let web = "https://evil.test/";
    // The local page's own resources, and its links to its own folder.
    assert!(!preview_refused(Some(page), false, Some(page)));
    assert!(!preview_refused(Some(page), true, Some(page)));
    assert!(!preview_refused(
        Some("herdr-preview://localhost"),
        false,
        Some(page)
    ));
    // The app or the user opens a local page, in a new tab or over a site.
    assert!(!preview_refused(None, true, None));
    assert!(!preview_refused(None, true, Some(web)));
    // A local page that hides its address still gets its own files.
    assert!(!preview_refused(None, false, Some(page)));
    // A web page gets none: not as a resource, not in a frame, not by
    // sending the tab there, and not by hiding its address.
    assert!(preview_refused(Some(web), false, Some(web)));
    assert!(preview_refused(Some(web), false, Some(page)));
    assert!(preview_refused(Some(web), true, Some(web)));
    assert!(preview_refused(None, false, Some(web)));
    assert!(preview_refused(None, false, None));
    assert!(preview_refused(
        Some("https://herdr-preview.localhost.evil.test/"),
        false,
        Some(page)
    ));
}

#[test]
fn only_a_local_pages_own_frames_reach_the_bridge() {
    let post = url("herdr-preview://localhost/.herdr/post?m=%7B%22kind%22%3A%22send%22%7D");
    let body = r#"{"kind":"send"}"#.to_owned();
    assert_eq!(
        posted(&post, Some("herdr-preview://localhost/mock.html")),
        Some((body.clone(), PostedFrom::Preview))
    );
    // A web page's message is kept apart, wherever the tab came from, and
    // so is one from a frame Servo does not name.
    assert_eq!(
        posted(&post, Some("https://evil.test/")),
        Some((body.clone(), PostedFrom::Elsewhere))
    );
    assert_eq!(posted(&post, None), Some((body, PostedFrom::Elsewhere)));
    // No message, or one too long to queue, is dropped.
    assert_eq!(
        posted(&url("herdr-preview://localhost/.herdr/post"), None),
        None
    );
    let long = format!(
        "herdr-preview://localhost/.herdr/post?m={}",
        "x".repeat(MAX_POST_BYTES + 1)
    );
    assert_eq!(
        posted(&url(&long), Some("herdr-preview://localhost/")),
        None
    );
}

#[test]
fn a_servo_request_meets_the_rules_in_order() {
    let page = "herdr-preview://localhost/mock.html";
    let web = "https://example.com/";
    let decide = |address: &str, referrer, main_frame, top| {
        verdict(&url(address), referrer, main_frame, top)
    };
    // A file of this computer is refused first, whoever asks and whatever
    // its path looks like: not as a message, and not as a page's file.
    for address in [
        "file:///etc/passwd",
        "file:///.herdr/post?m=1",
        "herdr-preview://elsewhere/.herdr/post?m=1",
        "herdr-preview://elsewhere/mock.html",
    ] {
        assert_eq!(
            decide(address, Some(page), false, Some(page)),
            Verdict::Refuse,
            "{address}"
        );
    }
    // The web goes to the network before the preview scheme's rules: a web
    // address with the message path posts nothing and reads no folder.
    for address in [
        "https://example.com/.herdr/post?m=1",
        "https://example.com/",
    ] {
        assert_eq!(
            decide(address, Some(page), false, Some(page)),
            Verdict::Network,
            "{address}"
        );
        assert_eq!(
            decide(address, Some(web), true, Some(web)),
            Verdict::Network
        );
    }
    // A message is read before any file is looked for, from any frame: who
    // posted it is `posted`'s to say, and the path is never a file.
    let post = "herdr-preview://localhost/.herdr/post?m=1";
    assert_eq!(decide(post, Some(page), false, Some(page)), Verdict::Post);
    assert_eq!(decide(post, Some(web), false, Some(web)), Verdict::Post);
    assert_eq!(decide(post, None, false, None), Verdict::Post);
    // Every other preview request is answered by the preview scheme, from
    // the folder only where `preview_refused` allows.
    assert_eq!(
        decide(page, Some(page), false, Some(page)),
        Verdict::Serve { folder: true }
    );
    assert_eq!(
        decide(page, Some(web), false, Some(page)),
        Verdict::Serve { folder: false }
    );
    assert_eq!(
        decide(page, None, false, Some(web)),
        Verdict::Serve { folder: false }
    );
}
