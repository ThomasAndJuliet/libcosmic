// From iced_aw, license MIT

//! A widget that handles menu trees
use std::collections::HashMap;
use std::sync::Arc;

use super::menu_inner::{
    CloseCondition, Direction, ItemHeight, ItemWidth, Menu, MenuState, PathHighlight,
    close_innermost,
};
use super::menu_tree::MenuTree;
use crate::Renderer;
#[cfg(wayland_platform)]
use crate::app::cosmic::{WINDOWING_SYSTEM, WindowingSystem};
use crate::style::menu_bar::StyleSheet;
use crate::widget::RcWrapper;
use crate::widget::dropdown::menu::{self, State};
use crate::widget::menu::menu_inner::init_root_menu;

use iced::event::Status;
use iced::{Point, Shadow, Vector, window};
use iced_core::Border;
use iced_widget::core::layout::{Limits, Node};
use iced_widget::core::mouse::{self, Cursor};
use iced_widget::core::renderer::{self, Renderer as IcedRenderer};
use iced_widget::core::widget::{Tree, tree};
use iced_widget::core::{
    Alignment, Clipboard, Element, Layout, Length, Padding, Rectangle, Shell, Widget, event,
    overlay, touch,
};

/// A `MenuBar` collects `MenuTree`s and handles all the layout, event processing, and drawing.
pub fn menu_bar<Message>(menu_roots: Vec<MenuTree<Message>>) -> MenuBar<Message>
where
    Message: Clone + 'static,
{
    MenuBar::new(menu_roots)
}

#[derive(Clone, Default)]
pub(crate) struct MenuBarState {
    pub(crate) inner: RcWrapper<MenuBarStateInner>,
    // Cloned popups retain the model generation they were constructed for.
    model: Option<Arc<MenuShape>>,
}

type MenuShape = Vec<Vec<(tree::Tag, usize)>>;

impl MenuBarState {
    pub(super) fn is_current(&self) -> bool {
        self.inner.with_data(|inner| match (&self.model, &inner.model) {
            (Some(own), Some(current)) => Arc::ptr_eq(own, current),
            (None, None) => true,
            _ => false,
        })
    }

    pub(super) fn close_outdated_popups<Message>(
        &self,
        handler: Option<&Arc<dyn Fn(crate::surface::Action<Message>) -> Message + Send + Sync + 'static>>,
        shell: &mut Shell<'_, Message>,
    ) {
        if let Some(handler) = handler {
            self.inner.with_data_mut(|inner| {
                for id in inner.outdated_popups.drain(..) {
                    shell.publish(handler(crate::surface::Action::DestroyPopup(id)));
                }
            });
        }
    }
}

pub(crate) struct MenuBarStateInner {
    pub(crate) tree: Tree,
    model: Option<Arc<MenuShape>>,
    outdated_popups: Vec<window::Id>,
    pub(crate) popup_id: HashMap<window::Id, window::Id>,
    pub(crate) pressed: bool,
    pub(crate) bar_pressed: bool,
    pub(crate) view_cursor: Cursor,
    pub(crate) open: bool,
    pub(crate) active_root: Vec<usize>,
    pub(crate) horizontal_direction: Direction,
    pub(crate) vertical_direction: Direction,
    /// List of all menu states
    pub(crate) menu_states: Vec<MenuState>,
}
impl MenuBarStateInner {
    /// get the list of indices hovered for the menu
    pub(super) fn get_trimmed_indices(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        self.menu_states
            .iter()
            .skip(index)
            .take_while(|ms| ms.index.is_some())
            .map(|ms| ms.index.expect("No indices were found in the menu state."))
    }

    pub(crate) fn reset(&mut self) {
        self.open = false;
        self.active_root = Vec::new();
        self.menu_states.clear();
    }
}
impl Default for MenuBarStateInner {
    fn default() -> Self {
        Self {
            tree: Tree::empty(),
            model: None,
            outdated_popups: Vec::new(),
            pressed: false,
            view_cursor: Cursor::Available([-0.5, -0.5].into()),
            open: false,
            active_root: Vec::new(),
            horizontal_direction: Direction::Positive,
            vertical_direction: Direction::Positive,
            menu_states: Vec::new(),
            popup_id: HashMap::new(),
            bar_pressed: false,
        }
    }
}

pub(crate) fn menu_roots_children<Message>(menu_roots: &[MenuTree<Message>]) -> Vec<Tree>
where
    Message: Clone + 'static,
{
    /*
    menu bar
        menu root 1 (stateless)
            flat tree
        menu root 2 (stateless)
            flat tree
        ...
    */

    menu_roots
        .iter()
        .map(|root| {
            let mut tree = Tree::empty();
            let flat = root
                .flattern()
                .iter()
                .map(|mt| Tree::new(mt.item.clone()))
                .collect();
            tree.children = flat;
            tree
        })
        .collect()
}

#[allow(invalid_reference_casting)]
pub(crate) fn menu_roots_diff<Message>(menu_roots: &mut [MenuTree<Message>], tree: &mut Tree)
where
    Message: Clone + 'static,
{
    if tree.children.len() > menu_roots.len() {
        tree.children.truncate(menu_roots.len());
    }

    tree.children
        .iter_mut()
        .zip(menu_roots.iter())
        .for_each(|(t, root)| {
            let mut flat = root
                .flattern()
                .iter()
                .map(|mt| {
                    let widget = &mt.item;
                    let widget_ptr = widget as *const dyn Widget<Message, crate::Theme, Renderer>;
                    let widget_ptr_mut =
                        widget_ptr as *mut dyn Widget<Message, crate::Theme, Renderer>;
                    //TODO: find a way to diff_children without unsafe code
                    unsafe { &mut *widget_ptr_mut }
                })
                .collect::<Vec<_>>();

            t.diff_children(flat.as_mut_slice());
        });

    if tree.children.len() < menu_roots.len() {
        let extended = menu_roots[tree.children.len()..].iter().map(|root| {
            let mut tree = Tree::empty();
            let flat = root
                .flattern()
                .iter()
                .map(|mt| Tree::new(mt.item.clone()))
                .collect();
            tree.children = flat;
            tree
        });
        tree.children.extend(extended);
    }
}

pub fn get_mut_or_default<T: Default>(vec: &mut Vec<T>, index: usize) -> &mut T {
    if index < vec.len() {
        &mut vec[index]
    } else {
        vec.resize_with(index + 1, T::default);
        &mut vec[index]
    }
}

