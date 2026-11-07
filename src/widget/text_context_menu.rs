// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! Right-click context menu for widgets with selectable text.
//!
//! [`TextMenuTarget`] provides native editing and selection. [`TextMenuSession`]
//! presents resolved menu roots and returns generation-checked choices to their
//! source. Application messages remain in the source widget's retained state.
//!
//! Internally uses libcosmic's [`Menu`](crate::widget::menu) system for
//! proper rendering, hover effects, and positioning.
//!
//! On Wayland, the session uses the existing native popup runtime; otherwise
//! [`TextMenuSession::overlay`] supplies the in-window presentation.

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
use std::sync::{Arc, Mutex, Weak};

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

/// A command carried by the native text-menu transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMenuCommand {
    Edit(TextAction),
    Custom(usize),
}

/// Identity of one opening of a source-owned text-menu session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextMenuGeneration(u64);

/// A choice from the snapshot belonging to `generation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextMenuChoice {
    pub generation: TextMenuGeneration,
    pub command: TextMenuCommand,
}

/// Resolved native menu roots and their sizing policy.
#[derive(Clone)]
pub struct TextMenuView {
    pub roots: Vec<menu::Tree<TextMenuCommand>>,
    pub item_width: ItemWidth,
}

#[derive(Default)]
struct SessionMailbox {
    generation: u64,
    open: bool,
    popup_id: Option<iced_core::window::Id>,
    choice: Option<TextMenuChoice>,
}

/// Owns one source's text menu and deferred choice.
///
/// Keep this noncloneable owner in the source widget's retained tree. Popup
/// and overlay views hold only weak, generation-checked handles. Application
/// messages and custom-action snapshots remain with the source, outside the
/// runtime queue. Dropping this owner cancels its native popup.
#[derive(Default)]
pub struct TextMenuSession {
    mailbox: Arc<Mutex<SessionMailbox>>,
    menu_state: MenuBarState,
    overlay_menu: Option<Menu<'static, TextMenuCommand>>,
    native: bool,
}

impl TextMenuSession {
    /// Opens a new snapshot, replacing and invalidating the previous one.
    /// Returns the token to retain alongside the source's custom messages.
    pub fn open(
        &mut self,
        mut view: TextMenuView,
        at: Point,
        renderer: &crate::Renderer,
        viewport: &Rectangle,
        cursor: mouse::Cursor,
        parent_window_id: iced_core::window::Id,
    ) -> TextMenuGeneration {
        let handle = self.begin();
        let generation = handle.generation;
        if view.roots.is_empty() {
            self.mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .open = false;
            return generation;
        }
        view.roots.iter_mut().for_each(menu::Tree::set_index);
        self.native = uses_native_popup() && parent_window_id != iced_core::window::Id::NONE;
        #[cfg(wayland_platform)]
        if self.native {
            queue_text_context_popup(
                at,
                view,
                handle,
                renderer,
                viewport,
                cursor,
                parent_window_id,
            );
            return generation;
        }
        let _ = (renderer, viewport, cursor, parent_window_id);
        self.overlay_menu = Some(make_menu(view, &self.menu_state, at, true));
        generation
    }

    fn begin(&mut self) -> SessionHandle {
        self.dismiss();
        let generation = {
            let mut mailbox = self
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            mailbox.open = true;
            TextMenuGeneration(mailbox.generation)
        };
        SessionHandle {
            owner: Arc::downgrade(&self.mailbox),
            generation,
            menu_state: self.menu_state.clone(),
            popup_id: None,
        }
    }

    /// Takes the committed choice once. Closing its presentation preserves it;
    /// explicitly calling `dismiss` cancels any unconsumed choice.
    pub fn take_choice(&mut self) -> Option<TextMenuChoice> {
        self.mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .choice
            .take()
    }

    /// Whether the current opening still accepts a choice.
    pub fn is_open(&self) -> bool {
        self.mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .open
    }

    /// Cancels the current opening and any unconsumed choice.
    pub fn dismiss(&mut self) {
        let popup_id = {
            let mut mailbox = self
                .mailbox
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            mailbox.generation = mailbox.generation.wrapping_add(1);
            mailbox.open = false;
            mailbox.choice = None;
            mailbox.popup_id.take()
        };
        self.overlay_menu = None;
        self.menu_state.inner.with_data_mut(|state| {
            if let Some(id) = popup_id {
                state.popup_id.retain(|_, popup| *popup != id);
            }
            state.reset();
        });
        #[cfg(wayland_platform)]
        if let Some(id) = popup_id {
            queue_destroy_popup(id);
        }
    }

