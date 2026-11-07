// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! Right-click context menu for widgets with selectable text.
//!
//! Use [`context_menu_overlay`] from your widget's `overlay()` method
//! with any widget that implements
//! [`HasSelectableText`](iced_core::widget::text::HasSelectableText)
//! to get a context menu with Copy, Select All, and optionally Cut/Paste.
//!
//! Internally uses libcosmic's [`Menu`](crate::widget::menu) system for
//! proper rendering, hover effects, and positioning.
//!
//! On Wayland, [`create_text_context_popup`] can be used instead to show
//! the context menu as a native popup surface.

pub use iced_core::widget::text::HasSelectableText;
#[cfg(wayland_platform)]
use iced_core::window;
#[cfg(wayland_platform)]
use iced_runtime::platform_specific::wayland::CornerRadius;

#[cfg(wayland_platform)]
use crate::surface::action::LiveSettings;
use crate::widget::RcElementWrapper;
#[cfg(wayland_platform)]
use crate::widget::menu::StyleSheet;
use crate::widget::menu::{
    self, CloseCondition, ItemHeight, ItemWidth, Menu, MenuBarState, PathHighlight, menu_roots_diff,
};
use crate::{theme, widget};

use iced_core::layout::Limits;
use iced_core::widget::Tree;
use iced_core::{
    Clipboard, Layout, Point, Rectangle, Shell, Size, Vector, mouse, overlay, renderer,
};
use iced_widget::core::event;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

/// A built-in operation on a native text target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAction {
    Copy,
    Cut,
    Paste,
    SelectAll,
}

// Keep the stock popup's internal vocabulary while sharing its executor.
pub(crate) use TextAction as TextCtxAction;

/// The native target's effective editing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    Editable,
    ReadOnlyInput,
    Selectable,
}

/// Current capabilities, without exposing selected plaintext.
///
/// Clipboard availability is a snapshot for presentation. Executors read the
/// clipboard and recheck the target's permissions when an action is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextMenuContext {
    pub kind: TextKind,
    pub secure: bool,
    pub has_selection: bool,
    pub has_text: bool,
    pub clipboard_has_text: bool,
}

impl TextMenuContext {
    /// Whether an action can be offered in this snapshot.
    pub fn allows(self, action: TextAction) -> bool {
        match action {
            TextAction::Copy => !self.secure && self.has_selection,
            TextAction::Cut => {
                self.kind == TextKind::Editable && !self.secure && self.has_selection
            }
            TextAction::Paste => self.kind == TextKind::Editable && self.clipboard_has_text,
            TextAction::SelectAll => self.has_text,
        }
    }
}

/// Native selection and editing for a concrete text widget.
///
/// Configure the target before creating its widget tree. External presentation
/// suppresses the stock menu while retaining native focus and request handling.
/// Every executor checks live permissions, including after a view rebuild.
pub trait TextMenuTarget<Message> {
    fn use_external_text_menu(&mut self);
    fn take_text_menu_request(&self, tree: &mut Tree) -> Option<Point>;
    fn text_menu_context(&self, tree: &Tree) -> TextMenuContext;
    fn execute_text_menu_action(
        &mut self,
        tree: &mut Tree,
        action: TextAction,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    );
}

impl<Message, Theme, Renderer> TextMenuTarget<Message>
    for iced_core::widget::Text<'_, Theme, Renderer>
where
    Theme: iced_core::widget::text::Catalog,
    Renderer: iced_core::text::Renderer,
{
    fn use_external_text_menu(&mut self) {}

    fn take_text_menu_request(&self, tree: &mut Tree) -> Option<Point> {
        let position = self.context_menu_position(tree);
        self.set_context_menu_position(tree, None);
        position
    }

    fn text_menu_context(&self, tree: &Tree) -> TextMenuContext {
        TextMenuContext {
            kind: TextKind::Selectable,
            secure: false,
            has_selection: self.is_selectable() && self.selected_text(tree).is_some(),
            has_text: self.is_selectable() && self.has_text(tree),
            clipboard_has_text: self.clipboard_has_text(tree),
        }
    }

    fn execute_text_menu_action(
        &mut self,
        tree: &mut Tree,
        action: TextAction,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        if !<Self as TextMenuTarget<Message>>::text_menu_context(self, tree).allows(action) {
            return;
        }
        match action {
            TextAction::Copy => self.copy_to_clipboard(tree, clipboard),
            TextAction::SelectAll => self.select_all(tree),
            TextAction::Cut | TextAction::Paste => return,
        }
        shell.request_redraw();
    }
}

