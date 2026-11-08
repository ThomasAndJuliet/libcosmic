// SPDX-License-Identifier: MPL-2.0

use super::*;
use crate::Core;
use crate::app::Application;
use crate::app::cosmic::Cosmic;
use iced::advanced::renderer::Headless;
use iced_futures::futures::{FutureExt, StreamExt};

struct WindowApp {
    core: Core,
}
impl Application for WindowApp {
    type Executor = crate::executor::Default;
    type Flags = ();
    type Message = ();
    const APP_ID: &'static str = "dev.twright.PopupLifecycleTest";
    fn core(&self) -> &Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }
    fn init(core: Core, _: ()) -> (Self, crate::app::Task<()>) {
        (Self { core }, iced::Task::none())
    }
    fn view(&self) -> crate::Element<'_, ()> {
        crate::widget::space::horizontal().into()
    }
}

fn window(id: window::Id) -> Cosmic<WindowApp> {
    let mut core = Core::default();
    core.set_main_window_id(Some(id));
    Cosmic::new(WindowApp { core })
}

fn renderer() -> crate::Renderer {
    iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
        iced::Font::default(),
        iced::Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("a headless renderer")
}

fn queue(
    parent: window::Id,
    session: &mut TextMenuSession,
    renderer: &crate::Renderer,
) -> window::Id {
    queue_view(
        parent,
        session,
        renderer,
        stock_view(TextMenuContext {
            kind: TextKind::Editable,
            secure: false,
            has_selection: true,
            has_text: true,
            clipboard_has_text: true,
        }),
    )
}

fn queue_view(
    parent: window::Id,
    session: &mut TextMenuSession,
    renderer: &crate::Renderer,
    view: TextMenuView,
) -> window::Id {
    let at = Point::new(20.0, 20.0);
    let handle = session.begin();
    session.native = true;
    queue_text_context_popup(
        at,
        view,
        handle,
        renderer,
        &Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0)),
        mouse::Cursor::Available(at),
        parent,
    );
    session
        .menu_state
        .inner
        .with_data(|inner| inner.popup_id[&parent])
}

fn custom_view(command: usize) -> TextMenuView {
    let item = menu::Tree::from(crate::Element::from(
        menu::menu_button(vec![widget::text("Custom action").into()])
            .on_press(TextMenuCommand::Custom(command)),
    ));
    TextMenuView {
        roots: vec![menu::Tree::with_children(
            RcElementWrapper::new(crate::Element::from(widget::Row::new())),
            vec![item],
        )],
        item_width: ItemWidth::Uniform(240),
    }
}

fn popup_fixture<Message: Clone + 'static>(
    request: PopupRequest,
    renderer: &crate::Renderer,
    limits: &Limits,
) -> (TextContextMenuPopup<Message>, Tree, iced_core::layout::Node) {
    use iced_core::widget::Widget;
    let mut popup = TextContextMenuPopup {
        menu: request.menu,
        handle: request.handle,
        _phantom: std::marker::PhantomData,
    };
    let mut tree = Tree::new(&popup as &dyn Widget<Message, crate::Theme, crate::Renderer>);
    let node = popup.layout(&mut tree, renderer, limits);
    (popup, tree, node)
}

