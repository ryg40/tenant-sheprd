#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
#[cfg(unix)]
use std::path::Path;

mod answers;
#[cfg(unix)]
mod button;
mod decide;
mod holding;
#[cfg(unix)]
mod port;

/// A fake `artifacts` program in `dir`: `--json status` prints `status`,
/// `start` and `off` leave a file of their name beside it, one line for each
/// `start`, and each adds its name to `runs`, in order. `start` prints the
/// page of `status`. Never the real program.
#[cfg(unix)]
pub(crate) fn fake_program(dir: &Path, status: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(program::NAME);
    let page = program::parse_status(status.as_bytes())
        .ok()
        .and_then(|status| status.url)
        .map_or_else(
            || "http://127.0.0.1:6790/browse".to_owned(),
            |url| url.as_str().to_owned(),
        );
    let dir = dir.display();
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n\
         '--json status') printf '%s\\n' '{status}' ;;\n\
         start) echo start >> '{dir}/started'; echo start >> '{dir}/runs'; echo 'started: pid 1'; echo '{page}' ;;\n\
         off) : > '{dir}/stopped'; echo off >> '{dir}/runs'; echo 'off: stopped 1 local server' ;;\n\
         *) echo 'artifacts: unknown' >&2; exit 2 ;;\nesac\n"
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn url(text: &str) -> WebUrl {
    WebUrl::try_from(text).unwrap()
}

/// A loopback port nobody listens on now, for a fake server's page: the
/// owner's own server may hold 6790.
#[cfg(unix)]
pub(crate) fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A window with the activity bar and a focused workspace (where pages
/// embed, it takes the page's tab), `program` found and `ports` as its
/// port listing; the center of its Artifacts button.
#[cfg(unix)]
pub(crate) fn clicking(
    cx: &mut gpui::TestAppContext,
    program: PathBuf,
    ports: crate::local_ports::LocalPorts,
) -> (
    gpui::Entity<crate::SheprdWindow>,
    &mut gpui::VisualTestContext,
    gpui::Point<gpui::Pixels>,
) {
    use crate::sidebar::layout_tests::{SidebarFixture, fixture_window, full_draw};
    use gpui::{AppContext as _, px, size};
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            view.local_ports = ports;
            if let Some(snapshot) = &view.live.snapshot {
                let mut snapshot = (**snapshot).clone();
                snapshot.focused_workspace_id = Some("w0".into());
                view.live.snapshot = Some(std::sync::Arc::new(snapshot));
            }
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1000.), px(700.)));
    cx.update(|_, cx| {
        cx.set_global(Launcher {
            program: Some(program),
            job: None,
        })
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    // Without a URL setting, the program alone puts the button on the bar.
    cx.update(|window, cx| {
        window.refresh();
        full_draw(window, cx).clear(cx);
    });
    let button = cx.debug_bounds("activity-artifacts").unwrap().center();
    (view, cx, button)
}

/// Whether the window shows `page`: as the tab it has selected where pages
/// embed, else handed to the system browser.
#[cfg(unix)]
pub(crate) fn shows(
    view: &gpui::Entity<crate::SheprdWindow>,
    cx: &mut gpui::VisualTestContext,
    page: &WebUrl,
) -> bool {
    use crate::browser::{Location, Pick, Store};
    if !crate::browser::EMBEDDED {
        return cx.opened_url().as_deref() == Some(page.as_str());
    }
    cx.update(|_, cx| {
        let view = view.read(cx);
        let shown = match view.active_group().and_then(|group| view.group_pick(group)) {
            Some(Pick::Page(id)) => cx.global::<Store>().get(id),
            _ => None,
        };
        let opened = Some(Location::Web { url: page.clone() });
        shown.map(|tab| &tab.location) == Some(&opened)
    })
}
