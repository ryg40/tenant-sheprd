//! A page Servo draws, on Linux: the window's side of it, and the element
//! that paints its latest frame where its group draws.
//!
//! Servo draws offscreen, on its own thread ([`crate::browser::engine`]), and
//! hands each frame over as an image. The window paints that image itself,
//! so its menus, toasts, and tooltips draw over a page as over anything
//! else. One rectangle, on whole device pixels, is where the image is
//! painted, where the pointer is tested, and what the page is sized to.
use {
    super::{Drawn, snap},
    crate::browser::{
        TabId, Zoom,
        engine::{self, Command, Engine, Latest, Mailbox, PageKey, keys},
        reports::{Event, Source, report},
    },
    gpui::{
        App, Bounds, Corners, CursorStyle, DispatchPhase, Div, Element, ElementId, FocusHandle,
        GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement,
        IntoElement, KeyDownEvent, KeyUpEvent, LayoutId, MouseButton, MouseDownEvent,
        MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, RenderImage, ScrollDelta,
        ScrollEvent, Size, Style, Styled, Window, div, px, size,
    },
    servo::{
        DevicePoint, InputEvent, MouseButton as PageButton, MouseButtonAction, MouseButtonEvent,
        MouseLeftViewportEvent, MouseMoveEvent as PageMove, WebViewPoint, WheelDelta, WheelEvent,
        WheelMode,
    },
    std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::{Arc, mpsc::SyncSender},
    },
};

mod select;

pub(crate) use select::Colors;

/// Points one line of a wheel scrolls, for mice that count lines.
const LINE: f32 = 38.;

/// Images no page shows any more, whose textures the window frees when it
/// next can.
pub(crate) type Retired = Rc<RefCell<Vec<Arc<RenderImage>>>>;

/// The frame a page shows, and the points it covers.
struct Shown {
    image: Arc<RenderImage>,
    size: Size<Pixels>,
}

/// One tab's page: the window's handle to a `WebView` on the engine's thread.
pub(crate) struct Page {
    key: PageKey,
    engine: Engine,
    mailbox: Arc<Mailbox>,
    /// The pointer's place and the page's size, which the engine reads the
    /// latest of.
    latest: Arc<Latest>,
    /// Where the page says that the engine is gone.
    reports: SyncSender<Event<Source>>,
    source: Source,
    /// Keys reach the page while this is the window's focus.
    focus: FocusHandle,
    /// Read as the page lays out, after the window shows or hides it.
    pub(super) shown: Cell<bool>,
    /// Whether the page was told it has the keyboard.
    focused: Cell<bool>,
    /// Asked for the keyboard from outside a frame; taken as it next paints.
    wants_focus: Cell<bool>,
    /// Whether the pointer was over the page when it last moved.
    hovered: Cell<bool>,
    /// The buttons pressed over the page and not yet released, which the
    /// page hears the release of wherever the pointer is by then.
    pressed: RefCell<Vec<MouseButton>>,
    zoom: Cell<Zoom>,
    /// The device size and scale the engine was last sent.
    sized: Cell<Option<(u32, u32, f32)>>,
    frame: RefCell<Option<Shown>>,
    retired: Retired,
    /// The list of the page's open `<select>`, if it has one.
    select: select::Select,
}

impl Page {
    pub(in crate::browser) fn new(
        key: PageKey,
        engine: Engine,
        mailbox: Arc<Mailbox>,
        latest: Arc<Latest>,
        (reports, source): (SyncSender<Event<Source>>, Source),
        focus: FocusHandle,
        retired: Retired,
    ) -> Self {
        Self {
            key,
            engine,
            mailbox,
            latest,
            reports,
            source,
            focus,
            shown: Cell::new(false),
            focused: Cell::new(false),
            wants_focus: Cell::new(false),
            hovered: Cell::new(false),
            pressed: RefCell::default(),
            zoom: Cell::new(Zoom::default()),
            sized: Cell::new(None),
            frame: RefCell::new(None),
            retired,
            select: select::Select::default(),
        }
    }

