#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
#[cfg(unix)]
use std::path::Path;

mod answers;
#[cfg(unix)]
mod button;
mod decide;

/// A fake `artifacts` program in `dir`: `--json status` prints `status`,
/// `start` and `off` leave a file of their name beside it. Never the real
/// program.
#[cfg(unix)]
fn fake_program(dir: &Path, status: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(program::NAME);
    let dir = dir.display();
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n\
         '--json status') printf '%s\\n' '{status}' ;;\n\
         start) : > '{dir}/started'; echo 'started: pid 1'; echo 'http://127.0.0.1:6790/browse' ;;\n\
         off) : > '{dir}/stopped'; echo 'off: stopped 1 local server' ;;\n\
         *) echo 'artifacts: unknown' >&2; exit 2 ;;\nesac\n"
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn url(text: &str) -> WebUrl {
    WebUrl::try_from(text).unwrap()
}
