use super::*;

use core::prelude::v1::test;

#[gpui::test]
fn kanban_save_reload_and_authoritative_removal_preserve_survivors_and_chrome(
    cx: &mut TestAppContext,
) {
    let directory = TestDirectory::new();
    let socket = sheprd_client::session_socket(&directory.0, "default").unwrap();
    let path = endpoint_path(&directory.0, &socket);
    let lane = |repo| order::parse(&format!("github:github.example.test:team/{repo}")).unwrap();
    let (one, two, three) = (lane("one"), lane("two"), lane("three"));
    let roster = vec![one.clone(), two.clone()];
    let mut chrome = Chrome {
        device_order: vec!["test-device".into()],
        kanban_lane_order: order::moved(&roster, &roster, &two, Some(&one)).unwrap(),
        kanban_ranks: crate::orchestrator::view::kanban::rank::Ranks::bounded([(
            one.clone(),
            vec!["7".into(), "3".into()],
        )]),
        notes_width: Some(310.),
        sidebar_width: Some(270.),
        ..Default::default()
    };
    let mut preferences = Preferences::start(Ok(path.clone()));
    await_loaded(&mut preferences);
    preferences.save(chrome.clone());
    let worker = preferences.worker.take().unwrap();
    drop(preferences);
    worker.join().unwrap();
    let reloaded = read_chrome(&path).unwrap();
    assert_eq!(reloaded.kanban_ranks, chrome.kanban_ranks);
    assert_eq!(reloaded, chrome);
    // Temporary disappearance, filters, and caps are not removal evidence.
    let temporary = order::reconcile(&chrome.kanban_lane_order, std::slice::from_ref(&one), false);
    assert_eq!(temporary, chrome.kanban_lane_order);
    chrome.kanban_lane_order = order::reconcile(&temporary, &[one.clone(), three.clone()], true);
    let mut expected = chrome.clone();
    expected.kanban_lane_order = vec![one.clone(), three];
    write_chrome(&path, chrome).unwrap();
    assert_eq!(read_chrome(&path).unwrap(), expected);
    // A late load cannot replace an order already chosen this session.
    let mut stored = order::Stored {
        keys: order::Keys {
            lanes: vec![two.clone(), one.clone()],
            ..Default::default()
        },
        modified: true,
        ..Default::default()
    };
    stored.restore(order::Keys {
        lanes: vec![one.clone(), two.clone()],
        ..Default::default()
    });
    assert_eq!(stored.keys.lanes, vec![two.clone(), one.clone()]);

    let (window, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    // Chrome and Local are ready, but pending/failed discovery hides a remote lane.
    // Even a selection made during discovery is not evidence that hosts loaded.
    for failed in [false, true] {
        write_chrome(&path, expected.clone()).unwrap();
        let mut preferences = Preferences::start(Ok(path.clone()));
        let loaded = await_loaded(&mut preferences);
        let worker = window.update(cx, |window, _| {
            window.sidebar_preferences = Some(preferences);
            window.catalog = crate::endpoint::Catalog::unavailable_for_test(failed);
            window.live.status = crate::state::ConnectionStatus::Connected;
            window.endpoints[0].connection.target = sheprd_client::ConnectTarget::Local;
            window.apply_stored_chrome(Ok(loaded));
            window.kanban_order.keys.lanes = order::reconcile(
                &window.kanban_order.keys.lanes,
                std::slice::from_ref(&one),
                window.kanban_roster_authoritative(1),
            );
            window.save_chrome();
            let mut preferences = window.sidebar_preferences.take().unwrap();
            preferences.worker.take().unwrap()
        });
        worker.join().unwrap();
        assert_eq!(read_chrome(&path).unwrap(), expected);
    }

    // An explicit window shares the normal socket's preferences, but never loads hosts.
    let target = sheprd_client::ConnectTarget::Socket(socket);
    let scoped_path = endpoint_path(&directory.0, &target.socket_path().unwrap());
    assert_eq!(scoped_path, path);
    for roster in [vec![], vec![one.clone(), two.clone()]] {
        write_chrome(&path, expected.clone()).unwrap();
        let mut preferences = Preferences::start(Ok(scoped_path.clone()));
        let loaded = await_loaded(&mut preferences);
        let worker = window.update(cx, |window, _| {
            window.kanban_order = Default::default();
            window.sidebar_preferences = Some(preferences);
            window.catalog = crate::endpoint::Catalog::new(&target);
            window.endpoints[0].connection.target = target.clone();
            window.apply_stored_chrome(Ok(loaded));
            window.kanban_order.keys.lanes = order::reconcile(
                &window.kanban_order.keys.lanes,
                &roster,
                window.kanban_roster_authoritative(0),
            );
            if !roster.is_empty() {
                window.kanban_order.keys.lanes =
                    order::moved(&window.kanban_order.keys.lanes, &roster, &two, Some(&one))
                        .unwrap();
                window.kanban_order.modified = true;
            }
            window.save_chrome();
            let mut preferences = window.sidebar_preferences.take().unwrap();
            preferences.worker.take().unwrap()
        });
        worker.join().unwrap();
        let mut scoped_expected = expected.clone();
        if !roster.is_empty() {
            scoped_expected.kanban_lane_order.insert(0, two.clone());
        }
        assert_eq!(read_chrome(&path).unwrap(), scoped_expected);
    }

    // An invalid initial file is unknown, not empty. Its parent permits replacement.
    let original = b"{\"kanban_lane_order\": [\"recoverable saved order\"],";
    fs::write(&path, original).unwrap();
    let mut preferences = Preferences::start(Ok(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(5);
    let loaded = loop {
        if let Some(loaded) = preferences.loaded() {
            break loaded;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    assert!(matches!(loaded, Err(crate::Error::Json(_))));
    let worker = window.update(cx, |window, _| {
        window.kanban_order = Default::default();
        window.sidebar_preferences = Some(preferences);
        window.apply_stored_chrome(loaded);
        // An automatic save after application must keep the same refusal fence.
        window.save_chrome();
        let mut preferences = window.sidebar_preferences.take().unwrap();
        preferences.worker.take().unwrap()
    });
    worker.join().unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
}