    /// Queues a command for the page. Whether the engine took it: a
    /// dropped one is logged, and an engine that is gone is reported to the
    /// window, once for the app.
    pub(in crate::browser) fn send(&self, command: impl FnOnce(PageKey) -> Command) -> bool {
        match self.engine.send(command(self.key)) {
            Ok(()) => true,
            Err(engine::Error::Stopped) => {
                if self.engine.report_end() {
                    tracing::warn!("The browser engine's thread ended");
                    let failed = Event::Failed(self.source, engine::Error::Stopped);
                    report(&self.reports, failed);
                }
                false
            }
            Err(error) => {
                tracing::debug!(%error, "Dropped a browser engine command");
                false
            }
        }
    }

    /// Hands the page a press, a key, or a wheel turn, after the pointer's
    /// place if one still waits, so the page hears them in order.
    fn input(&self, event: InputEvent) {
        if let Some((x, y)) = self.latest.take_moved() {
            let point = WebViewPoint::Device(DevicePoint::new(x, y));
            let moved = InputEvent::MouseMove(PageMove::new(point));
            self.send(|key| Command::Input(key, Box::new(moved)));
        }
        self.send(|key| Command::Input(key, Box::new(event)));
    }

    /// Tells the page where the pointer is. Only the latest place waits for
    /// the engine, however fast the pointer moves.
    fn moved(&self, x: f32, y: f32) {
        if self.latest.put_moved((x, y)) && !self.send(Command::Moved) {
            // Nothing queued will read the place: the next move asks again.
            self.latest.take_moved();
        }
    }

    pub(crate) fn show(&self) {
        if !self.shown.replace(true) {
            self.send(|key| Command::Shown(key, true));
        }
    }

    /// Hides the page, first taking the keyboard back from it. Servo stops
    /// drawing a hidden page.
    pub(crate) fn hide(&self) {
        self.blur();
        if self.shown.replace(false) {
            self.send(|key| Command::Shown(key, false));
        }
    }

    /// Has the page report light, or dark, as its `prefers-color-scheme`.
    pub(crate) fn set_light(&self, light: bool) {
        self.send(|key| Command::Light(key, light));
    }

    pub(crate) fn zoom(&self, zoom: Zoom) {
        if self.zoom.replace(zoom) != zoom {
            // A list is placed for the zoom its page had.
            self.dismiss_list();
            // Servo keeps zoom as f32.
            #[expect(clippy::cast_possible_truncation, reason = "a zoom factor is small")]
            let factor = zoom.factor() as f32;
            self.send(|key| Command::Zoom(key, factor));
        }
    }

    /// Moves keyboard input into the page, as it next paints.
    pub(crate) fn focus(&self) {
        self.wants_focus.set(true);
    }

    pub(crate) fn holds_keyboard(&self) -> bool {
        self.focused.get()
    }

    /// Tells the page it lost the keyboard. The window's focus goes
    /// wherever the press or the command that took it puts it.
    pub(crate) fn blur(&self) {
        self.dismiss_list();
        if self.focused.replace(false) {
            self.send(|key| Command::Focused(key, false));
        }
    }

    fn take_keyboard(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
        if !self.focused.replace(true) {
            self.send(|key| Command::Focused(key, true));
        }
    }

    /// Sizes the page to `bounds` at `scale`: only a change reaches Servo.
    fn resize(&self, bounds: Bounds<Pixels>, scale: f32) {
        let (Some(width), Some(height)) = (
            device(bounds.size.width, scale),
            device(bounds.size.height, scale),
        ) else {
            return;
        };
        let sized = Some((width, height, scale));
        if self.sized.get() == sized {
            return;
        }
        // A list is placed for the size its page had.
        self.dismiss_list();
        if self.latest.put_sized((width, height, scale)) && !self.send(Command::Sized) {
            // Nothing queued will read the size: the next frame asks again.
            self.latest.take_sized();
            return;
        }
        self.sized.set(sized);
    }

