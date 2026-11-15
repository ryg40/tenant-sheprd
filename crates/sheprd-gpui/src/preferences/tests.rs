#![allow(clippy::unwrap_used)]
use super::*;
use core::prelude::v1::test;
use std::env;
use std::time::{Duration, Instant};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        loop {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "herdr-preferences-test-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("Cannot create test directory: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn await_loaded(preferences: &mut Preferences) -> Chrome {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(chrome) = preferences.loaded() {
            assert!(preferences.loaded().is_none());
            return chrome.unwrap();
        }
        assert!(Instant::now() < deadline, "preferences load timed out");
        thread::sleep(Duration::from_millis(1));
    }
}

#[core::prelude::v1::test]
fn roundtrip_and_reset_drain_after_drop() {
    let directory = TestDirectory::new();
    let path = endpoint_path(&directory.0, Path::new("/tmp/test.sock"));
    let mut preferences = Preferences::start(Ok(path.clone()));
    assert_eq!(await_loaded(&mut preferences), Chrome::default());
    for width in 1..=100 {
        preferences.save(Chrome {
            activity_last_opened: Default::default(),
            activity_panel: None,
            agent_sort: Some(AgentSort::Priority),
            device_order: Vec::new(),
            kanban_lane_order: Vec::new(),
            kanban_ranks: Default::default(),
            mod_panel_width: None,
            notes_width: None,
            orchestrator_repos: None,
            review_files_width: None,
            sidebar_split: Some(0.4),
            sidebar_width: Some(width as f32),
            walkr_dismissed: Default::default(),
        });
    }
    let worker = preferences.worker.take().unwrap();
    drop(preferences);
    // Only the test waits for persistence before reading or removing files.
    worker.join().unwrap();
    let mut preferences = Preferences::start(Ok(path.clone()));
    assert_eq!(
        await_loaded(&mut preferences),
        Chrome {
            activity_last_opened: Default::default(),
            activity_panel: None,
            agent_sort: Some(AgentSort::Priority),
            device_order: Vec::new(),
            kanban_lane_order: Vec::new(),
            kanban_ranks: Default::default(),
            mod_panel_width: None,
            notes_width: None,
            orchestrator_repos: None,
            review_files_width: None,
            sidebar_split: Some(0.4),
            sidebar_width: Some(100.0),
            walkr_dismissed: Default::default(),
        }
    );
    preferences.save(Chrome::default());
    let worker = preferences.worker.take().unwrap();
    drop(preferences);
    worker.join().unwrap();
    assert_eq!(read_chrome(&path).unwrap(), Chrome::default());
    let mut preferences = Preferences::start(Ok(path));
    assert_eq!(await_loaded(&mut preferences), Chrome::default());
}

#[core::prelude::v1::test]
fn only_a_toggled_sort_is_stored_and_older_files_migrate() {
    let directory = TestDirectory::new();
    let path = endpoint_path(&directory.0, Path::new("/tmp/sort.sock"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let chrome = |agent_sort| Chrome {
        activity_last_opened: Default::default(),
        activity_panel: None,
        agent_sort,
        device_order: Vec::new(),
        kanban_lane_order: Vec::new(),
        kanban_ranks: Default::default(),
        mod_panel_width: None,
        notes_width: None,
        orchestrator_repos: None,
        review_files_width: None,
        sidebar_split: None,
        sidebar_width: Some(240.0),
        walkr_dismissed: Default::default(),
    };
    for (json, expected) in [
        // A file from before the sort existed follows the daemon.
        (r#"{"sidebar_width_px": 240.0}"#, None),
        // Earlier builds wrote their default whether or not it was chosen,
        // so only a stored "priority" is known to be a toggle.
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort": "grouped"}"#,
            None,
        ),
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort": "priority"}"#,
            Some(AgentSort::Priority),
        ),
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort": "sideways"}"#,
            None,
        ),
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort_manual": null}"#,
            None,
        ),
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort_manual": "grouped"}"#,
            Some(AgentSort::Grouped),
        ),
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort_manual": "spaces"}"#,
            None,
        ),
        (
            r#"{"sidebar_width_px": 240.0, "agent_sort_manual": 1}"#,
            None,
        ),
    ] {
        fs::write(&path, json).unwrap();
        assert_eq!(read_chrome(&path).unwrap(), chrome(expected), "{json}");
    }
    for agent_sort in [None, Some(AgentSort::Grouped), Some(AgentSort::Priority)] {
        write_chrome(&path, chrome(agent_sort)).unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // The legacy key is no longer written, so it cannot pin a default.
        assert!(stored.get("agent_sort").is_none(), "{stored}");
        assert_eq!(read_chrome(&path).unwrap(), chrome(agent_sort));
    }
}