    /// Handles source-side dismissal for a native popup. Call this after
    /// taking a choice and before forwarding an ordinary source event. Returns
    /// true when an active opening was cancelled.
    pub fn dismiss_on_event(&mut self, event: &event::Event) -> bool {
        if self.native && self.is_open() && is_dismiss_event(event) {
            self.dismiss();
            true
        } else {
            false
        }
    }

    /// Presents the current opening as an in-window overlay when native
    /// popup surfaces are unavailable. Choices still return through this session.
    pub fn overlay<'a, Message: Clone + 'static>(
        &'a mut self,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, crate::Theme, crate::Renderer>> {
        if !self.is_open() {
            return None;
        }
        let mut menu = self.overlay_menu.clone()?;
        menu.position = Point::new(translation.x, translation.y);
        let mailbox = self
            .mailbox
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let handle = SessionHandle {
            owner: Arc::downgrade(&self.mailbox),
            generation: TextMenuGeneration(mailbox.generation),
            menu_state: self.menu_state.clone(),
            popup_id: None,
        };
        Some(overlay::Element::new(Box::new(TextMenuOverlay {
            menu,
            handle,
            _message: std::marker::PhantomData,
        })))
    }
}

impl Drop for TextMenuSession {
    fn drop(&mut self) {
        self.dismiss();
    }
}

#[derive(Clone)]
struct SessionHandle {
    owner: Weak<Mutex<SessionMailbox>>,
    generation: TextMenuGeneration,
    menu_state: MenuBarState,
    popup_id: Option<iced_core::window::Id>,
}

impl SessionHandle {
    fn is_current(&self) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            let mailbox = owner.lock().unwrap_or_else(|error| error.into_inner());
            mailbox.open
                && mailbox.generation == self.generation.0
                && mailbox.popup_id == self.popup_id
        })
    }

    fn commit(&self, command: TextMenuCommand) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let mut mailbox = owner.lock().unwrap_or_else(|error| error.into_inner());
        if !mailbox.open
            || mailbox.generation != self.generation.0
            || mailbox.popup_id != self.popup_id
        {
            return false;
        }
        mailbox.open = false;
        mailbox.choice = Some(TextMenuChoice {
            generation: self.generation,
            command,
        });
        #[cfg(wayland_platform)]
        wake_runtime();
        true
    }

    // Closing the presentation after a committed choice must not erase it.
    fn dismiss(&self) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        {
            let mut mailbox = owner.lock().unwrap_or_else(|error| error.into_inner());
            if mailbox.generation != self.generation.0 || mailbox.popup_id != self.popup_id {
                return false;
            }
            mailbox.open = false;
            mailbox.popup_id = None;
        }
        self.menu_state.inner.with_data_mut(|state| {
            if let Some(id) = self.popup_id {
                state.popup_id.retain(|_, popup| *popup != id);
            }
            state.reset();
        });
        true
    }
}

fn uses_native_popup() -> bool {
    #[cfg(all(wayland_platform, feature = "winit"))]
    if matches!(
        crate::app::cosmic::WINDOWING_SYSTEM.get(),
        Some(crate::app::cosmic::WindowingSystem::Wayland)
    ) {
        return true;
    }
    false
}

pub(crate) fn take_stock_action(session: &mut TextMenuSession) -> Option<TextAction> {
    match session.take_choice()?.command {
        TextMenuCommand::Edit(action) => Some(action),
        TextMenuCommand::Custom(_) => None,
    }
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
    menu: Menu<'static, TextMenuCommand>,
    handle: SessionHandle,
}