/// Shared state for communicating deferred context menu actions
/// from a Wayland popup back to the owning text widget.
pub(crate) type PendingAction = Arc<Mutex<Option<TextCtxAction>>>;

/// Creates a new [`PendingAction`] for use with popup-based context menus.
pub(crate) fn pending_action() -> PendingAction {
    Arc::new(Mutex::new(None))
}

/// Takes a pending action if one was set by a popup menu, and returns it.
pub(crate) fn take_pending_action(pending: &PendingAction) -> Option<TextCtxAction> {
    pending.lock().ok().and_then(|mut guard| guard.take())
}

use std::cell::Cell;

thread_local! {
    static CURRENT_WINDOW_ID: Cell<iced_core::window::Id> = const { Cell::new(iced_core::window::Id::NONE) };
}

#[cfg(wayland_platform)]
use iced_runtime::platform_specific::wayland::popup::SctkPopupSettings;

/// A request to create a text context-menu popup surface, queued by a widget
/// during `update()` and drained by `Cosmic::update()` so the popup goes
/// through the normal `get_popup()` Task + `surface_views` pipeline.
#[cfg(wayland_platform)]
pub(crate) struct PopupRequest {
    live_settings: LiveSettings,
    settings: SctkPopupSettings,
    menu: Menu<'static, TextCtxAction>,
    pending_action: PendingAction,
}

#[cfg(wayland_platform)]
thread_local! {
    static PENDING_POPUP_REQUESTS: std::cell::RefCell<Vec<PopupRequest>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn set_current_window_id(id: iced_core::window::Id) {
    CURRENT_WINDOW_ID.set(id);
}

pub(crate) fn current_window_id() -> iced_core::window::Id {
    CURRENT_WINDOW_ID.get()
}

/// Drains all popup requests queued by widgets this frame.
#[cfg(wayland_platform)]
pub(crate) fn take_popup_requests(mut owns: impl FnMut(window::Id) -> bool) -> Vec<PopupRequest> {
    PENDING_POPUP_REQUESTS.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (owned, remaining) = std::mem::take(&mut *queue)
            .into_iter()
            .partition(|request: &PopupRequest| owns(request.settings.parent));
        *queue = remaining;
        owned
    })
}

/// Consumes a [`PopupRequest`], returning the popup settings plus a view
/// builder. The builder rebuilds the menu element each frame from the
/// captured content, independent of app state.
#[cfg(wayland_platform)]
#[allow(clippy::type_complexity)]
pub(crate) fn into_popup_view<Message: Clone + 'static>(
    req: PopupRequest,
) -> (
    LiveSettings,
    SctkPopupSettings,
    Box<dyn Fn() -> crate::Element<'static, crate::Action<Message>> + Send + Sync>,
) {
    let PopupRequest {
        settings,
        menu,
        pending_action,
        live_settings,
    } = req;

    let view = Box::new(move || {
        let popup_widget: TextContextMenuPopup<Message> = TextContextMenuPopup {
            menu: menu.clone(),
            pending_action: pending_action.clone(),
            _phantom: std::marker::PhantomData,
        };
        crate::Element::from(crate::widget::container(popup_widget).center(iced_core::Length::Fill))
            .map(crate::action::app)
    });

    (live_settings, settings, view)
}