#[core::prelude::v1::test]
fn drop_does_not_wait_for_blocked_worker_and_queued_saves_still_drain() {
    let (saves, requests) = mpsc::channel();
    let (ready_tx, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let (drained_tx, drained) = mpsc::channel();
    let worker = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        blocked.recv().unwrap();
        drained_tx
            .send(
                requests
                    .into_iter()
                    .map(|save: Save| save.chrome)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    });
    let preferences = Preferences {
        saves: Some(saves),
        revision: AtomicU64::new(0),
        outcome: Arc::default(),
        loaded: None,
        worker: Some(worker),
    };
    let queued = [
        Chrome {
            activity_last_opened: Default::default(),
            activity_panel: None,
            agent_sort: Some(AgentSort::Grouped),
            device_order: Vec::new(),
            kanban_lane_order: Vec::new(),
            kanban_ranks: Default::default(),
            mod_panel_width: None,
            notes_width: None,
            orchestrator_repos: None,
            review_files_width: None,
            sidebar_split: None,
            sidebar_width: Some(160.),
            walkr_dismissed: Default::default(),
        },
        Chrome {
            activity_last_opened: Default::default(),
            activity_panel: None,
            agent_sort: Some(AgentSort::Priority),
            device_order: Vec::new(),
            kanban_lane_order: Vec::new(),
            kanban_ranks: Default::default(),
            mod_panel_width: None,
            notes_width: Some(420.),
            orchestrator_repos: None,
            review_files_width: None,
            sidebar_split: Some(0.6),
            sidebar_width: Some(400.),
            walkr_dismissed: Default::default(),
        },
        Chrome::default(),
    ];
    for chrome in queued.clone() {
        preferences.save(chrome);
    }
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let (dropped_tx, dropped) = mpsc::channel();
    let dropper = thread::spawn(move || {
        drop(preferences);
        dropped_tx.send(()).unwrap();
    });
    let result = dropped.recv_timeout(Duration::from_secs(5));
    // Release even on failure so a regressed join does not strand the threads.
    release.send(()).unwrap();
    let saved = drained.recv_timeout(Duration::from_secs(5)).unwrap();
    dropper.join().unwrap();
    assert!(result.is_ok(), "drop waited for the blocked worker");
    assert_eq!(saved, queued);
}

#[gpui::test]
fn malformed_and_invalid_widths_fall_back_to_default(cx: &mut TestAppContext) {
    let directory = TestDirectory::new();
    let path = directory.0.join("preferences.json");
    let (window, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let await_result = |preferences: &mut Preferences| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(loaded) = preferences.loaded() {
                assert!(preferences.loaded().is_none());
                break loaded;
            }
            assert!(Instant::now() < deadline, "preferences load timed out");
            thread::sleep(Duration::from_millis(1));
        }
    };
    let mut apply_failed = |preferences, loaded| {
        let worker = window.update(cx, |window, _| {
            window.sidebar_preferences = Some(preferences);
            window.apply_stored_chrome(loaded);
            assert_eq!(window.chrome(), Chrome::default());
            // Using defaults in memory must not authorize an automatic disk replacement.
            window.save_chrome();
            let mut preferences = window.sidebar_preferences.take().unwrap();
            preferences.worker.take().unwrap()
        });
        worker.join().unwrap();
    };
    for contents in [
        "not json",
        "[]",
        "null",
        r#"{"sidebar_width_px":0}"#,
        r#"{"sidebar_width_px":-1}"#,
        r#"{"sidebar_width_px":"200"}"#,
        r#"{"sidebar_width_px":true}"#,
        r#"{"sidebar_width_px":1e100}"#,
        r#"{"sidebar_width_px":1e-100}"#,
        r#"{"sidebar_width_px":NaN}"#,
    ] {
        fs::write(&path, contents).unwrap();
        assert!(read_chrome(&path).is_err(), "accepted {contents}");
        let mut preferences = Preferences::start(Ok(path.clone()));
        let loaded = await_result(&mut preferences);
        match contents {
            "not json" | r#"{"sidebar_width_px":NaN}"# => {
                assert!(matches!(loaded, Err(crate::Error::Json(_))));
            }
            "[]" | "null" => {
                assert!(matches!(
                    loaded,
                    Err(crate::Error::Settings(
                        crate::config::Error::PreferencesNotObject
                    ))
                ));
            }
            _ => assert!(matches!(loaded, Err(crate::Error::InvalidStoredWidth))),
        }
        apply_failed(preferences, loaded);
        assert_eq!(fs::read(&path).unwrap(), contents.as_bytes());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let original = br#"{"sidebar_width_px":275}"#;
        fs::write(&path, original).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o200)).unwrap();
        let mut preferences = Preferences::start(Ok(path.clone()));
        let loaded = await_result(&mut preferences);
        // A privileged test process can read write-only files.
        if let Err(crate::Error::Io(ref error)) = loaded {
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            apply_failed(preferences, loaded);
        } else {
            assert_eq!(loaded.unwrap().sidebar_width, Some(275.));
            let worker = preferences.worker.take().unwrap();
            drop(preferences);
            worker.join().unwrap();
        }
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    for contents in ["{}", r#"{"sidebar_width_px":null}"#] {
        fs::write(&path, contents).unwrap();
        assert_eq!(read_chrome(&path).unwrap(), Chrome::default());
    }
    for width in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0] {
        assert!(
            write_chrome(
                &path,
                Chrome {
                    activity_last_opened: Default::default(),
                    activity_panel: None,
                    agent_sort: None,
                    device_order: Vec::new(),
                    kanban_lane_order: Vec::new(),
                    kanban_ranks: Default::default(),
                    mod_panel_width: None,
                    notes_width: None,
                    orchestrator_repos: None,
                    review_files_width: None,
                    sidebar_split: None,
                    sidebar_width: Some(width),
                    walkr_dismissed: Default::default(),
                }
            )
            .is_err()
        );
    }
    let chrome = Chrome {
        activity_last_opened: Default::default(),
        activity_panel: None,
        agent_sort: None,
        device_order: Vec::new(),
        kanban_lane_order: Vec::new(),
        kanban_ranks: Default::default(),
        mod_panel_width: None,
        notes_width: None,
        orchestrator_repos: None,
        review_files_width: None,
        sidebar_split: None,
        sidebar_width: Some(237.5),
        walkr_dismissed: Default::default(),
    };
    write_chrome(&path, chrome.clone()).unwrap();
    assert_eq!(read_chrome(&path).unwrap(), chrome);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[core::prelude::v1::test]