#[cfg(wayland_platform)]
thread_local! {
    static PENDING_POPUP_REQUESTS: std::cell::RefCell<Vec<PopupRequest>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn set_current_window_id(id: iced_core::window::Id) {
    CURRENT_WINDOW_ID.set(id);
}

/// Window currently constructing text widgets; capture this in a wrapper constructor.
pub fn current_window_id() -> iced_core::window::Id {
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
        handle,
        live_settings,
    } = req;

    let view = Box::new(move || {
        let popup_widget: TextContextMenuPopup<Message> = TextContextMenuPopup {
            menu: menu.clone(),
            handle: handle.clone(),
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
/// Mirrors the text-menu popup request queue: widgets running
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

fn make_menu(
    view: TextMenuView,
    menu_state: &MenuBarState,
    at: Point,
    is_overlay: bool,
) -> Menu<'static, TextMenuCommand> {
    let mut roots = view.roots;
    roots.iter_mut().for_each(menu::Tree::set_index);
    let bounds = Rectangle {
        x: at.x,
        y: at.y,
        width: 240.0,
        height: 240.0,
    };
    menu_state.inner.with_data_mut(|state| {
        state.reset();
        menu_roots_diff(&mut roots, &mut state.tree);
        state.open = true;
    });
    Menu {
        tree: menu_state.clone(),
        menu_roots: roots,
        bounds_expand: 16,
        menu_overlays_parent: true,
        close_condition: CloseCondition {
            leave: false,
            click_outside: true,
            click_inside: true,
        },
        item_width: view.item_width,
        item_height: ItemHeight::Dynamic(40),
        bar_bounds: bounds,
        main_offset: -(bounds.height as i32),
        cross_offset: 0,
        root_bounds_list: vec![bounds],
        path_highlight: Some(PathHighlight::MenuActive),
        style: Cow::Owned(theme::menu_bar::MenuBarStyle::Default),
        position: Point::ORIGIN,
        is_overlay,
        window_id: iced_core::window::Id::NONE,
        depth: 0,
        on_surface_action: None,
    }
}

pub(crate) fn stock_view(context: TextMenuContext) -> TextMenuView {
    TextMenuView {
        roots: build_menu_roots(context),
        item_width: ItemWidth::Uniform(240),
    }
}

fn build_menu_roots(context: TextMenuContext) -> Vec<menu::Tree<TextMenuCommand>> {
    let is_editable = context.kind == TextKind::Editable;
    let item = |label: &'static str, action: TextCtxAction, enabled: bool| {
        menu::Tree::from(crate::Element::from(
            menu::menu_button(vec![widget::text(label).into()])
                .on_press_maybe(enabled.then_some(TextMenuCommand::Edit(action))),
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

struct TextMenuOverlay<Message: Clone + 'static> {
    menu: Menu<'static, TextMenuCommand>,
    handle: SessionHandle,
    _message: std::marker::PhantomData<Message>,
}

impl<Message: Clone + 'static> overlay::Overlay<Message, crate::Theme, crate::Renderer>
    for TextMenuOverlay<Message>
{
    fn layout(&mut self, renderer: &crate::Renderer, bounds: Size) -> iced_core::layout::Node {
        if !self.handle.is_current() {
            return iced_core::layout::Node::new(Size::ZERO);
        }
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

            let mut init_messages: Vec<TextMenuCommand> = Vec::new();
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
        if self.handle.is_current() {
            self.menu.draw(renderer, theme, style, layout, cursor);
        }
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
        if !self.handle.is_current() {
            return;
        }
        if matches!(
            event,
            event::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
                key: iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Escape),
                ..
            })
        ) {
            self.handle.dismiss();
            shell.capture_event();
            shell.request_redraw();
            return;
        }
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
                self.handle.dismiss();
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
            self.handle.commit(action);
            // The menu closes and the selection may have changed.
            shell.request_redraw();
        }

        let is_open = self.menu.tree.inner.with_data(|state| state.open);
        if !is_open {
            self.handle.dismiss();
        }
    }

    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &crate::Renderer,
    ) -> Option<overlay::Element<'a, Message, crate::Theme, crate::Renderer>> {
        if !self.handle.is_current() {
            return None;
        }
        Some(command_overlay(
            self.menu.child_overlay(layout, renderer)?,
            self.handle.clone(),
        ))
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

fn command_overlay<'a, Message: Clone + 'static>(
    inner: overlay::Element<'a, TextMenuCommand, crate::Theme, crate::Renderer>,
    handle: SessionHandle,
) -> overlay::Element<'a, Message, crate::Theme, crate::Renderer> {
    overlay::Element::new(Box::new(CommandOverlay {
        inner,
        handle,
        _message: std::marker::PhantomData,
    }))
}

struct CommandOverlay<'a, Message> {
    inner: overlay::Element<'a, TextMenuCommand, crate::Theme, crate::Renderer>,
    handle: SessionHandle,
    _message: std::marker::PhantomData<Message>,
}

impl<Message: Clone + 'static> overlay::Overlay<Message, crate::Theme, crate::Renderer>
    for CommandOverlay<'_, Message>
{
    fn layout(&mut self, renderer: &crate::Renderer, bounds: Size) -> iced_core::layout::Node {
        if !self.handle.is_current() {
            return iced_core::layout::Node::new(Size::ZERO);
        }
        self.inner.as_overlay_mut().layout(renderer, bounds)
    }
    fn draw(
        &self,
        renderer: &mut crate::Renderer,
        theme: &crate::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        if self.handle.is_current() {
            self.inner
                .as_overlay()
                .draw(renderer, theme, style, layout, cursor);
        }
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
        if !self.handle.is_current() {
            return;
        }
        let mut messages = Vec::new();
        let mut local = Shell::new(&mut messages);
        self.inner
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, clipboard, &mut local);
        if local.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(local.redraw_request());
        if local.is_layout_invalid() {
            shell.invalidate_layout();
        }
        for command in messages {
            if self.handle.commit(command) {
                self.handle.dismiss();
                #[cfg(wayland_platform)]
                if let Some(id) = self.handle.popup_id {
                    queue_destroy_popup(id);
                }
                shell.request_redraw();
            }
        }
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &crate::Renderer,
    ) -> mouse::Interaction {
        if !self.handle.is_current() {
            return mouse::Interaction::None;
        }
        self.inner
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }
    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &crate::Renderer,
    ) -> Option<overlay::Element<'a, Message, crate::Theme, crate::Renderer>> {
        if !self.handle.is_current() {
            return None;
        }
        Some(command_overlay(
            self.inner.as_overlay_mut().overlay(layout, renderer)?,
            self.handle.clone(),
        ))
    }
}

