//! A click on a web address in an application that reads the mouse: a plain
//! click is the application's, and a Shift click is the link's.

use super::*;
use gpui::Modifiers;

#[gpui::test]
fn a_plain_click_on_a_link_goes_to_the_application_and_a_shift_click_opens_it(
    cx: &mut gpui::TestAppContext,
) {
    let url = "https://example.com/x";
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.update(cx, |fixture, _| fixture.0.clone());
    let (endpoint, mut server) = connected_endpoint("local:mouse");
    let position = cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            prepare_mouse(view, endpoint, cx);
            let frame = &mut Arc::make_mut(view.live.surface.as_mut().unwrap()).frame;
            frame.cells = (0..frame.height)
                .flat_map(|y| (0..frame.width).map(move |x| (x, y)))
                .map(|(x, y)| CellData {
                    symbol: (y == 4)
                        .then(|| url.chars().nth(usize::from(x).wrapping_sub(1)))
                        .flatten()
                        .map_or_else(|| " ".into(), String::from),
                    fg: 0,
                    bg: 0,
                    modifier: 0,
                    skip: false,
                    hyperlink: None,
                })
                .collect();
            mouse_position(view, 3.5, 4.5)
        })
    });
    let press = |modifiers| MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers,
        click_count: 1,
        ..Default::default()
    };
    let release = |modifiers| MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers,
        click_count: 1,
    };

    // Without a modifier the press and release are the application's.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let none = Modifiers::default();
            assert!(view.terminal_mouse_down(&press(none), window, cx));
            assert!(view.terminal_mouse_up(&release(none), cx));
        });
    });
    // However the client batches them, the program gets the press and then
    // the release.
    let mut sent = Vec::new();
    while sent.len() < 2 {
        let ClientMessage::ClientShellPaneInput { events, .. } = server.receive() else {
            panic!("expected pane input");
        };
        sent.extend(events.into_iter().map(|event| match event {
            ClientPaneInputEvent::Mouse { kind, .. } => kind,
            other => panic!("expected mouse input, got {other:?}"),
        }));
    }
    assert_eq!(
        sent,
        [
            ClientMouseKind::Down(ClientMouseButton::Left),
            ClientMouseKind::Up(ClientMouseButton::Left),
        ]
    );
    assert!(cx.opened_url().is_none());

    // With Shift the press stays here, and the click opens the address.
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(!view.terminal_mouse_down(&press(shift), window, cx));
            let pressed = view.terminal_link_press(position, shift).unwrap();
            assert!(view.activate_terminal_link(&pressed, position, shift, false, window, cx));
        });
    });
    assert_eq!(cx.opened_url().as_deref(), Some(url));
}