/// A `MenuBar` collects `MenuTree`s and handles all the layout, event processing, and drawing.
#[allow(missing_debug_implementations)]
pub struct MenuBar<Message> {
    width: Length,
    height: Length,
    spacing: f32,
    padding: Padding,
    bounds_expand: u16,
    main_offset: i32,
    cross_offset: i32,
    close_condition: CloseCondition,
    item_width: ItemWidth,
    item_height: ItemHeight,
    path_highlight: Option<PathHighlight>,
    menu_roots: Vec<MenuTree<Message>>,
    style: <crate::Theme as StyleSheet>::Style,
    window_id: window::Id,
    #[cfg(wayland_platform)]
    positioner: iced_runtime::platform_specific::wayland::popup::SctkPositioner,
    pub(crate) on_surface_action:
        Option<Arc<dyn Fn(crate::surface::Action<Message>) -> Message + Send + Sync + 'static>>,
}

impl<Message> MenuBar<Message>
where
    Message: Clone + 'static,
{
    /// Creates a new [`MenuBar`] with the given menu roots
    #[must_use]
    pub fn new(menu_roots: Vec<MenuTree<Message>>) -> Self {
        let mut menu_roots = menu_roots;
        menu_roots.iter_mut().for_each(MenuTree::set_index);

        Self {
            width: Length::Shrink,
            height: Length::Shrink,
            spacing: 0.0,
            padding: Padding::ZERO,
            bounds_expand: 16,
            main_offset: 0,
            cross_offset: 0,
            close_condition: CloseCondition {
                leave: false,
                click_outside: true,
                click_inside: true,
            },
            item_width: ItemWidth::Uniform(150),
            item_height: ItemHeight::Dynamic(40),
            path_highlight: Some(PathHighlight::MenuActive),
            menu_roots,
            style: <crate::Theme as StyleSheet>::Style::default(),
            window_id: window::Id::RESERVED,
            #[cfg(wayland_platform)]
            positioner: iced_runtime::platform_specific::wayland::popup::SctkPositioner::default(),
            on_surface_action: None,
        }
    }

    /// Sets the expand value for each menu's check bounds
    ///
    /// When the cursor goes outside of a menu's check bounds,
    /// the menu will be closed automatically, this value expands
    /// the check bounds
    #[must_use]
    pub fn bounds_expand(mut self, value: u16) -> Self {
        self.bounds_expand = value;
        self
    }

    /// [`CloseCondition`]
    #[must_use]
    pub fn close_condition(mut self, close_condition: CloseCondition) -> Self {
        self.close_condition = close_condition;
        self
    }

    /// Moves each menu in the horizontal open direction
    #[must_use]
    pub fn cross_offset(mut self, value: i32) -> Self {
        self.cross_offset = value;
        self
    }

    /// Sets the height of the [`MenuBar`]
    #[must_use]
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// [`ItemHeight`]
    #[must_use]
    pub fn item_height(mut self, item_height: ItemHeight) -> Self {
        self.item_height = item_height;
        self
    }

    /// [`ItemWidth`]
    #[must_use]
    pub fn item_width(mut self, item_width: ItemWidth) -> Self {
        self.item_width = item_width;
        self
    }

    /// Moves all the menus in the vertical open direction
    #[must_use]
    pub fn main_offset(mut self, value: i32) -> Self {
        self.main_offset = value;
        self
    }

    /// Sets the [`Padding`] of the [`MenuBar`]
    #[must_use]
    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    /// Sets the method for drawing path highlight
    #[must_use]
    pub fn path_highlight(mut self, path_highlight: Option<PathHighlight>) -> Self {
        self.path_highlight = path_highlight;
        self
    }

    /// Sets the spacing between menu roots
    #[must_use]
    pub fn spacing(mut self, units: f32) -> Self {
        self.spacing = units;
        self
    }

    /// Sets the style of the menu bar and its menus
    #[must_use]
    pub fn style(mut self, style: impl Into<<crate::Theme as StyleSheet>::Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets the width of the [`MenuBar`]
    #[must_use]
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    #[cfg(wayland_platform)]
    pub fn with_positioner(
        mut self,
        positioner: iced_runtime::platform_specific::wayland::popup::SctkPositioner,
    ) -> Self {
        self.positioner = positioner;
        self
    }

    #[must_use]
    pub fn window_id(mut self, id: window::Id) -> Self {
        self.window_id = id;
        self
    }

    #[must_use]
    pub fn window_id_maybe(mut self, id: Option<window::Id>) -> Self {
        if let Some(id) = id {
            self.window_id = id;
        }
        self
    }

    #[must_use]
    pub fn on_surface_action(
        mut self,
        handler: impl Fn(crate::surface::Action<Message>) -> Message + Send + Sync + 'static,
    ) -> Self {
        self.on_surface_action = Some(Arc::new(handler));
        self
    }

    #[cfg(wayland_platform)]
    #[allow(clippy::too_many_lines)]
    fn create_popup(
        &mut self,
        layout: Layout<'_>,
        view_cursor: Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
        my_state: &mut MenuBarState,
    ) {
        if self.window_id != window::Id::NONE && self.on_surface_action.is_some() {
            use crate::surface::action::{LiveSettings, destroy_popup};
            use crate::theme::THEME;
            use iced_runtime::platform_specific::wayland::CornerRadius;
            use iced_runtime::platform_specific::wayland::popup::{
                SctkPopupSettings, SctkPositioner,
            };

            let surface_action = self.on_surface_action.as_ref().unwrap();
            let old_active_root = my_state
                .inner
                .with_data(|state| state.active_root.first().copied());

            // if position is not on menu bar button skip.
            let hovered_root = layout
                .children()
                .position(|lo| view_cursor.is_over(lo.bounds()));
            if hovered_root.is_none()
                || old_active_root
                    .zip(hovered_root)
                    .is_some_and(|r| r.0 == r.1)
            {
                return;
            }

            let (id, root_list) = my_state.inner.with_data_mut(|state| {
                if let Some(id) = state.popup_id.get(&self.window_id).copied() {
                    // close existing popups
                    state.menu_states.clear();
                    state.active_root.clear();
                    shell.publish(surface_action(destroy_popup(id)));
                    state.view_cursor = view_cursor;
                }
                // A fresh id per popup, so the old popup's Done cannot be mistaken for the new one's
                (
                    window::Id::unique(),
                    layout.children().map(|lo| lo.bounds()).collect(),
                )
            });

            let mut popup_menu: Menu<'static, _> = Menu {
                tree: my_state.clone(),
                menu_roots: self.menu_roots.clone(),
                bounds_expand: self.bounds_expand,
                menu_overlays_parent: false,
                close_condition: self.close_condition,
                item_width: self.item_width,
                item_height: self.item_height,
                bar_bounds: layout.bounds(),
                main_offset: self.main_offset,
                cross_offset: self.cross_offset,
                root_bounds_list: root_list,
                path_highlight: self.path_highlight,
                style: std::borrow::Cow::Owned(self.style.clone()),
                position: Point::new(0., 0.),
                is_overlay: false,
                window_id: id,
                depth: 0,
                on_surface_action: self.on_surface_action.clone(),
            };

            init_root_menu(
                &mut popup_menu,
                renderer,
                shell,
                view_cursor.position().unwrap(),
                viewport.size(),
                Vector::new(0., 0.),
                layout.bounds(),
                self.main_offset as f32,
            );
            let (anchor_rect, gravity) = my_state.inner.with_data_mut(|state| {
                state.popup_id.insert(self.window_id, id);
                (state
                    .menu_states
                    .iter()
                    .find(|s| s.index.is_none())
                    .map(|s| s.menu_bounds.parent_bounds)
                    .map_or_else(
                        || {
                            let bounds = layout.bounds();
                            Rectangle {
                                x: bounds.x as i32,
                                y: bounds.y as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            }
                        },
                        |r| Rectangle {
                            x: r.x as i32,
                            y: r.y as i32,
                            width: r.width as i32,
                            height: r.height as i32,
                        },
                    ), match (state.horizontal_direction, state.vertical_direction) {
                        (Direction::Positive, Direction::Positive) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::BottomRight,
                        (Direction::Positive, Direction::Negative) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::TopRight,
                        (Direction::Negative, Direction::Positive) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::BottomLeft,
                        (Direction::Negative, Direction::Negative) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::TopLeft,
                    })
            });

            let menu_node = popup_menu.layout(renderer, Limits::NONE.min_width(1.).min_height(1.));
            let popup_size = menu_node.size();
            let positioner = SctkPositioner {
                size: Some((
                    popup_size.width.ceil() as u32 + 2,
                    popup_size.height.ceil() as u32 + 2,
                )),
                anchor_rect,
                anchor:
                    cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Anchor::BottomLeft,
                gravity,
                reactive: true,
                ..Default::default()
            };
            let parent = self.window_id;

            let t = THEME.lock().unwrap();
            let styling = t.appearance(&crate::theme::menu_bar::MenuBarStyle::Default, false);
            drop(t);
            let rad = styling.menu_border_radius;

            shell.publish((surface_action)(crate::surface::action::simple_popup(
                move || LiveSettings {
                    corners: Some(CornerRadius {
                        top_left: rad[0] as u32,
                        top_right: rad[1] as u32,
                        bottom_left: rad[2] as u32,
                        bottom_right: rad[3] as u32,
                    }),
                    ..Default::default()
                },
                move || SctkPopupSettings {
                    parent,
                    id,
                    positioner: positioner.clone(),
                    parent_size: None,
                    grab: true,
                    close_with_children: false,
                    input_zone: None,
                },
                Some(move || {
                    Element::from(crate::widget::container(popup_menu.clone()).center(Length::Fill))
                        .map(crate::action::app)
                }),
            )));
        }
    }
}
impl<Message> Widget<Message, crate::Theme, Renderer> for MenuBar<Message>
where
    Message: Clone + 'static,
{
    fn size(&self) -> iced_core::Size<Length> {
        iced_core::Size::new(self.width, self.height)
    }

    fn diff(&mut self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<MenuBarState>();
        let shape: MenuShape = self.menu_roots.iter().map(|root| {
            root.flattern().iter().map(|node| (node.item.tag(), node.children.len())).collect()
        }).collect();
        state.inner.with_data_mut(|inner| {
            if inner.model.as_deref() != Some(&shape) {
                // An existing native popup still owns the old menu model.
                // Invalidate it before replacing the shared widget tree, and
                // retain its IDs until an event can request their destruction.
                if inner.model.is_some() {
                    inner.reset();
                    inner.outdated_popups.extend(inner.popup_id.drain().map(|(_, id)| id));
                }
                inner.model = Some(Arc::new(shape));
            }
            menu_roots_diff(&mut self.menu_roots, &mut inner.tree);
        });
        state.model = state.inner.with_data(|inner| inner.model.clone());
        // Root widgets have their own state tree, separate from the popup's
        // flattened items. Reconcile both when roots are added or removed.
        menu_roots_diff(&mut self.menu_roots, tree);
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MenuBarState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MenuBarState::default())
    }

    fn children(&self) -> Vec<Tree> {
        menu_roots_children(&self.menu_roots)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        use super::flex;

        let limits = limits.width(self.width).height(self.height);
        let mut children = self
            .menu_roots
            .iter_mut()
            .map(|root| &mut root.item)
            .collect::<Vec<_>>();
        // the first children of the tree are the menu roots items
        let mut tree_children = tree
            .children
            .iter_mut()
            .map(|t| &mut t.children[0])
            .collect::<Vec<_>>();
        flex::resolve_wrapper(
            &flex::Axis::Horizontal,
            renderer,
            &limits,
            self.padding,
            self.spacing,
            Alignment::Center,
            &mut children,
            &mut tree_children,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &event::Event,
        layout: Layout<'_>,
        view_cursor: Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        use event::Event::{Mouse, Touch};
        use mouse::Button::Left;
        use mouse::Event::ButtonReleased;
        use touch::Event::{FingerLifted, FingerLost};

        tree.state.downcast_ref::<MenuBarState>().close_outdated_popups(
            self.on_surface_action.as_ref(), shell,
        );
        process_root_events(
            &mut self.menu_roots,
            view_cursor,
            tree,
            event,
            layout,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let my_state = tree.state.downcast_mut::<MenuBarState>();

        // The compositor dismissed our popup: nothing else tells this state about it.
        #[cfg(wayland_platform)]
        if let iced::Event::PlatformSpecific(iced::event::PlatformSpecific::Wayland(
            iced::event::wayland::Event::Popup(iced::event::wayland::PopupEvent::Done, _, popup),
        )) = event
        {
            my_state.inner.with_data_mut(|d| {
                if d.popup_id.get(&self.window_id) == Some(popup) {
                    // submenus were dismissed with it
                    d.popup_id.clear();
                    d.reset();
                }
            });
        }

        // XXX this should reset the state if there are no other copies of the state, which implies no dropdown menus open.
        let reset = self.window_id != window::Id::NONE
            && my_state
                .inner
                .with_data(|d| !d.open && !d.active_root.is_empty());

        let open = my_state.inner.with_data_mut(|state| {
            if reset {
                if let Some(popup_id) = state.popup_id.get(&self.window_id).copied() {
                    if let Some(handler) = self.on_surface_action.as_ref() {
                        shell.publish((handler)(crate::surface::Action::DestroyPopup(popup_id)));
                        state.reset();
                    }
                }
            }
            state.open
        });

        match event {
            event::Event::Keyboard(iced_widget::core::keyboard::Event::KeyPressed {
                key:
                    iced_widget::core::keyboard::Key::Named(
                        iced_widget::core::keyboard::key::Named::Escape,
                    ),
                ..
            }) if open => {
                my_state.inner.with_data_mut(|state| {
                    let handler = self.on_surface_action.as_ref();
                    close_innermost(state, &mut |id| {
                        if let Some(handler) = handler {
                            shell.publish((handler)(crate::surface::Action::DestroyPopup(id)));
                        }
                    });
                });
                shell.capture_event();
            }
            Mouse(mouse::Event::ButtonPressed(Left))
            | Touch(touch::Event::FingerPressed { .. })
                if view_cursor.is_over(layout.bounds()) =>
            {
                // TODO should we track that it has been pressed?
                shell.capture_event();
            }
            Mouse(ButtonReleased(Left)) | Touch(FingerLifted { .. } | FingerLost { .. }) => {
                let create_popup = my_state.inner.with_data_mut(|state| {
                    let mut create_popup = false;
                    if state.menu_states.is_empty() && view_cursor.is_over(layout.bounds()) {
                        state.view_cursor = view_cursor;
                        state.open = true;
                        create_popup = true;
                    } else if let Some(_id) = state.popup_id.remove(&self.window_id) {
                        state.menu_states.clear();
                        state.active_root.clear();
                        state.open = false;
                        #[cfg(wayland_platform)]
                        {
                            let surface_action = self.on_surface_action.as_ref().unwrap();
                            shell.capture_event();

                            shell.publish(surface_action(crate::surface::action::destroy_popup(
                                _id,
                            )));
                        }
                        state.view_cursor = view_cursor;
                    }
                    create_popup
                });

                if !create_popup {
                    return;
                }
                shell.capture_event();
                shell.request_redraw();
                #[cfg(wayland_platform)]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland)) {
                    self.create_popup(layout, view_cursor, renderer, shell, viewport, my_state);
                }
            }
            Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorEntered)
                if open && view_cursor.is_over(layout.bounds()) =>
            {
                shell.request_redraw();
                shell.capture_event();
                #[cfg(wayland_platform)]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland)) {
                    self.create_popup(layout, view_cursor, renderer, shell, viewport, my_state);
                }
            }
            _ => (),
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &crate::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        view_cursor: Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<MenuBarState>();
        let cursor_pos = view_cursor.position().unwrap_or_default();
        state.inner.with_data_mut(|state| {
            let position = if state.open && (cursor_pos.x < 0.0 || cursor_pos.y < 0.0) {
                state.view_cursor
            } else {
                view_cursor
            };

            // draw path highlight
            if self.path_highlight.is_some() {
                let mut is_overlay = true;
                #[cfg(wayland_platform)]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland))
                    && self.on_surface_action.is_some()
                    && self.window_id != window::Id::NONE
                {
                    is_overlay = true;
                };
                let styling = theme.appearance(&self.style, is_overlay);
                if let Some(active) = state.active_root.first() {
                    let active_bounds = layout
                        .children()
                        .nth(*active)
                        .expect("Active child not found in menu?")
                        .bounds();
                    let path_quad = renderer::Quad {
                        bounds: active_bounds,
                        border: Border {
                            radius: styling.bar_border_radius.into(),
                            ..Default::default()
                        },
                        shadow: Shadow::default(),
                        snap: true,
                    };

                    renderer.fill_quad(path_quad, styling.path);
                }
            }

            self.menu_roots
                .iter()
                .zip(&tree.children)
                .zip(layout.children())
                .for_each(|((root, t), lo)| {
                    root.item.draw(
                        &t.children[root.index],
                        renderer,
                        theme,
                        style,
                        lo,
                        position,
                        viewport,
                    );
                });
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, crate::Theme, Renderer>> {
        #[cfg(wayland_platform)]
        if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland))
            && self.on_surface_action.is_some()
            && self.window_id != window::Id::NONE
        {
            return popup_shield(
                tree.state.downcast_ref::<MenuBarState>(),
                self.window_id,
                layout.bounds() + translation,
                self.on_surface_action.as_ref(),
            );
        }

        let state = tree.state.downcast_ref::<MenuBarState>();
        if state.inner.with_data(|state| !state.open) {
            return None;
        }

        Some(
            Menu {
                tree: state.clone(),
                menu_roots: self.menu_roots.clone(),
                bounds_expand: self.bounds_expand,
                menu_overlays_parent: false,
                close_condition: self.close_condition,
                item_width: self.item_width,
                item_height: self.item_height,
                bar_bounds: layout.bounds(),
                main_offset: self.main_offset,
                cross_offset: self.cross_offset,
                root_bounds_list: layout.children().map(|lo| lo.bounds()).collect(),
                path_highlight: self.path_highlight,
                style: std::borrow::Cow::Borrowed(&self.style),
                position: Point::new(translation.x, translation.y),
                is_overlay: true,
                window_id: window::Id::NONE,
                depth: 0,
                on_surface_action: self.on_surface_action.clone(),
            }
            .overlay(),
        )
    }
}