#[cfg(wayland_platform)]
thread_local! {
    static PENDING_POPUP_DESTROYS: std::cell::RefCell<Vec<iced_core::window::Id>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Queues a context-menu popup for teardown.
///
/// Mirrors [`create_text_context_popup`]'s request queue: widgets running
/// inside `update()` can't reach `Cosmic` to issue a Task, so they push the
/// id here and `Cosmic::update()` drains it into a `destroy_popup` Task that
/// flows through the normal surface pipeline.
#[cfg(wayland_platform)]
fn queue_destroy_popup(id: iced_core::window::Id) {
    // A replacement may arrive before the owner installed the first popup.
    // Cancel that create instead of destroying a surface which never existed.
    let cancelled = PENDING_POPUP_REQUESTS.with(|queue| {
        let mut queue = queue.borrow_mut();
        let before = queue.len();
        queue.retain(|request| request.settings.id != id);
        queue.len() != before
    });
    if !cancelled {
        PENDING_POPUP_DESTROYS.with(|queue| queue.borrow_mut().push(id));
        wake_runtime();
    }
}

#[cfg(wayland_platform)]
static WAKE_SENDERS: std::sync::Mutex<Vec<iced_futures::futures::channel::mpsc::Sender<()>>> =
    std::sync::Mutex::new(Vec::new());

/// Stable identity for [`wake_subscription`].
#[cfg(wayland_platform)]
struct PopupWake;

/// Nudges the runtime so `Cosmic::update()` runs and drains the popup queues.
#[cfg(wayland_platform)]
fn wake_runtime() {
    let mut senders = WAKE_SENDERS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    senders.retain_mut(|sender| match sender.try_send(()) {
        Ok(()) => true,
        // A full queue already has a wake pending; closed subscriptions no
        // longer own a window and must not prevent the others from waking.
        Err(error) => !error.is_disconnected(),
    });
}

/// Subscription that backs [`wake_runtime`]: it owns the receiving end of the
/// wake channel and re-emits each ping as [`crate::Action::None`]. Add it to
/// the app's subscriptions (done by `Cosmic::subscription`) so popup creation
/// and teardown queued from widget `update()` get drained promptly.
#[cfg(wayland_platform)]
pub(crate) fn wake_subscription<Message: Send + 'static>()
-> iced_futures::Subscription<crate::Action<Message>> {
    use iced_futures::futures::{SinkExt, StreamExt};
    iced_futures::Subscription::run_with(std::any::TypeId::of::<PopupWake>(), |_| {
        iced::stream::channel(
            16,
            |mut output: iced_futures::futures::channel::mpsc::Sender<crate::Action<Message>>| async move {
                let (tx, mut rx) = iced_futures::futures::channel::mpsc::channel(16);
                {
                    let mut senders = WAKE_SENDERS
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    senders.retain(|sender| !sender.is_closed());
                    senders.push(tx);
                }
                while rx.next().await.is_some() {
                    let _ = output.send(crate::Action::None).await;
                }
            },
        )
    })
}

/// Drains all popup teardown requests queued by widgets this frame.
///
/// Called by `Cosmic::update()`, which turns each id into a `destroy_popup()`
/// Task. Drained before the creation queue so a replacement closes its old
/// native surface before opening the new one.
#[cfg(wayland_platform)]
pub(crate) fn take_popup_destroys(mut owns: impl FnMut(window::Id) -> bool) -> Vec<window::Id> {
    PENDING_POPUP_DESTROYS.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (owned, remaining) = std::mem::take(&mut *queue)
            .into_iter()
            .partition(|id| owns(*id));
        *queue = remaining;
        owned
    })
}

/// Discard deferred work when its owner has closed. Other windows' queues
/// remain intact, including when an outer window host has no main window.
#[cfg(wayland_platform)]
pub(crate) fn forget_popup_requests(
    mut owns_parent: impl FnMut(window::Id) -> bool,
    mut owns_popup: impl FnMut(window::Id) -> bool,
) {
    let _ = PENDING_POPUP_REQUESTS.try_with(|queue| {
        queue
            .borrow_mut()
            .retain(|request| !owns_parent(request.settings.parent))
    });
    let _ =
        PENDING_POPUP_DESTROYS.try_with(|queue| queue.borrow_mut().retain(|id| !owns_popup(*id)));
}

