//! The installed handlers, in a child process: this test binary run again
//! with only `crashing_child`, which crashes as asked.
use super::*;
use std::process::{Command, Stdio};

const CHILD: &str = "HERDR_GPUI_CRASH_CHILD";
/// An application-defined exception code (bit 29 set), which no system
/// component raises.
#[cfg(windows)]
const TEST_EXCEPTION: u32 = 0xE0DE_AD01;

#[test]
#[ignore = "only crashes when the tests below run it as a child"]
fn crashing_child() {
    let Some(kind) = std::env::var_os(CHILD) else {
        return;
    };
    crate::init(&["sheprd_diagnostics"]).unwrap();
    install("1.2.3");
    match kind.to_str() {
        Some("panic") => panic!("deliberate test panic"),
        Some("contained") => {
            contain("page-", handed_over);
            let page = std::thread::Builder::new()
                .name("page-1".into())
                .spawn(page_panics)
                .unwrap();
            assert!(page.join().is_err());
            let handed = HANDED.lock().unwrap().clone();
            panic!("deliberate test panic after {handed:?}");
        }
        #[cfg(windows)]
        Some("overlap") => {
            // Another thread's dump is still being written when this crash
            // arrives; it ends 300 ms later.
            windows::BUSY.store(true, std::sync::atomic::Ordering::Release);
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                windows::BUSY.store(false, std::sync::atomic::Ordering::Release);
            });
            raise();
        }
        #[cfg(windows)]
        Some("exception") => raise(),
        #[cfg(windows)]
        Some("overflow") => {
            quiet_error_reporting();
            println!("{}", deeper(0));
        }
        _ => {}
    }
}

/// What the contained thread's panic says, as a page's text would: no log
/// line and no output of the child may hold it.
const PRIVATE: &str = "the text of a page";

fn page_panics() {
    panic!("{PRIVATE}");
}

/// The name the hook handed over from a contained thread's panic.
static HANDED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn handed_over(thread: &str) {
    *HANDED.lock().unwrap() = Some(thread.to_owned());
}

/// Raises an exception nothing handles, so the process ends in the
/// unhandled-exception filter.
#[cfg(windows)]
fn raise() {
    quiet_error_reporting();
    // SAFETY: no arguments; the exception is never handled, which
    // is the point: the process ends in the unhandled-exception filter.
    #[allow(unsafe_code)]
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::RaiseException(
            TEST_EXCEPTION,
            0,
            0,
            std::ptr::null(),
        );
    }
}

/// Recurses until the stack runs out; `black_box` keeps each frame.
#[cfg(windows)]
fn deeper(depth: u64) -> u64 {
    if depth == u64::MAX {
        return 0;
    }
    let frame = std::hint::black_box([depth; 64]);
    deeper(depth + 1) + frame[0]
}

/// Windows Error Reporting still runs after the filter; it must not wait on
/// a dialog in a desktop session.
#[cfg(windows)]
fn quiet_error_reporting() {
    use windows_sys::Win32::System::Diagnostics::Debug::{SEM_NOGPFAULTERRORBOX, SetErrorMode};
    // SAFETY: sets a process flag; no pointers.
    #[allow(unsafe_code)]
    unsafe {
        SetErrorMode(SEM_NOGPFAULTERRORBOX);
    }
}

/// Runs `crashing_child` with `kind`; returns its logs directory.
fn crash(kind: &str, state: &Path) -> PathBuf {
    crash_output(kind, state).0
}

/// Runs `crashing_child` with `kind`; returns its logs directory and what
/// it wrote to its standard output and error.
fn crash_output(kind: &str, state: &Path) -> (PathBuf, String) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash::tests::end_to_end::crashing_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env(CHILD, kind)
        .env("XDG_STATE_HOME", state)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!output.status.success(), "the child did not crash");
    let printed = [output.stdout, output.stderr]
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .join("\n");
    (state.join("herdr").join("gpui").join("logs"), printed)
}