impl<Message> From<MenuBar<Message>> for Element<'_, Message, crate::Theme, Renderer>
where
    Message: Clone + 'static,
{
    fn from(value: MenuBar<Message>) -> Self {
        Self::new(value)
    }
}

/// While a menu's native popup is open, an overlay over its parent window that takes a
/// press anywhere but on `bar`: it closes the popup and keeps the press from the widgets
/// under it. A press on `bar` is left to the menu bar.
#[cfg(wayland_platform)]
pub(crate) fn popup_shield<'a, Message: 'a>(
    state: &MenuBarState,
    window_id: window::Id,
    bar: Rectangle,
    on_surface_action: Option<
        &Arc<dyn Fn(crate::surface::Action<Message>) -> Message + Send + Sync + 'static>,
    >,
) -> Option<overlay::Element<'a, Message, crate::Theme, Renderer>> {
    struct Shield<Message> {
        state: MenuBarState,
        window_id: window::Id,
        bar: Rectangle,
        on_surface_action: Arc<dyn Fn(crate::surface::Action<Message>) -> Message + Send + Sync>,
    }

    impl<Message> overlay::Overlay<Message, crate::Theme, Renderer> for Shield<Message> {
        fn layout(&mut self, _renderer: &Renderer, bounds: iced::Size) -> Node {
            Node::new(bounds)
        }

        fn draw(
            &self,
            _renderer: &mut Renderer,
            _theme: &crate::Theme,
            _style: &renderer::Style,
            _layout: Layout<'_>,
            _cursor: Cursor,
        ) {
        }

        fn update(
            &mut self,
            event: &event::Event,
            _layout: Layout<'_>,
            cursor: Cursor,
            _renderer: &Renderer,
            _clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
        ) {
            if matches!(
                event,
                event::Event::Mouse(mouse::Event::ButtonPressed(_))
                    | event::Event::Touch(touch::Event::FingerPressed { .. })
            ) && !cursor.is_over(self.bar)
            {
                self.state.inner.with_data_mut(|state| {
                    if let Some(id) = state.popup_id.remove(&self.window_id) {
                        shell.publish((self.on_surface_action)(
                            crate::surface::action::destroy_popup(id),
                        ));
                        state.reset();
                        shell.capture_event();
                    }
                });
            }
        }
    }

    let on_surface_action = on_surface_action?.clone();
    state
        .inner
        .with_data(|d| d.popup_id.contains_key(&window_id))
        .then(|| {
            overlay::Element::new(Box::new(Shield {
                state: state.clone(),
                window_id,
                bar,
                on_surface_action,
            }))
        })
}