fn invalid_sidebar_splits_preserve_other_preferences() {
    let directory = TestDirectory::new();
    let path = directory.0.join("preferences.json");
    let expected = Chrome {
        activity_last_opened: Default::default(),
        activity_panel: None,
        agent_sort: Some(AgentSort::Priority),
        device_order: Vec::new(),
        kanban_lane_order: Vec::new(),
        kanban_ranks: Default::default(),
        mod_panel_width: None,
        notes_width: None,
        orchestrator_repos: None,
        review_files_width: None,
        sidebar_split: None,
        sidebar_width: Some(240.0),
        walkr_dismissed: Default::default(),
    };
    for split in [
        "null", "0", "-1", "0.099", "0.901", "1e100", "1e-100", "\"0.5\"", "true", "[]", "{}",
    ] {
        fs::write(
            &path,
            format!(
                r#"{{"sidebar_width_px":240,"agent_sort":"priority","sidebar_split":{split}}}"#
            ),
        )
        .unwrap();
        assert_eq!(read_chrome(&path).unwrap(), expected, "{split}");
    }
    for split in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        0.0,
        0.099,
        0.901,
    ] {
        write_chrome(
            &path,
            Chrome {
                sidebar_split: Some(split),
                ..expected.clone()
            },
        )
        .unwrap();
        assert_eq!(read_chrome(&path).unwrap(), expected, "{split}");
    }
}

#[core::prelude::v1::test]
fn sidebar_split_roundtrips_including_boundaries_and_reset() {
    let directory = TestDirectory::new();
    let path = directory.0.join("preferences.json");
    for sidebar_split in [Some(0.1), Some(0.4), Some(0.9), None] {
        let chrome = Chrome {
            activity_last_opened: Default::default(),
            activity_panel: None,
            agent_sort: Some(AgentSort::Priority),
            device_order: Vec::new(),
            kanban_lane_order: Vec::new(),
            kanban_ranks: Default::default(),
            mod_panel_width: None,
            notes_width: None,
            orchestrator_repos: None,
            review_files_width: None,
            sidebar_split,
            sidebar_width: Some(240.0),
            walkr_dismissed: Default::default(),
        };
        write_chrome(&path, chrome.clone()).unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["sidebar_split"], serde_json::json!(sidebar_split));
        assert_eq!(read_chrome(&path).unwrap(), chrome);
    }
}

#[core::prelude::v1::test]
fn endpoint_paths_use_stable_fnv1a() {
    let root = Path::new("/state/herdr/gpui");
    assert_eq!(
        endpoint_path(root, Path::new("hello")),
        Path::new("/state/herdr/gpui/local-a430d84680aabd0b.json")
    );
    assert_eq!(
        endpoint_path(root, Path::new("")),
        Path::new("/state/herdr/gpui/local-cbf29ce484222325.json")
    );
    assert_ne!(
        endpoint_path(root, Path::new("/a.sock")),
        endpoint_path(root, Path::new("/b.sock"))
    );
}

mod activity_last_opened;
mod activity_panel;
mod device_order;
mod notes_width;
mod walkr_dismissed;

mod kanban_order;