/// Creates a context menu overlay for any widget implementing
/// [`HasSelectableText`].
///
/// Call this from your widget's `overlay()` method. Built-in choices execute
/// through the target's native editing and callback path.
///
/// The `menu_bar_state` parameter must be a persistent [`MenuBarState`]
/// stored in the widget's tree state.
pub(crate) fn context_menu_overlay<'a, W, Message>(
    widget: &'a mut W,
    tree: &'a mut Tree,
    translation: Vector,
    menu_bar_state: MenuBarState,
) -> Option<overlay::Element<'a, Message, crate::Theme, crate::Renderer>>
where
    W: TextMenuTarget<Message> + HasSelectableText + 'a,
    Message: Clone + 'static,
{
    let click_position = widget.context_menu_position(tree)?;
    let mut menu_roots = build_menu_roots(widget.text_menu_context(tree));
    menu_roots.iter_mut().for_each(menu::Tree::set_index);

    let bounds = Rectangle {
        x: click_position.x,
        y: click_position.y,
        width: 240.0,
        height: 240.0,
    };

    let item_count = menu_roots[0].children.len();
    menu_bar_state.inner.with_data_mut(|state| {
        let stale = state.menu_states.first().is_some_and(|ms| {
            ms.menu_bounds.child_positions.len() != item_count
                || (ms.menu_bounds.parent_bounds.x - bounds.x).abs() > 0.5
                || (ms.menu_bounds.parent_bounds.y - bounds.y).abs() > 0.5
        });
        if !state.open || stale {
            state.menu_states.clear();
            state.active_root.clear();
            state.open = true;
        }
        menu_roots_diff(&mut menu_roots, &mut state.tree);
    });

    let menu = Menu {
        tree: menu_bar_state.clone(),
        menu_roots,
        bounds_expand: 16,
        menu_overlays_parent: true,
        close_condition: CloseCondition {
            leave: false,
            click_outside: true,
            click_inside: true,
        },
        item_width: ItemWidth::Uniform(240),
        item_height: ItemHeight::Dynamic(40),
        bar_bounds: bounds,
        main_offset: -(bounds.height as i32),
        cross_offset: 0,
        root_bounds_list: vec![bounds],
        path_highlight: Some(PathHighlight::MenuActive),
        style: Cow::Owned(theme::menu_bar::MenuBarStyle::Default),
        position: Point::new(translation.x, translation.y),
        is_overlay: true,
        window_id: iced::window::Id::NONE,
        depth: 0,
        on_surface_action: None,
    };

    Some(overlay::Element::new(Box::new(TextMenuOverlay {
        menu,
        widget,
        tree,
        _message: std::marker::PhantomData,
    })))
}

fn build_menu_roots(context: TextMenuContext) -> Vec<menu::Tree<TextCtxAction>> {
    let is_editable = context.kind == TextKind::Editable;
    let item = |label: &'static str, action: TextCtxAction, enabled: bool| {
        menu::Tree::from(crate::Element::from(
            menu::menu_button(vec![widget::text(label).into()])
                .on_press_maybe(enabled.then_some(action)),
        ))
    };

    let mut items = Vec::with_capacity(4);
    if is_editable {
        items.push(item(
            "Cut",
            TextCtxAction::Cut,
            context.allows(TextAction::Cut),
        ));
    }
    items.push(item(
        "Copy",
        TextCtxAction::Copy,
        context.allows(TextAction::Copy),
    ));
    if is_editable {
        items.push(item(
            "Paste",
            TextCtxAction::Paste,
            context.allows(TextAction::Paste),
        ));
    }
    items.push(item(
        "Select All",
        TextCtxAction::SelectAll,
        context.allows(TextAction::SelectAll),
    ));

    vec![menu::Tree::with_children(
        RcElementWrapper::new(crate::Element::from(widget::Row::new())),
        items,
    )]
}

struct TextMenuOverlay<'a, W, Message: Clone + 'static> {
    menu: Menu<'a, TextCtxAction>,
    widget: &'a mut W,
    tree: &'a mut Tree,
    _message: std::marker::PhantomData<Message>,
}

impl<W, Message> overlay::Overlay<Message, crate::Theme, crate::Renderer>
    for TextMenuOverlay<'_, W, Message>