fn only(dir: &Path, extension: &str) -> PathBuf {
    let found = files(dir, extension);
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn a_panic_leaves_a_report_and_a_flushed_log_record() {
    let state = tempfile::tempdir().unwrap();
    let logs = crash("panic", state.path());
    let report_path = only(&logs, REPORT_EXTENSION);
    let report = fs::read_to_string(&report_path).unwrap();
    assert!(
        report.contains("message: deliberate test panic\n"),
        "{report}"
    );
    assert!(report.contains("end_to_end.rs"), "{report}");
    assert!(report.contains("\nbacktrace:\n"), "{report}");
    let log = fs::read_to_string(logs.join("herdr-gpui.jsonl")).unwrap();
    let name = report_path.file_name().unwrap().to_str().unwrap();
    let record = log
        .lines()
        .find(|line| line.contains("Sheprd panicked"))
        .unwrap_or_else(|| panic!("no panic record in\n{log}"));
    assert!(record.contains(name) && record.contains("deliberate test panic"));
    #[cfg(windows)]
    assert_eq!(
        &fs::read(only(&logs, DUMP_EXTENSION)).unwrap()[..4],
        b"MDMP"
    );
}

/// A panic on a thread the app contains writes no crash report and logs
/// its thread and place without its message; a panic on any other thread
/// of the same process still leaves its report.
#[test]
fn a_contained_thread_leaves_no_report_and_another_thread_still_does() {
    let state = tempfile::tempdir().unwrap();
    let (logs, printed) = crash_output("contained", state.path());
    // The only report is the other thread's, which says that the hook
    // handed the name over.
    let report = fs::read_to_string(only(&logs, REPORT_EXTENSION)).unwrap();
    assert!(report.contains(r#"after Some("page-1")"#), "{report}");
    let log = fs::read_to_string(logs.join("herdr-gpui.jsonl")).unwrap();
    assert!(!log.contains(PRIVATE), "{log}");
    assert!(!printed.contains(PRIVATE), "{printed}");
    let record = log
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|record| record["fields"]["thread"] == "page-1")
        .unwrap_or_else(|| panic!("no record of the contained thread in\n{log}"));
    let place = format!("{}:", file!());
    let location = record["fields"]["location"].as_str().unwrap_or_default();
    assert!(location.starts_with(&place), "{record}");
}

#[cfg(windows)]
#[test]
fn an_unhandled_exception_leaves_a_dump_and_a_report() {
    let state = tempfile::tempdir().unwrap();
    let logs = crash("exception", state.path());
    assert_eq!(
        &fs::read(only(&logs, DUMP_EXTENSION)).unwrap()[..4],
        b"MDMP"
    );
    let report = fs::read_to_string(only(&logs, REPORT_EXTENSION)).unwrap();
    assert!(
        report.contains(&format!("unhandled exception {TEST_EXCEPTION:#010x}")),
        "{report}"
    );
    let log = fs::read_to_string(logs.join("herdr-gpui.jsonl")).unwrap();
    assert!(log.contains("Sheprd crashed"), "{log}");
}

#[cfg(windows)]
#[test]
fn a_stack_overflow_still_leaves_a_dump() {
    let state = tempfile::tempdir().unwrap();
    let logs = crash("overflow", state.path());
    assert_eq!(
        &fs::read(only(&logs, DUMP_EXTENSION)).unwrap()[..4],
        b"MDMP"
    );
    let report = fs::read_to_string(only(&logs, REPORT_EXTENSION)).unwrap();
    // STATUS_STACK_OVERFLOW
    assert!(
        report.contains("unhandled exception 0xc00000fd"),
        "{report}"
    );
}

#[cfg(windows)]
#[test]
fn a_crash_during_another_dump_still_gets_its_own() {
    let state = tempfile::tempdir().unwrap();
    let logs = crash("overlap", state.path());
    assert_eq!(
        &fs::read(only(&logs, DUMP_EXTENSION)).unwrap()[..4],
        b"MDMP"
    );
    let report = fs::read_to_string(only(&logs, REPORT_EXTENSION)).unwrap();
    assert!(
        report.contains(&format!("unhandled exception {TEST_EXCEPTION:#010x}")),
        "{report}"
    );
}