/// Queues a Wayland popup surface containing the text context menu.
///
/// Pushes a [`PopupRequest`] onto the request queue; `Cosmic::update()`
/// drains it and creates the popup through `get_popup()`, so it flows
/// through the normal Task + `surface_views` pipeline.
#[cfg(wayland_platform)]
fn queue_text_context_popup(
    click_position: Point,
    view: TextMenuView,
    mut handle: SessionHandle,
    renderer: &crate::Renderer,
    viewport: &Rectangle,
    cursor: mouse::Cursor,
    window_id: window::Id,
) {
    use iced_runtime::platform_specific::wayland::popup::{SctkPopupSettings, SctkPositioner};
    let id = window::Id::unique();
    handle.popup_id = Some(id);
    let Some(owner) = handle.owner.upgrade() else {
        return;
    };
    owner
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .popup_id = Some(id);
    let mut popup_menu = make_menu(view, &handle.menu_state, click_position, false);
    popup_menu.window_id = id;
    handle.menu_state.inner.with_data_mut(|state| {
        state.view_cursor = cursor;
        state.popup_id.insert(window_id, id);
    });
    let bounds = popup_menu.bar_bounds;
    {
        let mut init_messages: Vec<TextMenuCommand> = Vec::new();
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

    let anchor_rect = handle.menu_state.inner.with_data_mut(|state| {
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
            handle: handle.clone(),
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

fn is_dismiss_event(event: &event::Event) -> bool {
    matches!(
        event,
        event::Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left | mouse::Button::Middle
        )) | event::Event::Keyboard(iced_core::keyboard::Event::KeyPressed {
            key: iced_core::keyboard::Key::Named(iced_core::keyboard::key::Named::Escape),
            ..
        }) | event::Event::Touch(iced_core::touch::Event::FingerPressed { .. })
    )
}

/// Widget that wraps [`Menu`] inside a Wayland popup and intercepts
/// [`TextMenuCommand`] choices for source-side execution.
#[derive(Clone)]
struct TextContextMenuPopup<Message: Clone + 'static> {
    menu: Menu<'static, TextMenuCommand>,
    handle: SessionHandle,
    _phantom: std::marker::PhantomData<Message>,
}

impl<Message: Clone + 'static> TextContextMenuPopup<Message> {
    fn is_current_popup(&self) -> bool {
        self.handle.is_current()
    }

    fn dismiss(&self) -> bool {
        self.handle.dismiss()
    }
}

impl<Message: Clone + 'static> iced_core::widget::Widget<Message, crate::Theme, crate::Renderer>
    for TextContextMenuPopup<Message>
{
    fn size(&self) -> Size<iced_core::Length> {
        use iced_core::widget::Widget;
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::size(&self.menu)
    }

    fn tag(&self) -> iced_core::widget::tree::Tag {
        use iced_core::widget::Widget;
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::tag(&self.menu)
    }

    fn state(&self) -> iced_core::widget::tree::State {
        use iced_core::widget::Widget;
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::state(&self.menu)
    }

    fn children(&self) -> Vec<iced_core::widget::Tree> {
        use iced_core::widget::Widget;
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::children(&self.menu)
    }

    fn diff(&mut self, tree: &mut iced_core::widget::Tree) {
        use iced_core::widget::Widget;
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::diff(&mut self.menu, tree);
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
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::layout(
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
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::draw(
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

        let mut local_messages: Vec<TextMenuCommand> = Vec::new();
        let mut local_shell = Shell::new(&mut local_messages);

        {
            use iced_core::widget::Widget;
            Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::update(
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
            self.handle.commit(action);
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

    fn overlay<'a>(
        &'a mut self,
        _tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &crate::Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'a, Message, crate::Theme, crate::Renderer>> {
        if !self.is_current_popup() {
            return None;
        }
        Some(command_overlay(
            self.menu.child_overlay(layout, renderer)?,
            self.handle.clone(),
        ))
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
        Widget::<TextMenuCommand, crate::Theme, crate::Renderer>::mouse_interaction(
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
mod lifecycle_tests;