    /// Takes the engine's latest frame, if it drew one since the last paint.
    fn refresh(&self) {
        let Some(frame) = self.mailbox.take() else {
            return;
        };
        if !frame.scale.is_finite() || frame.scale <= 0. {
            return;
        }
        let size = size(
            px(frame.width as f32 / frame.scale),
            px(frame.height as f32 / frame.scale),
        );
        let Some(pixels) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra) else {
            return;
        };
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
        if let Some(old) = self.frame.replace(Some(Shown { image, size })) {
            self.retired.borrow_mut().push(old.image);
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        if let Some(old) = self.frame.take() {
            self.retired.borrow_mut().push(old.image);
        }
        self.send(Command::Close);
    }
}

/// `length` in whole device pixels at `scale`, if it has any.
fn device(length: Pixels, scale: f32) -> Option<u32> {
    let pixels = (f32::from(length) * scale).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked to be a small positive whole number"
    )]
    (pixels.is_finite() && (1. ..=16384.).contains(&pixels)).then_some(pixels as u32)
}

/// `position` in the page's device pixels, the page drawn at `bounds`.
fn device_point(position: Point<Pixels>, bounds: Bounds<Pixels>, scale: f32) -> (f32, f32) {
    (
        f32::from(position.x - bounds.origin.x) * scale,
        f32::from(position.y - bounds.origin.y) * scale,
    )
}

fn page_point(position: Point<Pixels>, bounds: Bounds<Pixels>, scale: f32) -> WebViewPoint {
    let (x, y) = device_point(position, bounds, scale);
    WebViewPoint::Device(DevicePoint::new(x, y))
}

fn page_button(button: MouseButton) -> PageButton {
    match button {
        MouseButton::Left => PageButton::Primary,
        MouseButton::Right => PageButton::Secondary,
        MouseButton::Middle => PageButton::Auxiliary,
        MouseButton::Navigate(gpui::NavigationDirection::Back) => PageButton::Back,
        MouseButton::Navigate(gpui::NavigationDirection::Forward) => PageButton::Forward,
    }
}

/// The element a page lays out in: it fills its space, takes the keys
/// while the page has the keyboard, and paints the page's latest frame.
pub(crate) struct PageElement {
    id: TabId,
    page: Rc<Page>,
    drawn: Drawn,
    colors: Colors,
}

impl PageElement {
    pub(crate) fn new(id: TabId, page: Rc<Page>, drawn: Drawn) -> Self {
        Self {
            id,
            page,
            drawn,
            colors: Colors::default(),
        }
    }

    /// Draws the page's own lists in `colors`.
    pub(crate) fn themed(mut self, colors: Colors) -> Self {
        self.colors = colors;
        self
    }
}

impl IntoElement for PageElement {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        let (down, up) = (self.page.clone(), self.page.clone());
        let list = select::element(&self.page, self.colors);
        div()
            .size_full()
            .flex_shrink(1.)
            .track_focus(&self.page.focus)
            // i18n: data
            .key_context("BrowserPage")
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if down.focused.get() {
                    if !down.list_key(event, window) {
                        down.keyed();
                        down.input(keys::down(event));
                    }
                    cx.stop_propagation();
                }
            })
            .on_key_up(move |event: &KeyUpEvent, _, cx| {
                if up.focused.get() {
                    if !up.list_key_up(event) {
                        up.input(keys::up(event));
                    }
                    cx.stop_propagation();
                }
            })
            .child(Surface {
                id: self.id,
                page: self.page,
                drawn: self.drawn,
            })
            .children(list)
    }
}