#[allow(unused_results, clippy::too_many_arguments)]
fn process_root_events<Message>(
    menu_roots: &mut [MenuTree<Message>],
    view_cursor: Cursor,
    tree: &mut Tree,
    event: &event::Event,
    layout: Layout<'_>,
    renderer: &Renderer,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
    viewport: &Rectangle,
) {
    for ((root, t), lo) in menu_roots
        .iter_mut()
        .zip(&mut tree.children)
        .zip(layout.children())
    {
        // assert!(t.tag == tree::Tag::stateless());
        root.item.update(
            &mut t.children[root.index],
            event,
            lo,
            view_cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use iced_runtime::user_interface::{Cache, UserInterface};

    #[derive(Clone)]
    enum Message {
        Item(&'static str),
        Under,
        #[cfg_attr(not(wayland_platform), allow(dead_code))]
        Surface(crate::surface::Action<Self>),
    }

    fn renderer() -> Renderer {
        iced::futures::executor::block_on(<Renderer as Headless>::new(
            iced::Font::default(),
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer")
    }

    fn item() -> MenuTree<Message> {
        MenuTree::from(crate::Element::from(
            crate::widget::button::text("Item").on_press(Message::Item("Item")),
        ))
    }

    /// File, Edit, View as text roots, as a wide window shows them
    fn wide() -> crate::Element<'static, Message> {
        let roots = ["File", "Edit", "View"]
            .into_iter()
            .map(|title| {
                MenuTree::with_children(
                    crate::widget::RcElementWrapper::new(crate::Element::from(super::super::root(
                        title,
                    ))),
                    vec![item(), item()],
                )
            })
            .collect();
        menu_bar(roots).into()
    }

    /// The same menus folded into one icon button with a tooltip, as Cerea
    /// shows them in a narrow window
    fn folded() -> crate::Element<'static, Message> {
        let button =
            crate::widget::button::icon(crate::widget::icon::from_name("open-menu-symbolic"))
                .class(crate::theme::Button::MenuRoot);
        let root = crate::widget::tooltip(
            button,
            crate::widget::text("Menu"),
            crate::widget::tooltip::Position::Bottom,
        );
        let menus = ["File", "Edit", "View"]
            .into_iter()
            .map(|title| {
                MenuTree::with_children(
                    crate::widget::RcElementWrapper::new(crate::Element::from(
                        crate::widget::text(title),
                    )),
                    vec![item()],
                )
            })
            .collect::<Vec<_>>();
        menu_bar(vec![MenuTree::with_children(
            crate::widget::RcElementWrapper::new(crate::Element::from(root)),
            menus,
        )])
        .into()
    }

    #[test]
    fn roots_that_change_kind_are_laid_out_with_their_own_state() {
        let mut renderer = renderer();
        let size = iced::Size::new(600.0, 40.0);
        let mut cache = Cache::default();
        // Wide, folded, wide again: each view lays out against the last one's tree
        for view in [wide, folded, wide, folded] {
            let mut ui = UserInterface::build(view(), size, cache, &mut renderer);
            _ = ui.update(
                &[iced::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(10.0, 10.0),
                })],
                Cursor::Available(Point::new(10.0, 10.0)),
                &mut renderer,
                &mut iced_core::clipboard::Null,
                &mut Vec::new(),
            );
            cache = ui.into_cache();
        }
    }

    /// A button, then File and Edit menus of one item each, as Cerea's header has
    /// its sidebar toggle before its menu bar
    fn button_then_bar(window: Option<window::Id>) -> crate::Element<'static, Message> {
        let roots = ["File", "Edit"]
            .into_iter()
            .map(|title| {
                let root = super::super::root::<Message, Renderer>(title)
                    .width(80)
                    .height(40);
                let item = crate::widget::button::text(title)
                    .on_press(Message::Item(title))
                    .width(Length::Fill);
                MenuTree::with_children(
                    crate::widget::RcElementWrapper::new(crate::Element::from(root)),
                    vec![MenuTree::from(crate::Element::from(item))],
                )
            })
            .collect();
        let mut bar = menu_bar(roots);
        if let Some(window) = window {
            bar = bar.window_id(window).on_surface_action(Message::Surface);
        }
        crate::widget::row::with_children(vec![
            crate::widget::button::text("Under")
                .on_press(Message::Under)
                .width(60)
                .height(40)
                .into(),
            bar.into(),
        ])
        .into()
    }

    const UNDER: Point = Point::new(30.0, 20.0);
    const FILE: Point = Point::new(100.0, 20.0);
    const EDIT: Point = Point::new(180.0, 20.0);
    /// Edit's item
    const ITEM: Point = Point::new(190.0, 56.0);

    /// Moves to `at`, then presses and releases `button` there, one frame each.
    /// Returns every message, and whether the press was captured.
    fn click<M>(
        ui: &mut UserInterface<'_, M, crate::Theme, Renderer>,
        renderer: &mut Renderer,
        at: Point,
        button: mouse::Button,
    ) -> (Vec<M>, bool) {
        let mut messages = Vec::new();
        let mut captured = false;
        for event in [
            mouse::Event::CursorMoved { position: at },
            mouse::Event::ButtonPressed(button),
            mouse::Event::ButtonReleased(button),
        ] {
            let (_, statuses) = ui.update(
                &[iced::Event::Mouse(event)],
                Cursor::Available(at),
                renderer,
                &mut iced_core::clipboard::Null,
                &mut messages,
            );
            if matches!(event, mouse::Event::ButtonPressed(_)) {
                captured = statuses[0] == event::Status::Captured;
            }
        }
        (messages, captured)
    }

    fn hover<M>(
        ui: &mut UserInterface<'_, M, crate::Theme, Renderer>,
        renderer: &mut Renderer,
        at: Point,
    ) -> Vec<M> {
        let mut messages = Vec::new();
        _ = ui.update(
            &[iced::Event::Mouse(mouse::Event::CursorMoved {
                position: at,
            })],
            Cursor::Available(at),
            renderer,
            &mut iced_core::clipboard::Null,
            &mut messages,
        );
        messages
    }

    /// Clicks a root, then moves on it as a pointer does, so an in-window menu is laid out
    fn open(
        ui: &mut UserInterface<'_, Message, crate::Theme, Renderer>,
        renderer: &mut Renderer,
        root: Point,
    ) -> Vec<Message> {
        let (messages, _) = click(ui, renderer, root, mouse::Button::Left);
        _ = hover(ui, renderer, root + Vector::new(1.0, 0.0));
        messages
    }

    fn under(messages: &[Message]) -> bool {
        messages.iter().any(|m| matches!(m, Message::Under))
    }

    #[test]
    fn a_click_outside_an_in_window_menu_only_closes_it() {
        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            button_then_bar(None),
            iced::Size::new(400.0, 300.0),
            Cache::default(),
            &mut renderer,
        );
        _ = open(&mut ui, &mut renderer, FILE);
        // The button cannot see the pointer under the menu, but the press used to
        // reach every widget that acts without it
        let (messages, captured) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(captured, "the closing press is swallowed");
        assert!(!under(&messages));
        // The menu is closed, so the next click reaches the button
        let (messages, _) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(under(&messages));

        // Moving to another root with File open opens Edit, whose item takes clicks
        _ = open(&mut ui, &mut renderer, FILE);
        // Two moves, as the first only closes File's menu
        _ = hover(&mut ui, &mut renderer, EDIT);
        _ = hover(&mut ui, &mut renderer, EDIT + Vector::new(1.0, 0.0));
        let (messages, _) = click(&mut ui, &mut renderer, ITEM, mouse::Button::Left);
        assert!(messages.iter().any(|m| matches!(m, Message::Item("Edit"))));
        // Clicking a root with a menu open closes the menus, as before
        _ = open(&mut ui, &mut renderer, FILE);
        _ = click(&mut ui, &mut renderer, EDIT, mouse::Button::Left);
        let (messages, _) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(under(&messages));
    }

    #[cfg(wayland_platform)]
    #[test]
    fn a_click_outside_a_native_menu_only_closes_it() {
        use crate::app::cosmic::{WINDOWING_SYSTEM, WindowingSystem};
        _ = WINDOWING_SYSTEM.set(WindowingSystem::Wayland);
        let popup = |messages: &[Message]| {
            messages
                .iter()
                .any(|m| matches!(m, Message::Surface(crate::surface::Action::Popup(..))))
        };
        let destroyed = |messages: &[Message]| {
            messages
                .iter()
                .any(|m| matches!(m, Message::Surface(crate::surface::Action::DestroyPopup(_))))
        };
        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            button_then_bar(Some(window::Id::unique())),
            iced::Size::new(400.0, 300.0),
            Cache::default(),
            &mut renderer,
        );
        let messages = open(&mut ui, &mut renderer, FILE);
        assert!(popup(&messages));
        let (messages, captured) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(
            captured && !under(&messages),
            "the closing click is swallowed"
        );
        assert!(destroyed(&messages));
        let (messages, _) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(under(&messages));

        // The bar still handles its own clicks: moving to Edit with File open opens
        // Edit, and clicking it closes the menu, as before
        _ = open(&mut ui, &mut renderer, FILE);
        let (messages, _) = click(&mut ui, &mut renderer, EDIT, mouse::Button::Left);
        assert!(popup(&messages) && destroyed(&messages));
        let (messages, _) = click(&mut ui, &mut renderer, FILE, mouse::Button::Left);
        assert!(popup(&messages), "the menu was closed");
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Pick {
        Open,
        First,
        Deep,
    }

    impl super::super::Action for Pick {
        type Message = Message;

        fn message(&self) -> Message {
            Message::Item(match self {
                Self::Open => "Open",
                Self::First => "First",
                Self::Deep => "Deep",
            })
        }
    }

    /// One menu in the window's corner, as Lollypop's three-bar button is, then a
    /// button. The menu's entries are the real ones, 36 px high and 150 wide:
    ///
    /// ```text
    /// File: Recent >  Open
    /// Recent: First  More >
    /// More: Deep
    /// ```
    fn bar_with_folders(window: Option<window::Id>) -> crate::Element<'static, Message> {
        use super::super::{Item, items};
        let file = items(
            &HashMap::new(),
            vec![
                Item::Folder(
                    "Recent",
                    vec![
                        Item::Button("First", None, Pick::First),
                        Item::Folder("More", vec![Item::Button("Deep", None, Pick::Deep)]),
                    ],
                ),
                Item::Button("Open", None, Pick::Open),
            ],
        );
        let root = super::super::root::<Message, Renderer>("File")
            .width(80)
            .height(40);
        let mut bar = menu_bar(vec![MenuTree::with_children(
            crate::widget::RcElementWrapper::new(crate::Element::from(root)),
            file,
        )]);
        if let Some(window) = window {
            bar = bar.window_id(window).on_surface_action(Message::Surface);
        }
        crate::widget::row::with_children(vec![
            bar.into(),
            crate::widget::button::text("Under")
                .on_press(Message::Under)
                .width(60)
                .height(40)
                .into(),
        ])
        .into()
    }

    const FOLDER_WINDOW: iced::Size = iced::Size::new(600.0, 400.0);
    const CORNER_ROOT: Point = Point::new(20.0, 20.0);
    const BESIDE_THE_BAR: Point = Point::new(110.0, 20.0);

    fn picked(messages: &[Message], name: &str) -> bool {
        messages.iter().any(|m| matches!(m, Message::Item(n) if *n == name))
    }

    #[test]
    fn a_click_on_a_submenu_entry_keeps_an_in_window_menu_open() {
        const RECENT: Point = Point::new(75.0, 58.0);
        const OPEN: Point = Point::new(75.0, 94.0);
        const FIRST: Point = Point::new(225.0, 58.0);
        const MORE: Point = Point::new(225.0, 94.0);
        const DEEP: Point = Point::new(375.0, 94.0);

        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            bar_with_folders(None),
            FOLDER_WINDOW,
            Cache::default(),
            &mut renderer,
        );

        // A click on an entry of the top menu leaves its submenu usable, as often as
        // it is clicked, and an entry of the submenu still acts and closes everything
        _ = open(&mut ui, &mut renderer, CORNER_ROOT);
        _ = hover(&mut ui, &mut renderer, RECENT);
        let (messages, _) = click(&mut ui, &mut renderer, RECENT, mouse::Button::Left);
        assert!(messages.is_empty());
        _ = click(&mut ui, &mut renderer, RECENT, mouse::Button::Left);
        _ = hover(&mut ui, &mut renderer, FIRST);
        let (messages, _) = click(&mut ui, &mut renderer, FIRST, mouse::Button::Left);
        assert!(picked(&messages, "First"), "the click on Recent closed the menu");
        let (messages, _) = click(&mut ui, &mut renderer, BESIDE_THE_BAR, mouse::Button::Left);
        assert!(under(&messages), "the entry closed the menu");

        // The same one level down
        _ = open(&mut ui, &mut renderer, CORNER_ROOT);
        _ = hover(&mut ui, &mut renderer, RECENT);
        _ = hover(&mut ui, &mut renderer, MORE);
        let (messages, _) = click(&mut ui, &mut renderer, MORE, mouse::Button::Left);
        assert!(messages.is_empty());
        _ = hover(&mut ui, &mut renderer, DEEP);
        let (messages, _) = click(&mut ui, &mut renderer, DEEP, mouse::Button::Left);
        assert!(picked(&messages, "Deep"), "the click on More closed the menu");
        let (messages, _) = click(&mut ui, &mut renderer, BESIDE_THE_BAR, mouse::Button::Left);
        assert!(under(&messages), "the entry closed the menu");

        // With a submenu open and clicked, a click elsewhere only closes the menus
        _ = open(&mut ui, &mut renderer, CORNER_ROOT);
        _ = hover(&mut ui, &mut renderer, RECENT);
        _ = click(&mut ui, &mut renderer, RECENT, mouse::Button::Left);
        let (messages, captured) =
            click(&mut ui, &mut renderer, BESIDE_THE_BAR, mouse::Button::Left);
        assert!(captured && !under(&messages), "the closing click is swallowed");
        let (messages, _) = click(&mut ui, &mut renderer, BESIDE_THE_BAR, mouse::Button::Left);
        assert!(under(&messages), "the click elsewhere closed the menu");

        // An entry beside a submenu entry acts and closes the menu, as before
        _ = open(&mut ui, &mut renderer, CORNER_ROOT);
        _ = hover(&mut ui, &mut renderer, RECENT);
        _ = hover(&mut ui, &mut renderer, OPEN);
        let (messages, _) = click(&mut ui, &mut renderer, OPEN, mouse::Button::Left);
        assert!(picked(&messages, "Open"));
        let (messages, _) = click(&mut ui, &mut renderer, BESIDE_THE_BAR, mouse::Button::Left);
        assert!(under(&messages), "the entry closed the menu");
    }

    /// The popup a surface action asks for, as its own window would show it: Wayland
    /// sends a popup's events to that popup alone, in the popup's own coordinates.
    #[cfg(wayland_platform)]
    fn popup_ui(
        messages: &[Message],
        size: iced::Size,
        renderer: &mut Renderer,
    ) -> UserInterface<'static, crate::Action<Message>, crate::Theme, Renderer> {
        let view = messages
            .iter()
            .find_map(|m| match m {
                Message::Surface(crate::surface::Action::Popup(_, _, Some(view))) => {
                    Some(view.clone())
                }
                _ => None,
            })
            .expect("a native popup");
        UserInterface::build(view(), size, Cache::default(), renderer)
    }

    #[cfg(wayland_platform)]
    fn app(messages: Vec<crate::Action<Message>>) -> Vec<Message> {
        messages
            .into_iter()
            .filter_map(|m| match m {
                crate::Action::App(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    #[cfg(wayland_platform)]
    fn destroyed(messages: &[Message]) -> bool {
        messages
            .iter()
            .any(|m| matches!(m, Message::Surface(crate::surface::Action::DestroyPopup(_))))
    }

    #[cfg(wayland_platform)]
    #[test]
    fn a_click_on_a_submenu_entry_keeps_native_menus_open() {
        use crate::app::cosmic::{WINDOWING_SYSTEM, WindowingSystem};
        _ = WINDOWING_SYSTEM.set(WindowingSystem::Wayland);
        // In each popup's own coordinates, which lie inside the bar's in the window
        const FIRST_ROW: Point = Point::new(75.0, 18.0);
        const SECOND_ROW: Point = Point::new(75.0, 54.0);
        const TOP: iced::Size = iced::Size::new(152.0, 74.0);
        const SUB: iced::Size = iced::Size::new(152.0, 74.0);
        const DEEP: iced::Size = iced::Size::new(152.0, 38.0);
        let left = mouse::Button::Left;

        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            bar_with_folders(Some(window::Id::unique())),
            FOLDER_WINDOW,
            Cache::default(),
            &mut renderer,
        );

        // A click on an entry of the top menu leaves its submenu usable, as often as
        // it is clicked, and an entry of the submenu still acts and closes everything
        let opened = open(&mut ui, &mut renderer, CORNER_ROOT);
        let mut top = popup_ui(&opened, TOP, &mut renderer);
        let shown = app(hover(&mut top, &mut renderer, FIRST_ROW));
        let mut sub = popup_ui(&shown, SUB, &mut renderer);
        let (messages, _) = click(&mut top, &mut renderer, FIRST_ROW, left);
        assert!(!destroyed(&app(messages)), "the click on Recent closed the menu");
        _ = click(&mut top, &mut renderer, FIRST_ROW, left);
        _ = hover(&mut sub, &mut renderer, FIRST_ROW);
        let messages = app(click(&mut sub, &mut renderer, FIRST_ROW, left).0);
        assert!(picked(&messages, "First"), "the click on Recent closed the menu");
        assert!(destroyed(&messages), "the entry closed the menu");

        // The same one level down
        let opened = open(&mut ui, &mut renderer, CORNER_ROOT);
        let mut top = popup_ui(&opened, TOP, &mut renderer);
        let shown = app(hover(&mut top, &mut renderer, FIRST_ROW));
        let mut sub = popup_ui(&shown, SUB, &mut renderer);
        let shown = app(hover(&mut sub, &mut renderer, SECOND_ROW));
        let mut deep = popup_ui(&shown, DEEP, &mut renderer);
        let (messages, _) = click(&mut sub, &mut renderer, SECOND_ROW, left);
        assert!(!destroyed(&app(messages)), "the click on More closed the menu");
        _ = hover(&mut deep, &mut renderer, FIRST_ROW);
        let messages = app(click(&mut deep, &mut renderer, FIRST_ROW, left).0);
        assert!(picked(&messages, "Deep"), "the click on More closed the menu");
        assert!(destroyed(&messages), "the entry closed the menu");

        // With a submenu open and clicked, a click in the window only closes the menus
        let opened = open(&mut ui, &mut renderer, CORNER_ROOT);
        let mut top = popup_ui(&opened, TOP, &mut renderer);
        _ = hover(&mut top, &mut renderer, FIRST_ROW);
        _ = click(&mut top, &mut renderer, FIRST_ROW, left);
        let (messages, captured) = click(&mut ui, &mut renderer, BESIDE_THE_BAR, left);
        assert!(captured && !under(&messages), "the closing click is swallowed");
        assert!(destroyed(&messages));
        let (messages, _) = click(&mut ui, &mut renderer, BESIDE_THE_BAR, left);
        assert!(under(&messages), "the click in the window closed the menu");

        // An entry beside a submenu entry acts and closes the menu, as before
        let opened = open(&mut ui, &mut renderer, CORNER_ROOT);
        let mut top = popup_ui(&opened, TOP, &mut renderer);
        _ = hover(&mut top, &mut renderer, FIRST_ROW);
        _ = hover(&mut top, &mut renderer, SECOND_ROW);
        let messages = app(click(&mut top, &mut renderer, SECOND_ROW, left).0);
        assert!(picked(&messages, "Open") && destroyed(&messages));
    }
}

#[cfg(all(test, wayland_platform))]
mod popup_state_tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use std::borrow::Cow;

    fn root(label: &'static str) -> MenuTree<()> {
        MenuTree::with_children(
            crate::Element::from(crate::widget::text(label)),
            vec![MenuTree::new(crate::Element::from(crate::widget::text("Action")))],
        )
    }

    #[test]
    fn an_open_menu_survives_its_roots_being_replaced() {
        let renderer = iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::default(), iced::Pixels(14.0), Some("tiny-skia"),
        )).expect("a headless renderer");
        let mut bar = MenuBar::new(vec![root("File"), root("View")]);
        let mut tree = Tree::new(&bar as &dyn Widget<(), crate::Theme, Renderer>);
        bar.diff(&mut tree);
        let state = tree.state.downcast_ref::<MenuBarState>().clone();
        state.inner.with_data_mut(|inner| inner.open = true);
        let mut popup = Menu {
            tree: state.clone(), menu_roots: bar.menu_roots.clone(),
            bounds_expand: 16, menu_overlays_parent: false,
            close_condition: bar.close_condition, item_width: bar.item_width,
            item_height: bar.item_height, bar_bounds: Rectangle::new(Point::ORIGIN, iced::Size::new(200.0, 32.0)),
            main_offset: 0, cross_offset: 0,
            root_bounds_list: vec![
                Rectangle::new(Point::ORIGIN, iced::Size::new(100.0, 32.0)),
                Rectangle::new(Point::new(100.0, 0.0), iced::Size::new(100.0, 32.0)),
            ],
            path_highlight: None, style: Cow::Owned(bar.style.clone()),
            position: Point::ORIGIN, is_overlay: false, window_id: window::Id::unique(),
            depth: 0, on_surface_action: None,
        };
        let parent = window::Id::unique();
        let old_popup_id = popup.window_id;
        state.inner.with_data_mut(|inner| { inner.popup_id.insert(parent, old_popup_id); });
        let mut messages = Vec::new();
        init_root_menu(
            &mut popup, &renderer, &mut Shell::new(&mut messages), Point::new(150.0, 16.0),
            iced::Size::new(400.0, 300.0), Vector::ZERO,
            Rectangle::new(Point::ORIGIN, iced::Size::new(200.0, 32.0)), 0.0,
        );
        assert_eq!(state.inner.with_data(|inner| inner.active_root.clone()), vec![1]);
        let limits = Limits::new(iced::Size::ZERO, iced::Size::new(400.0, 300.0));
        let _ = popup.layout(&renderer, limits);

        // Responsive collapse and a changed menu model can both replace the
        // roots while an existing native popup still owns the old model.
        let mut replacement = MenuBar::new(vec![root("Menu")]);
        replacement.diff(&mut tree);
        let _ = popup.layout(&renderer, limits);
        assert!(!state.inner.with_data(|inner| inner.open));

        // A new menu can open before the old native surface's final frame
        // and dismissal arrive. Its state must not revive the stale model.
        let mut reopened = popup.clone();
        reopened.tree = tree.state.downcast_ref::<MenuBarState>().clone();
        reopened.menu_roots = replacement.menu_roots.clone();
        reopened.root_bounds_list.truncate(1);
        reopened.window_id = window::Id::unique();
        state.inner.with_data_mut(|inner| {
            inner.open = true;
            inner.popup_id.insert(parent, reopened.window_id);
        });
        init_root_menu(
            &mut reopened, &renderer, &mut Shell::new(&mut messages), Point::new(50.0, 16.0),
            iced::Size::new(400.0, 300.0), Vector::ZERO,
            Rectangle::new(Point::ORIGIN, iced::Size::new(100.0, 32.0)), 0.0,
        );
        assert!(reopened.layout(&renderer, limits).size().width > 0.0);
        assert_eq!(popup.layout(&renderer, limits).size(), iced::Size::ZERO);
        assert!(state.inner.with_data(|inner| inner.open));

        let closed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = closed.clone();
        let handler: Arc<dyn Fn(crate::surface::Action<()>) + Send + Sync> = Arc::new(move |action| {
            if let crate::surface::Action::DestroyPopup(id) = action {
                captured.lock().unwrap().push(id);
            }
        });
        state.close_outdated_popups(Some(&handler), &mut Shell::new(&mut messages));
        state.close_outdated_popups(Some(&handler), &mut Shell::new(&mut messages));
        assert_eq!(*closed.lock().unwrap(), vec![old_popup_id], "close only the outdated native popup, once");
        assert_eq!(state.inner.with_data(|inner| inner.popup_id.get(&parent).copied()), Some(reopened.window_id));
        assert!(state.inner.with_data(|inner| inner.open));
    }

    #[test]
    fn relabeling_a_menu_preserves_its_open_popup() {
        let mut bar = MenuBar::new(vec![root("File"), root("View")]);
        let mut tree = Tree::new(&bar as &dyn Widget<(), crate::Theme, Renderer>);
        bar.diff(&mut tree);
        let popup_state = tree.state.downcast_ref::<MenuBarState>().clone();
        popup_state.inner.with_data_mut(|inner| {
            inner.open = true;
            inner.active_root = vec![1];
        });
        let mut renamed = MenuBar::new(vec![root("Fichier"), root("Affichage")]);
        renamed.diff(&mut tree);
        assert!(popup_state.is_current(), "text changes do not invalidate measured menu slots");
        assert!(popup_state.inner.with_data(|inner| inner.open));
        assert_eq!(popup_state.inner.with_data(|inner| inner.active_root.clone()), vec![1]);
    }
}