where
    W: TextMenuTarget<Message> + HasSelectableText,
    Message: Clone + 'static,
{
    fn layout(&mut self, renderer: &crate::Renderer, bounds: Size) -> iced_core::layout::Node {
        // Initialise the menu before the first draw so it appears at the click
        // position immediately
        let needs_init = self
            .menu
            .tree
            .inner
            .with_data(|state| state.open && state.menu_states.is_empty());

        if needs_init {
            let overlay_offset = Point::ORIGIN - self.menu.position;
            let bar_bounds = self.menu.bar_bounds;
            let main_offset = self.menu.main_offset as f32;
            let overlay_cursor = bar_bounds.center();

            let mut init_messages: Vec<TextCtxAction> = Vec::new();
            let mut init_shell = Shell::new(&mut init_messages);
            menu::init_root_menu(
                &mut self.menu,
                renderer,
                &mut init_shell,
                overlay_cursor,
                bounds,
                overlay_offset,
                bar_bounds,
                main_offset,
            );
        }

        self.menu.layout(
            renderer,
            Limits::NONE
                .min_width(bounds.width)
                .max_width(bounds.width)
                .min_height(bounds.height)
                .max_height(bounds.height),
        )
    }

    fn draw(
        &self,
        renderer: &mut crate::Renderer,
        theme: &crate::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.draw(renderer, theme, style, layout, cursor);
    }

    fn update(
        &mut self,
        event: &event::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &crate::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // Right-clicks are not menu interactions. A right-click *on* the menu
        // (notably the press/release that opened it) is swallowed so it does
        // not close the menu. A right-click *outside* closes it — clearing the
        // menu position lets the widget reopen it at the new point on the same
        // event. Initialization happens in `layout()`.
        if matches!(
            event,
            event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                | event::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right))
        ) {
            let over_menu = cursor
                .position()
                .is_some_and(|p| layout.bounds().contains(p));
            if !over_menu {
                self.menu.tree.inner.with_data_mut(|state| {
                    state.menu_states.clear();
                    state.active_root.clear();
                    state.open = false;
                });
                self.widget.set_context_menu_position(self.tree, None);
                shell.request_redraw();
            }
            return;
        }

        let mut local_messages = Vec::new();
        let mut local_shell = Shell::new(&mut local_messages);

        self.menu
            .update(event, layout, cursor, renderer, clipboard, &mut local_shell);

        if local_shell.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(local_shell.redraw_request());
        if local_shell.is_layout_invalid() {
            shell.invalidate_layout();
        }

        for action in local_messages {
            self.widget
                .execute_text_menu_action(self.tree, action, clipboard, shell);
            self.widget.set_context_menu_position(self.tree, None);
            // The menu closes and the selection may have changed.
            shell.request_redraw();
        }

        let is_open = self.menu.tree.inner.with_data(|state| state.open);
        if !is_open {
            self.widget.set_context_menu_position(self.tree, None);
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &crate::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::None
        }
    }
}