fn escape_event() -> event::Event {
    use iced_core::keyboard::{Event, Key, Location, Modifiers, key};
    event::Event::Keyboard(Event::KeyPressed {
        key: Key::Named(key::Named::Escape),
        modified_key: Key::Named(key::Named::Escape),
        physical_key: key::Physical::Code(key::Code::Escape),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

#[test]
fn late_custom_choice_and_done_cannot_touch_a_successor_generation() {
    use cctk::sctk::reexports::client::{Connection, Proxy, protocol::wl_surface::WlSurface};
    use iced_core::widget::Widget;
    let renderer = renderer();
    let mut session = TextMenuSession::default();
    let parent = window::Id::unique();
    let first_id = queue_view(parent, &mut session, &renderer, custom_view(0));
    let first = take_popup_requests(|id| id == parent).pop().unwrap();
    let bounds = Rectangle::with_size(Size::new(400.0, 300.0));
    let (mut old_popup, mut tree, node) =
        popup_fixture::<()>(first, &renderer, &Limits::new(Size::ZERO, bounds.size()));
    let second_id = queue_view(parent, &mut session, &renderer, custom_view(0));
    let second = take_popup_requests(|id| id == parent).pop().unwrap();
    assert_ne!(old_popup.handle.generation, second.handle.generation);
    assert!(
        !old_popup.handle.commit(TextMenuCommand::Custom(0)),
        "an old index cannot address a successor's custom-message snapshot"
    );
    assert!(second.handle.commit(TextMenuCommand::Custom(0)));
    let expected = TextMenuChoice {
        generation: second.handle.generation,
        command: TextMenuCommand::Custom(0),
    };
    // An inert surface is sufficient: this event never talks to a compositor.
    let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let connection = Connection::from_socket(socket).unwrap();
    let surface = WlSurface::inert(connection.backend().downgrade());
    let mut messages = Vec::new();
    for event in [
        event::Event::PlatformSpecific(iced_core::event::PlatformSpecific::Wayland(
            iced_core::event::wayland::Event::Popup(
                iced_core::event::wayland::PopupEvent::Done,
                surface,
                first_id,
            ),
        )),
        escape_event(),
    ] {
        old_popup.update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced_core::clipboard::Null,
            &mut Shell::new(&mut messages),
            &bounds,
        );
    }
    assert_eq!(session.take_choice(), Some(expected));
    assert_eq!(session.take_choice(), None);
    assert!(
        session
            .menu_state
            .inner
            .with_data(|state| state.popup_id.get(&parent) == Some(&second_id)),
        "late old-surface events must not reset the successor presentation"
    );
    assert!(second.handle.dismiss());
    queue_destroy_popup(second_id);
    take_popup_destroys(|id| id == first_id || id == second_id);
}

#[test]
fn dropping_a_source_cancels_creation_and_invalidates_weak_popup_handles() {
    let renderer = renderer();
    let parent = window::Id::unique();
    let mut queued = TextMenuSession::default();
    queue_view(parent, &mut queued, &renderer, custom_view(3));
    drop(queued);
    assert!(
        take_popup_requests(|id| id == parent).is_empty(),
        "source removal cancels a queued create"
    );

    let mut shown = TextMenuSession::default();
    let popup_id = queue_view(parent, &mut shown, &renderer, custom_view(3));
    let request = take_popup_requests(|id| id == parent).pop().unwrap();
    let weak = request.handle.owner.clone();
    drop(shown);
    assert!(
        weak.upgrade().is_none(),
        "native views must not keep a source owner alive"
    );
    assert!(!request.handle.commit(TextMenuCommand::Custom(3)));
    assert!(!request.handle.dismiss());
    assert!(take_popup_destroys(|id| id == popup_id).contains(&popup_id));
}

#[test]
fn drag_state_clones_do_not_share_the_original_menu_owner() {
    let renderer = renderer();
    let parent = window::Id::unique();
    let mut source = widget::text_input::State::default();
    queue_view(
        parent,
        &mut source.text_menu_session,
        &renderer,
        custom_view(0),
    );
    let copy = source.clone();
    assert!(!Arc::ptr_eq(
        &source.text_menu_session.mailbox,
        &copy.text_menu_session.mailbox
    ));
    drop(copy);
    assert!(
        source.text_menu_session.is_open(),
        "dropping a drag snapshot cannot cancel the source"
    );
    drop(source);
    assert!(take_popup_requests(|id| id == parent).is_empty());
}

#[test]
fn overlay_choices_preserve_their_generation_once_and_outside_dismisses() {
    let renderer = renderer();
    let bounds = Rectangle::with_size(Size::new(400.0, 300.0));
    let mut session = TextMenuSession::default();
    let generation = session.open(
        custom_view(7),
        Point::new(20.0, 20.0),
        &renderer,
        &bounds,
        mouse::Cursor::Unavailable,
        window::Id::NONE,
    );
    let mut overlay = session.overlay::<()>(Vector::ZERO).unwrap();
    let node = overlay.as_overlay_mut().layout(&renderer, bounds.size());
    let layout = Layout::new(&node);
    let at = layout
        .children()
        .next()
        .unwrap()
        .children()
        .next()
        .unwrap()
        .bounds()
        .center();
    let mut messages = Vec::new();
    for event in [
        mouse::Event::CursorMoved { position: at },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        let mut shell = Shell::new(&mut messages);
        overlay.as_overlay_mut().update(
            &event::Event::Mouse(event),
            layout,
            mouse::Cursor::Available(at),
            &renderer,
            &mut iced_core::clipboard::Null,
            &mut shell,
        );
        if matches!(event, mouse::Event::ButtonReleased(_)) {
            assert_eq!(shell.redraw_request(), window::RedrawRequest::NextFrame);
        }
    }
    drop(overlay);
    assert_eq!(
        session.take_choice(),
        Some(TextMenuChoice {
            generation,
            command: TextMenuCommand::Custom(7)
        })
    );
    assert_eq!(session.take_choice(), None);
    assert!(!session.is_open());

    for escape in [false, true] {
        session.open(
            custom_view(8),
            Point::new(20.0, 20.0),
            &renderer,
            &bounds,
            mouse::Cursor::Unavailable,
            window::Id::NONE,
        );
        let mut overlay = session.overlay::<()>(Vector::ZERO).unwrap();
        let node = overlay.as_overlay_mut().layout(&renderer, bounds.size());
        let event = if escape {
            escape_event()
        } else {
            event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        };
        overlay.as_overlay_mut().update(
            &event,
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(390.0, 290.0)),
            &renderer,
            &mut iced_core::clipboard::Null,
            &mut Shell::new(&mut messages),
        );
        drop(overlay);
        assert!(
            !session.is_open(),
            "Escape and outside presses invalidate overlay openings"
        );
        assert_eq!(session.take_choice(), None);
    }
}

fn tooltip_view() -> TextMenuView {
    let row: crate::Element<'static, TextMenuCommand> = widget::tooltip(
        menu::menu_button(vec![widget::text("Unavailable").into()]),
        widget::text("Select text before copying"),
        widget::tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::ZERO)
    .into();
    TextMenuView {
        roots: vec![menu::Tree::with_children(
            RcElementWrapper::new(crate::Element::from(widget::Row::new())),
            vec![menu::Tree::from(row)],
        )],
        item_width: ItemWidth::Uniform(240),
    }
}

