//! Serves a local page's folder through the private preview scheme, with
//! markdown files rendered as pages. The platform asks on the UI thread, and
//! Servo on its own thread, so each request is queued for a worker that reads
//! the file; nothing here touches the disk on the thread that asked.
use super::{location::requested_file, markdown};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender, TrySendError},
    thread,
};
#[cfg(any(target_os = "macos", windows))]
use wry::http;

/// Requests waiting for the worker; more are answered 503.
const QUEUE: usize = 256;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Origins that belong to the preview scheme itself.
#[cfg(any(target_os = "macos", windows))]
const PREVIEW_ORIGINS: [&str; 3] = [
    "herdr-preview://localhost",
    "http://herdr-preview.localhost",
    "https://herdr-preview.localhost",
];

/// What the scheme answers one request with.
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
}

impl Answer {
    fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
        }
    }

    /// Nothing to say, as to a message the page only posted.
    #[cfg(servo_pages)]
    pub(crate) fn empty() -> Self {
        Self::new(204, "text/plain", Vec::new())
    }

    fn refused(status: u16) -> Self {
        Self::new(status, "text/plain", Vec::new())
    }

    /// The headers every answer carries, on either backend: its type, never
    /// cached, and never read as another type.
    // i18n: data
    pub(crate) fn headers(&self) -> [(&'static str, &'static str); 3] {
        [
            ("content-type", self.content_type),
            ("cache-control", "no-store"),
            ("x-content-type-options", "nosniff"),
        ]
    }
}

/// Hands an answer to the page that asked, from the worker's thread.
type Responder = Box<dyn FnOnce(Answer) + Send>;

struct Job {
    root: PathBuf,
    path: String,
    /// The window's theme, for a Markdown file's page; empty for any other
    /// request, which has no use for it.
    theme: String,
    responder: Responder,
}

pub(crate) struct Preview {
    jobs: SyncSender<Job>,
    /// The kit's `theme.css`, following the window's theme.
    theme: RefCell<String>,
}

fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("wasm") => "application/wasm",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Reads `path` under `root`, refusing anything a link leads out of the
/// folder, anything that is not a regular file, and anything too large.
fn read(root: &Path, request_path: &str, theme: &str) -> Result<(Vec<u8>, &'static str), u16> {
    let file = requested_file(root, request_path).ok_or(404_u16)?;
    let root = root.canonicalize().map_err(|_| 404_u16)?;
    let resolved = file.canonicalize().map_err(|_| 404_u16)?;
    if !resolved.starts_with(&root) {
        return Err(403);
    }
    let metadata = resolved.metadata().map_err(|_| 404_u16)?;
    if !metadata.is_file() {
        return Err(404);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(413);
    }
    let bytes = std::fs::read(&resolved).map_err(|_| 404_u16)?;
    if markdown::is_markdown(&resolved) {
        let name = resolved
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let page = markdown::page(&String::from_utf8_lossy(&bytes), name, theme);
        return Ok((page.into_bytes(), "text/html; charset=utf-8"));
    }
    Ok((bytes, content_type(&resolved)))
}

/// Whether a request comes from a web page rather than from the local page
/// itself or a navigation: a site must not load the folder's files. Servo's
/// requests carry no such headers when they are asked about; see
/// `policy::preview_refused`.
#[cfg(any(target_os = "macos", windows))]
pub(super) fn from_the_web(request: &http::Request<Vec<u8>>) -> bool {
    [http::header::ORIGIN, http::header::REFERER]
        .into_iter()
        .filter_map(|name| request.headers().get(name))
        .filter_map(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && *value != "null")
        .any(|value| {
            !PREVIEW_ORIGINS.iter().any(|origin| {
                value
                    .strip_prefix(origin)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
            })
        })
}

impl Preview {
    pub(crate) fn start() -> std::io::Result<Self> {
        let (jobs, queue) = mpsc::sync_channel::<Job>(QUEUE);
        // Ends when the window's pages, and with them the sender, are gone.
        thread::Builder::new()
            .name("browser-preview".into())
            .spawn(move || {
                for job in queue {
                    (job.responder)(match read(&job.root, &job.path, &job.theme) {
                        Ok((bytes, kind)) => Answer::new(200, kind, bytes),
                        Err(status) => Answer::refused(status),
                    });
                }
            })?;
        Ok(Self {
            jobs,
            theme: RefCell::default(),
        })
    }

    pub(crate) fn set_theme(&self, css: String) {
        *self.theme.borrow_mut() = css;
    }

    /// Answers one request for `path` from a page. `root` is the page's
    /// folder, or `None` for a web page or a request a web page made, which
    /// the scheme serves nothing to. `responder` may run on the worker.
    pub(crate) fn handle(
        &self,
        root: Option<&Path>,
        path: &str,
        responder: impl FnOnce(Answer) + Send + 'static,
    ) {
        let Some(root) = root else {
            responder(Answer::refused(403));
            return;
        };
        // The kit is in memory, so it is answered here without the worker.
        if let Some((body, kind)) = super::kit::asset(path, &self.theme.borrow()) {
            responder(Answer::new(200, kind, body));
            return;
        }
        let theme = if markdown::is_markdown(Path::new(path.split('?').next().unwrap_or(path))) {
            self.theme.borrow().clone()
        } else {
            String::new()
        };
        let job = Job {
            root: root.to_owned(),
            path: path.to_owned(),
            theme,
            responder: Box::new(responder),
        };
        if let Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) =
            self.jobs.try_send(job)
        {
            (job.responder)(Answer::refused(503));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    mod markdown_files;

    #[test]
    fn files_are_read_only_from_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        std::fs::create_dir_all(site.join("css")).unwrap();
        std::fs::write(site.join("index.html"), "<h1>hi</h1>").unwrap();
        std::fs::write(site.join("css/app.css"), "h1{}").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let (bytes, kind) = read(&site, "/", "").unwrap();
        assert_eq!(
            (bytes.as_slice(), kind),
            (&b"<h1>hi</h1>"[..], "text/html; charset=utf-8")
        );
        assert_eq!(
            read(&site, "/css/app.css", "").unwrap().1,
            "text/css; charset=utf-8"
        );
        assert_eq!(read(&site, "/missing.html", ""), Err(404));
        assert_eq!(read(&site, "/../secret.txt", ""), Err(404));
        assert_eq!(read(&site, "/css", ""), Err(404));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("secret.txt"), site.join("link.txt"))
                .unwrap();
            assert_eq!(read(&site, "/link.txt", ""), Err(403));
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn web_pages_cannot_load_local_files() {
        let request = |header: Option<(&str, &str)>| {
            let mut builder = http::Request::builder().uri("herdr-preview://localhost/a.js");
            if let Some((name, value)) = header {
                builder = builder.header(name, value);
            }
            builder.body(Vec::new()).unwrap()
        };
        assert!(!from_the_web(&request(None)));
        assert!(!from_the_web(&request(Some((
            "Referer",
            "herdr-preview://localhost/index.html"
        )))));
        assert!(!from_the_web(&request(Some((
            "Origin",
            "http://herdr-preview.localhost"
        )))));
        assert!(!from_the_web(&request(Some(("Origin", "null")))));
        assert!(from_the_web(&request(Some((
            "Referer",
            "https://evil.test/"
        )))));
        assert!(from_the_web(&request(Some((
            "Origin",
            "https://evil.test"
        )))));
        assert!(from_the_web(&request(Some((
            "Origin",
            "http://herdr-preview.localhost.evil.test"
        )))));
    }
}