/// Queues a Wayland popup surface containing the text context menu.
///
/// Pushes a [`PopupRequest`] onto the request queue; `Cosmic::update()`
/// drains it and creates the popup through `get_popup()`, so it flows
/// through the normal Task + `surface_views` pipeline.
#[cfg(wayland_platform)]
pub(crate) fn create_text_context_popup(
    click_position: Point,
    context: TextMenuContext,
    menu_bar_state: &MenuBarState,
    pending_action: &PendingAction,
    renderer: &crate::Renderer,
    viewport: &Rectangle,
    cursor: mouse::Cursor,
    window_id: window::Id,
) {
    use iced_runtime::platform_specific::wayland::popup::{SctkPopupSettings, SctkPositioner};

    if window_id == iced_core::window::Id::NONE {
        return;
    }

    let mut menu_roots = build_menu_roots(context);
    menu_roots.iter_mut().for_each(menu::Tree::set_index);

    let id = menu_bar_state.inner.with_data_mut(|state| {
        state.menu_states.clear();
        state.active_root.clear();
        menu_roots_diff(&mut menu_roots, &mut state.tree);
        if let Some(id) = state.popup_id.remove(&window_id) {
            queue_destroy_popup(id);
        }
        state.open = true;
        state.view_cursor = cursor;
        iced::window::Id::unique()
    });

    let bounds = Rectangle {
        x: click_position.x,
        y: click_position.y,
        width: 240.0,
        height: 240.0,
    };

    let mut popup_menu: Menu<'static, TextCtxAction> = Menu {
        tree: menu_bar_state.clone(),
        menu_roots,
        bounds_expand: 16,
        menu_overlays_parent: true,
        close_condition: CloseCondition {
            leave: false,
            click_outside: true,
            click_inside: true,
        },
        item_width: ItemWidth::Uniform(240),
        item_height: ItemHeight::Dynamic(40),
        bar_bounds: bounds,
        main_offset: -(bounds.height as i32),
        cross_offset: 0,
        root_bounds_list: vec![bounds],
        path_highlight: Some(PathHighlight::MenuActive),
        style: Cow::Owned(theme::menu_bar::MenuBarStyle::Default),
        position: Point::new(0., 0.),
        is_overlay: false,
        window_id: id,
        depth: 0,
        on_surface_action: None,
    };

    {
        let mut init_messages: Vec<TextCtxAction> = Vec::new();
        let mut init_shell = Shell::new(&mut init_messages);
        menu::init_root_menu(
            &mut popup_menu,
            renderer,
            &mut init_shell,
            cursor.position().unwrap_or_default(),
            viewport.size(),
            Vector::new(0., 0.),
            bounds,
            -(bounds.height),
        );
    }

    let anchor_rect = menu_bar_state.inner.with_data_mut(|state| {
        state.popup_id.insert(window_id, id);
        let pos = cursor.position().unwrap_or_default();
        iced::Rectangle {
            x: pos.x as i32,
            y: pos.y as i32,
            width: 1,
            height: 1,
        }
    });

    let menu_node = popup_menu.layout(renderer, iced::Limits::NONE.min_width(1.).min_height(1.));
    let popup_size = menu_node.size();

    let positioner = SctkPositioner {
        size: Some((
            popup_size.width.ceil() as u32 + 2,
            popup_size.height.ceil() as u32 + 2,
        )),
        anchor_rect,
        anchor: cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Anchor::None,
        gravity: cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::BottomRight,
        reactive: true,
        ..Default::default()
    };

    // Queue the request. `Cosmic::update()` drains it and creates the popup
    // through `get_popup()`, so it flows through the normal Task +
    // `surface_views` pipeline (rendering and teardown included).
    let settings = SctkPopupSettings {
        parent: window_id,
        id,
        positioner,
        parent_size: None,
        grab: true,
        close_with_children: false,
        input_zone: None,
    };

    let t = crate::theme::THEME.lock().unwrap();
    let styling = t.appearance(&crate::theme::menu_bar::MenuBarStyle::Default, false);
    drop(t);
    let rad = styling.menu_border_radius;
    PENDING_POPUP_REQUESTS.with(|q| {
        q.borrow_mut().push(PopupRequest {
            settings,
            menu: popup_menu,
            pending_action: pending_action.clone(),
            live_settings: LiveSettings {
                corners: Some(CornerRadius {
                    top_left: rad[0] as u32,
                    top_right: rad[1] as u32,
                    bottom_left: rad[2] as u32,
                    bottom_right: rad[3] as u32,
                }),
                ..Default::default()
            },
        });
    });
    wake_runtime();
}

/// Dismisses this widget's open context-menu popup on an outside click,
/// touch, or Escape.
#[cfg(wayland_platform)]
pub(crate) fn dismiss_popup_on_event(
    menu_bar_state: &MenuBarState,
    event: &event::Event,
    window_id: window::Id,
) {
    let is_dismiss = matches!(
        event,
        event::Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left | mouse::Button::Middle
        )) | event::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
            key: iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Escape),
            ..
        }) | event::Event::Touch(iced_core::touch::Event::FingerPressed { .. })
    );
    if !is_dismiss {
        return;
    }

    let popup_id = menu_bar_state
        .inner
        .with_data(|state| state.popup_id.get(&window_id).copied());
    if let Some(popup_id) = popup_id {
        menu_bar_state.inner.with_data_mut(|state| {
            state.popup_id.retain(|_, v| *v != popup_id);
            state.reset();
        });
        queue_destroy_popup(popup_id);
    }
}

/// Widget that wraps [`Menu`] inside a Wayland popup and intercepts
/// [`TextCtxAction`] messages for clipboard operations.
#[derive(Clone)]
struct TextContextMenuPopup<Message: Clone + 'static> {
    menu: Menu<'static, TextCtxAction>,
    pending_action: PendingAction,
    _phantom: std::marker::PhantomData<Message>,
}

impl<Message: Clone + 'static> TextContextMenuPopup<Message> {
    fn is_current_popup(&self) -> bool {
        self.menu
            .tree
            .inner
            .with_data(|state| state.popup_id.values().any(|id| *id == self.menu.window_id))
    }

    /// A replaced native surface can still receive its final events. Only
    /// the surface still registered by the text widget may reset that widget.
    fn dismiss(&self) -> bool {
        self.menu.tree.inner.with_data_mut(|state| {
            if !state.popup_id.values().any(|id| *id == self.menu.window_id) {
                return false;
            }
            state.popup_id.retain(|_, id| *id != self.menu.window_id);
            state.reset();
            true
        })
    }
}