fn draw_reason(
    mut reason: overlay::Element<'_, (), crate::Theme, crate::Renderer>,
    renderer: &mut crate::Renderer,
    bounds: Size,
) {
    let node = reason.as_overlay_mut().layout(renderer, bounds);
    assert!(
        node.size().width > 0.0 && node.size().height > 0.0,
        "the hovered menu row exposes its tooltip's layout"
    );
    reason.as_overlay().draw(
        renderer,
        &crate::Theme::dark(),
        &renderer::Style {
            text_color: iced_core::Color::WHITE,
            icon_color: iced_core::Color::WHITE,
            scale_factor: 1.0,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
    );
}

#[test]
fn disabled_row_tooltips_are_forwarded_by_native_popup_and_fallback_overlay() {
    use iced_core::widget::Widget;
    let mut renderer = renderer();
    let bounds = Rectangle::with_size(Size::new(400.0, 300.0));
    for native in [false, true] {
        let mut session = TextMenuSession::default();
        let mut messages = Vec::new();
        if native {
            let parent = window::Id::unique();
            queue_view(parent, &mut session, &renderer, tooltip_view());
            let request = take_popup_requests(|id| id == parent).pop().unwrap();
            let (mut popup, mut tree, node) =
                popup_fixture::<()>(request, &renderer, &Limits::new(Size::ZERO, bounds.size()));
            let layout = Layout::new(&node);
            let at = layout
                .children()
                .next()
                .unwrap()
                .children()
                .next()
                .unwrap()
                .bounds()
                .center();
            for event in [
                event::Event::Mouse(mouse::Event::CursorMoved { position: at }),
                event::Event::Window(window::Event::RedrawRequested(
                    iced_core::time::Instant::now(),
                )),
            ] {
                popup.update(
                    &mut tree,
                    &event,
                    layout,
                    mouse::Cursor::Available(at),
                    &renderer,
                    &mut iced_core::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &bounds,
                );
            }
            let reason = popup
                .overlay(&mut tree, layout, &renderer, &bounds, Vector::ZERO)
                .expect("native text popup forwards a shared row's tooltip");
            draw_reason(reason, &mut renderer, bounds.size());
        } else {
            session.open(
                tooltip_view(),
                Point::new(20.0, 20.0),
                &renderer,
                &bounds,
                mouse::Cursor::Unavailable,
                window::Id::NONE,
            );
            let mut menu = session.overlay::<()>(Vector::ZERO).unwrap();
            let node = menu.as_overlay_mut().layout(&renderer, bounds.size());
            let layout = Layout::new(&node);
            let at = layout
                .children()
                .next()
                .unwrap()
                .children()
                .next()
                .unwrap()
                .bounds()
                .center();
            for event in [
                event::Event::Mouse(mouse::Event::CursorMoved { position: at }),
                event::Event::Window(window::Event::RedrawRequested(
                    iced_core::time::Instant::now(),
                )),
            ] {
                menu.as_overlay_mut().update(
                    &event,
                    layout,
                    mouse::Cursor::Available(at),
                    &renderer,
                    &mut iced_core::clipboard::Null,
                    &mut Shell::new(&mut messages),
                );
            }
            let reason = menu
                .as_overlay_mut()
                .overlay(layout, &renderer)
                .expect("fallback text menu forwards a shared row's tooltip");
            draw_reason(reason, &mut renderer, bounds.size());
        }
        assert!(
            messages.is_empty(),
            "hovering a disabled reason cannot publish an app message"
        );
        assert_eq!(session.take_choice(), None);
        session.dismiss();
    }
    take_popup_destroys(|_| true);
}

#[test]
fn native_popup_choices_run_once_on_source_redraw_with_live_permissions() {
    use iced_core::widget::Widget;

    #[derive(Default)]
    struct RecordingClipboard(Vec<String>);
    impl Clipboard for RecordingClipboard {
        fn read(&self, _: iced_core::clipboard::Kind) -> Option<String> {
            None
        }
        fn write(&mut self, _: iced_core::clipboard::Kind, text: String) {
            self.0.push(text);
        }
    }

    let renderer = renderer();
    let viewport = Rectangle::with_size(Size::new(400.0, 300.0));
    let limits = Limits::new(Size::ZERO, viewport.size());
    for (choice, secure, read_only) in [
        (TextAction::Copy, false, false),
        (TextAction::Cut, false, false),
        (TextAction::Copy, true, false),
        (TextAction::Cut, false, true),
    ] {
        let parent = window::Id::unique();
        let mut input = widget::text_input("", "opening").on_input(|text| text);
        let mut source_tree =
            Tree::new(&input as &dyn Widget<String, crate::Theme, crate::Renderer>);
        input.diff(&mut source_tree);
        let source_state = source_tree
            .state
            .downcast_mut::<widget::text_input::State>();
        source_state.select_all();
        let view = stock_view(input.text_menu_context(&source_tree));
        let popup_id = queue_view(
            parent,
            &mut source_tree
                .state
                .downcast_mut::<widget::text_input::State>()
                .text_menu_session,
            &renderer,
            view,
        );
        let request = take_popup_requests(|id| id == parent).pop().unwrap();
        assert_eq!(request.settings.id, popup_id);
        let (mut popup, mut popup_tree, popup_node) =
            popup_fixture::<String>(request, &renderer, &limits);
        let popup_layout = Layout::new(&popup_node);
        // Derive the row's hit target from the real menu layout.
        let row = if choice == TextAction::Cut { 0 } else { 1 };
        let at = popup_layout
            .children()
            .next()
            .unwrap()
            .children()
            .nth(row)
            .unwrap()
            .bounds()
            .center();
        let mut clipboard = RecordingClipboard::default();
        let mut popup_messages = Vec::new();
        for event in [
            mouse::Event::CursorMoved { position: at },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            let released = matches!(event, mouse::Event::ButtonReleased(_));
            let mut shell = Shell::new(&mut popup_messages);
            popup.update(
                &mut popup_tree,
                &event::Event::Mouse(event),
                popup_layout,
                mouse::Cursor::Available(at),
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
            if released {
                assert_eq!(
                    shell.redraw_request(),
                    window::RedrawRequest::NextFrame,
                    "a committed native choice schedules execution without pointer movement"
                );
            }
        }
        assert_eq!(
            source_tree
                .state
                .downcast_ref::<widget::text_input::State>()
                .text_menu_session
                .mailbox
                .lock()
                .unwrap()
                .choice
                .map(|choice| choice.command),
            Some(TextMenuCommand::Edit(choice)),
            "the real popup must place its chosen action in the source mailbox"
        );
        assert!(
            clipboard.0.is_empty(),
            "the popup cannot copy its opening snapshot"
        );
        assert!(popup_messages.is_empty());
        assert!(
            take_popup_destroys(|id| id == popup_id).contains(&popup_id),
            "the native popup must queue teardown, which wakes the existing runtime"
        );

        // Change content and permissions after opening, before the scheduled redraw.
        let source_state = source_tree
            .state
            .downcast_mut::<widget::text_input::State>();
        source_state.tracked_value = widget::text_input::Value::new("current");
        source_state.select_all();
        if secure {
            input = input.password();
        }
        if read_only {
            input = input.editing(false);
        }
        let mut source_messages = Vec::new();
        for _ in 0..2 {
            let source_node = input.layout(&mut source_tree, &renderer, &limits);
            input.update(
                &mut source_tree,
                &event::Event::Window(window::Event::RedrawRequested(
                    iced_core::time::Instant::now(),
                )),
                Layout::new(&source_node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut source_messages),
                &viewport,
            );
        }
        assert_eq!(
            source_tree
                .state
                .downcast_mut::<widget::text_input::State>()
                .text_menu_session
                .take_choice(),
            None,
            "the source takes a choice only once"
        );
        let permitted = !secure && !read_only;
        assert_eq!(
            clipboard.0,
            if permitted {
                vec!["current".to_owned()]
            } else {
                vec![]
            },
            "source execution uses live selection and permissions exactly once"
        );
        assert_eq!(
            source_messages,
            if permitted && choice == TextAction::Cut {
                vec![String::new()]
            } else {
                vec![]
            },
            "a native Cut publishes one edit callback, and Copy publishes none"
        );
    }
}

#[test]
fn two_windows_keep_queued_text_popups_with_their_owner() {
    let mut first = window(window::Id::unique());
    let mut second = window(window::Id::unique());
    let first_id = first.app.core.main_window_id().unwrap();
    let second_id = second.app.core.main_window_id().unwrap();
    let renderer = renderer();
    let mut first_state = TextMenuSession::default();
    let mut second_state = TextMenuSession::default();
    let second_popup = queue(second_id, &mut second_state, &renderer);
    let first_popup = queue(first_id, &mut first_state, &renderer);
    drop(first.update(crate::Action::None));
    assert!(first.surface_views.contains_key(&first_popup));
    assert!(
        !first.surface_views.contains_key(&second_popup),
        "one live window must not steal the other's queued popup"
    );
    drop(second.update(crate::Action::None));
    assert!(second.surface_views.contains_key(&second_popup));
    queue_destroy_popup(second_popup);
    drop(first.update(crate::Action::None));
    assert!(
        PENDING_POPUP_DESTROYS.with(|queue| queue.borrow().contains(&second_popup)),
        "another window must not consume the owner's teardown"
    );
    drop(first);
    assert!(second.surface_views.contains_key(&second_popup));
    drop(second.update(crate::Action::None));
    assert!(!PENDING_POPUP_DESTROYS.with(|queue| queue.borrow().contains(&second_popup)));
}

#[test]
fn dropping_a_window_discards_only_its_pending_popups() {
    let first = window(window::Id::unique());
    let mut second = window(window::Id::unique());
    let first_id = first.app.core.main_window_id().unwrap();
    let second_id = second.app.core.main_window_id().unwrap();
    let renderer = renderer();
    let mut first_state = TextMenuSession::default();
    let mut second_state = TextMenuSession::default();
    queue(first_id, &mut first_state, &renderer);
    let second_popup = queue(second_id, &mut second_state, &renderer);
    drop(first);
    assert!(
        !PENDING_POPUP_REQUESTS.with(|queue| queue
            .borrow()
            .iter()
            .any(|req| req.settings.parent == first_id)),
        "closed owner cannot leave a pending popup behind"
    );
    assert!(PENDING_POPUP_REQUESTS.with(|queue| {
        queue
            .borrow()
            .iter()
            .any(|req| req.settings.parent == second_id)
    }));
    drop(second.update(crate::Action::None));
    assert!(second.surface_views.contains_key(&second_popup));
}

#[test]
fn reopening_a_text_popup_uses_a_fresh_surface_identity() {
    let renderer = renderer();
    let mut state = TextMenuSession::default();
    let parent = window::Id::unique();
    let first = queue(parent, &mut state, &renderer);
    let second = queue(parent, &mut state, &renderer);
    assert_ne!(
        first, second,
        "late dismissal must distinguish the replaced popup from its successor"
    );
    assert!(state.menu_state.inner.with_data(|inner| inner.open));
    take_popup_requests(|_| true);
    take_popup_destroys(|_| true);
}

#[test]
fn a_replaced_text_popup_cannot_dismiss_its_successor() {
    use iced_core::widget::Widget;
    let renderer = renderer();
    let mut state = TextMenuSession::default();
    let parent = window::Id::unique();
    let first_id = queue(parent, &mut state, &renderer);
    let first = take_popup_requests(|id| id == parent).pop().unwrap();
    let limits = Limits::new(Size::ZERO, Size::new(400.0, 300.0));
    let (mut popup, mut tree, node) = popup_fixture::<()>(first, &renderer, &limits);
    let second_id = queue(parent, &mut state, &renderer);
    assert_ne!(first_id, second_id);
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    popup.update(
        &mut tree,
        &escape_event(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut iced_core::clipboard::Null,
        &mut shell,
        &Rectangle::new(Point::ORIGIN, limits.max()),
    );
    assert!(
        state
            .menu_state
            .inner
            .with_data(|inner| inner.open && inner.popup_id.get(&parent) == Some(&second_id)),
        "a late event on the replaced surface must preserve its successor"
    );
    assert_eq!(
        popup.layout(&mut tree, &renderer, &limits).size(),
        Size::ZERO,
        "a replaced popup must not lay out its successor's shared tree"
    );
    take_popup_requests(|_| true);
    take_popup_destroys(|_| true);
}

#[test]
fn popup_wakes_survive_either_of_two_window_subscriptions_closing() {
    type WakeStream =
        iced_futures::futures::stream::BoxStream<'static, (window::Id, crate::Action<()>)>;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let receive = |stream: &mut WakeStream| {
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(100), stream.next())
                .await
                .expect("a live window's subscription must be woken")
                .expect("a live window's subscription must remain open")
        })
    };
    for close_first in [true, false] {
        let stream = |id| {
            let mut recipes =
                iced_futures::subscription::into_recipes(wake_subscription::<()>().with(id));
            recipes
                .pop()
                .unwrap()
                .stream(Box::pin(iced_futures::futures::stream::pending()))
        };
        let first_id = window::Id::unique();
        let second_id = window::Id::unique();
        let mut first = stream(first_id);
        let mut second = stream(second_id);
        assert!(first.next().now_or_never().is_none());
        assert!(
            second.next().now_or_never().is_none(),
            "second window must keep its wake subscription alive"
        );
        wake_runtime();
        assert!(matches!(receive(&mut first), (id, crate::Action::None) if id == first_id));
        assert!(matches!(receive(&mut second), (id, crate::Action::None) if id == second_id));
        let (mut surviving, expected) = if close_first {
            drop(first);
            (second, second_id)
        } else {
            drop(second);
            (first, first_id)
        };
        let mut session = TextMenuSession::default();
        let handle = session.begin();
        assert!(handle.commit(TextMenuCommand::Custom(0)));
        assert!(matches!(receive(&mut surviving), (id, crate::Action::None) if id == expected));
    }
}