/// Where the page's frame is painted and its pointer events are read.
struct Surface {
    id: TabId,
    page: Rc<Page>,
    drawn: Drawn,
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: Size::full(),
            flex_shrink: 1.,
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        // Whole device pixels: a frame then fills its rectangle one pixel
        // of the page to one of the screen.
        let scale = window.scale_factor();
        let bounds = snap(bounds, scale);
        let visible = snap(bounds.intersect(&window.content_mask().bounds), scale);
        self.drawn.borrow_mut().insert(self.id, visible);
        if !self.page.shown.get() {
            return None;
        }
        self.page.resize(bounds, scale);
        Some(window.insert_hitbox(bounds, HitboxBehavior::Normal))
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(hitbox) = hitbox.take() else {
            return;
        };
        let (page, area, scale) = (&self.page, hitbox.bounds, window.scale_factor());
        if page.wants_focus.take() {
            page.take_keyboard(window, cx);
        }
        // The window's focus left by another way than a press, as a command
        // does: the page is told, and its caret stops.
        if page.focused.get() && !page.focus.is_focused(window) {
            page.blur();
        }
        page.refresh();
        page.settle_list();
        if let Some(shown) = page.frame.borrow().as_ref() {
            // A frame drawn for another size, as while the group is
            // resized, keeps its own scale from the corner rather than
            // stretch; the rectangle clips it.
            let image = Bounds::new(area.origin, shown.size);
            if let Err(error) = window.paint_image(
                area,
                image,
                Corners::default(),
                shown.image.clone(),
                0,
                false,
            ) {
                tracing::debug!(%error, "Could not paint a page's frame");
            }
        }
        window.set_cursor_style(CursorStyle::Arrow, &hitbox);
        self.listen(hitbox, area, scale, window);
    }
}

impl Surface {
    /// Hands the pointer's events over the page to it, in its own device
    /// pixels, through the rectangle its frame was just painted in.
    fn listen(&self, hitbox: Hitbox, area: Bounds<Pixels>, scale: f32, window: &mut Window) {
        let (page, over) = (self.page.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let closed = page.closed_list(event.position, event.button);
            if !over.is_hovered(window) {
                // A press elsewhere in the window takes the keyboard back.
                if !area.contains(&event.position) {
                    page.blur();
                }
                return;
            }
            if closed {
                cx.stop_propagation();
                return;
            }
            page.take_keyboard(window, cx);
            page.pressed_at(event.position - area.origin);
            if !page.pressed.borrow().contains(&event.button) {
                page.pressed.borrow_mut().push(event.button);
            }
            page.input(InputEvent::MouseButton(MouseButtonEvent::new(
                MouseButtonAction::Down,
                page_button(event.button),
                page_point(event.position, area, scale),
            )));
            cx.stop_propagation();
        });
        let (page, over) = (self.page.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble || page.released_list(event.button) {
                return;
            }
            // A drag that began in the page ends for it wherever the button
            // is released; Servo would keep the button down otherwise.
            let pressed = {
                let mut pressed = page.pressed.borrow_mut();
                let before = pressed.len();
                pressed.retain(|button| *button != event.button);
                pressed.len() != before
            };
            if pressed || over.is_hovered(window) {
                page.input(InputEvent::MouseButton(MouseButtonEvent::new(
                    MouseButtonAction::Up,
                    page_button(event.button),
                    page_point(event.position, area, scale),
                )));
            }
        });
        let (page, over) = (self.page.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let inside = over.is_hovered(window);
            if inside {
                let (x, y) = device_point(event.position, area, scale);
                page.moved(x, y);
            } else if page.hovered.get() {
                page.input(InputEvent::MouseLeftViewport(MouseLeftViewportEvent {
                    focus_moving_to_another_iframe: false,
                }));
            }
            page.hovered.set(inside);
        });
        let page = self.page.clone();
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let (x, y) = match event.delta {
                ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
                ScrollDelta::Lines(delta) => (delta.x * LINE, delta.y * LINE),
            };
            let delta = WheelDelta {
                x: f64::from(x * scale),
                y: f64::from(y * scale),
                z: 0.,
                mode: WheelMode::DeltaPixel,
            };
            let point = page_point(event.position, area, scale);
            // The control moves with the page, and its list would not.
            page.dismiss_list();
            page.input(InputEvent::Wheel(WheelEvent::new(delta, point)));
            cx.stop_propagation();
        });
    }
}