impl<Message: Clone + 'static> iced_core::widget::Widget<Message, crate::Theme, crate::Renderer>
    for TextContextMenuPopup<Message>
{
    fn size(&self) -> Size<iced_core::Length> {
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::size(&self.menu)
    }

    fn tag(&self) -> iced_core::widget::tree::Tag {
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::tag(&self.menu)
    }

    fn state(&self) -> iced_core::widget::tree::State {
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::state(&self.menu)
    }

    fn children(&self) -> Vec<iced_core::widget::Tree> {
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::children(&self.menu)
    }

    fn diff(&mut self, tree: &mut iced_core::widget::Tree) {
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::diff(&mut self.menu, tree);
    }

    fn layout(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        renderer: &crate::Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        if !self.is_current_popup() {
            return iced_core::layout::Node::new(limits.min());
        }
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::layout(
            &mut self.menu,
            tree,
            renderer,
            limits,
        )
    }

    fn draw(
        &self,
        tree: &iced_core::widget::Tree,
        renderer: &mut crate::Renderer,
        theme: &crate::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !self.is_current_popup() {
            return;
        }
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::draw(
            &self.menu, tree, renderer, theme, style, layout, cursor, viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        event: &event::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &crate::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if !self.is_current_popup() {
            return;
        }
        #[cfg(wayland_platform)]
        {
            use iced_core::event::wayland::PopupEvent;
            let popup_event = match event {
                event::Event::PlatformSpecific(iced_core::event::PlatformSpecific::Wayland(
                    iced_core::event::wayland::Event::Popup(e, _, _),
                )) => Some(e),
                _ => None,
            };
            if matches!(popup_event, Some(PopupEvent::Done | PopupEvent::Unfocused)) {
                let popup_id = self.menu.window_id;
                self.dismiss();
                // `Done` means the surface was already destroyed by the
                // compositor; only `Unfocused` needs an explicit destroy.
                if matches!(popup_event, Some(PopupEvent::Unfocused)) {
                    queue_destroy_popup(popup_id);
                }
                return;
            }
        }

        // Escape dismisses the popup. Under the grab, keyboard input is
        // delivered to the popup surface, so handle it here.
        #[cfg(wayland_platform)]
        if matches!(
            event,
            event::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
                key: iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Escape),
                ..
            })
        ) {
            let popup_id = self.menu.window_id;
            self.dismiss();
            queue_destroy_popup(popup_id);
            shell.capture_event();
            return;
        }

        let mut local_messages: Vec<TextCtxAction> = Vec::new();
        let mut local_shell = Shell::new(&mut local_messages);

        {
            use iced_core::widget::Widget;
            Widget::<TextCtxAction, crate::Theme, crate::Renderer>::update(
                &mut self.menu,
                tree,
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                &mut local_shell,
                viewport,
            );
        }

        if local_shell.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(local_shell.redraw_request());
        if local_shell.is_layout_invalid() {
            shell.invalidate_layout();
        }

        for action in local_messages {
            if let Ok(mut pending) = self.pending_action.lock() {
                *pending = Some(action);
            }
            // The source executes every choice, including Copy, against its
            // current selection and permissions, before handling its next event.
            shell.request_redraw();
        }

        // Under the popup grab the parent widget receives no events, so the
        // popup must tear itself down once its menu has closed — whether an
        // item was chosen (`click_inside`) or the user clicked away
        // (`click_outside`).
        #[cfg(wayland_platform)]
        {
            let menu_closed = self.menu.tree.inner.with_data(|state| !state.open);
            if menu_closed && self.dismiss() {
                queue_destroy_popup(self.menu.window_id);
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &iced_core::widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &crate::Renderer,
    ) -> mouse::Interaction {
        if !self.is_current_popup() {
            return mouse::Interaction::default();
        }
        use iced_core::widget::Widget;
        Widget::<TextCtxAction, crate::Theme, crate::Renderer>::mouse_interaction(
            &self.menu, tree, layout, cursor, viewport, renderer,
        )
    }
}

impl<Message: Clone + 'static> From<TextContextMenuPopup<Message>>
    for crate::Element<'static, Message>
{
    fn from(popup: TextContextMenuPopup<Message>) -> Self {
        Self::new(popup)
    }
}

#[cfg(all(test, wayland_platform))]
mod lifecycle_tests {
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

    fn queue(parent: window::Id, state: &MenuBarState, renderer: &crate::Renderer) -> window::Id {
        let at = Point::new(20.0, 20.0);
        create_text_context_popup(
            at,
            TextMenuContext {
                kind: TextKind::Editable,
                secure: false,
                has_selection: true,
                has_text: true,
                clipboard_has_text: true,
            },
            state,
            &PendingAction::default(),
            renderer,
            &Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0)),
            mouse::Cursor::Available(at),
            parent,
        );
        state.inner.with_data(|inner| inner.popup_id[&parent])
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
            let pending = source_state.pending_action.clone();
            let menu_state = source_state.menu_bar_state.clone();
            create_text_context_popup(
                Point::new(20.0, 20.0),
                input.text_menu_context(&source_tree),
                &menu_state,
                &pending,
                &renderer,
                &viewport,
                mouse::Cursor::Available(Point::new(20.0, 20.0)),
                parent,
            );
            let request = take_popup_requests(|id| id == parent).pop().unwrap();
            let popup_id = request.settings.id;
            let mut popup: TextContextMenuPopup<String> = TextContextMenuPopup {
                menu: request.menu,
                pending_action: request.pending_action,
                _phantom: std::marker::PhantomData,
            };
            let mut popup_tree =
                Tree::new(&popup as &dyn Widget<String, crate::Theme, crate::Renderer>);
            let popup_node = popup.layout(&mut popup_tree, &renderer, &limits);
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
                *pending.lock().unwrap(),
                Some(choice),
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
                *pending.lock().unwrap(),
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
        let first_state = MenuBarState::default();
        let second_state = MenuBarState::default();
        let second_popup = queue(second_id, &second_state, &renderer);
        let first_popup = queue(first_id, &first_state, &renderer);
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
        queue(first_id, &MenuBarState::default(), &renderer);
        let second_popup = queue(second_id, &MenuBarState::default(), &renderer);
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
        let state = MenuBarState::default();
        let parent = window::Id::unique();
        let first = queue(parent, &state, &renderer);
        let second = queue(parent, &state, &renderer);
        assert_ne!(
            first, second,
            "late dismissal must distinguish the replaced popup from its successor"
        );
        assert!(state.inner.with_data(|inner| inner.open));
        take_popup_requests(|_| true);
        take_popup_destroys(|_| true);
    }

    #[test]
    fn a_replaced_text_popup_cannot_dismiss_its_successor() {
        use iced_core::widget::Widget;
        let renderer = renderer();
        let state = MenuBarState::default();
        let parent = window::Id::unique();
        let first_id = queue(parent, &state, &renderer);
        let first = take_popup_requests(|id| id == parent).pop().unwrap();
        let mut popup: TextContextMenuPopup<()> = TextContextMenuPopup {
            menu: first.menu,
            pending_action: first.pending_action,
            _phantom: std::marker::PhantomData,
        };
        let mut tree = Tree::new(&popup as &dyn Widget<(), crate::Theme, crate::Renderer>);
        let limits = Limits::new(Size::ZERO, Size::new(400.0, 300.0));
        let node = popup.layout(&mut tree, &renderer, &limits);
        let second_id = queue(parent, &state, &renderer);
        assert_ne!(first_id, second_id);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        popup.update(
            &mut tree,
            &event::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
                key: iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Escape),
                modified_key: iced_core::keyboard::Key::Named(
                    iced_core::keyboard::key::Named::Escape,
                ),
                physical_key: iced_core::keyboard::key::Physical::Code(
                    iced_core::keyboard::key::Code::Escape,
                ),
                location: iced_core::keyboard::Location::Standard,
                modifiers: iced_core::keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            }),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced_core::clipboard::Null,
            &mut shell,
            &Rectangle::new(Point::ORIGIN, limits.max()),
        );
        assert!(
            state
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
            wake_runtime();
            assert!(matches!(receive(&mut surviving), (id, crate::Action::None) if id == expected));
        }
    }
}
