// Copyright 2024 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! A context menu is a menu in a graphical user interface that appears upon user interaction, such as a right-click mouse operation.

#[cfg(wayland_platform)]
use crate::app::cosmic::{WINDOWING_SYSTEM, WindowingSystem};
use crate::widget::menu::{
    self, CloseCondition, Direction, ItemHeight, ItemWidth, MenuBarState, PathHighlight,
    init_root_menu, menu_roots_diff,
};
use derive_setters::Setters;
use iced::touch::Finger;
use iced::{Event, Vector, keyboard, window};
use iced_core::widget::{Tree, Widget, tree};
use iced_core::{Length, Point, Size, mouse, touch};
use std::collections::HashSet;
use std::sync::Arc;

use crate::widget::RcWrapper;

/// A context menu is a menu in a graphical user interface that appears upon user interaction, such as a right-click mouse operation.
pub fn context_menu<'a, Message: 'static + Clone>(
    content: impl Into<crate::Element<'a, Message>>,
    // on_context: Message,
    context_menu: Option<Vec<menu::Tree<Message>>>,
) -> ContextMenu<'a, Message> {
    let mut this = ContextMenu {
        content: content.into(),
        context_menu: context_menu.map(|menus| {
            vec![menu::Tree::with_children(
                crate::Element::from(crate::widget::Row::new()),
                menus,
            )]
        }),
        close_on_escape: true,
        window_id: crate::widget::text_context_menu::current_window_id(),
        item_width: ItemWidth::Uniform(240),
        on_open: None,
        on_close: None,
        on_surface_action: None,
    };

    if let Some(ref mut context_menu) = this.context_menu {
        context_menu.iter_mut().for_each(menu::Tree::set_index);
    }

    this
}

/// A context menu is a menu in a graphical user interface that appears upon user interaction, such as a right-click mouse operation.
#[derive(Setters)]
#[must_use]
pub struct ContextMenu<'a, Message> {
    #[setters(skip)]
    content: crate::Element<'a, Message>,
    #[setters(skip)]
    context_menu: Option<Vec<menu::Tree<Message>>>,
    pub window_id: window::Id,
    pub close_on_escape: bool,
    /// Width of each menu item, and therefore of the menu.
    pub item_width: ItemWidth,
    /// Emitted when the menu opens, so the application can mark what was right-clicked.
    #[setters(strip_option)]
    pub on_open: Option<Message>,
    /// Emitted when the menu closes by any path, including the compositor dismissing it.
    #[setters(strip_option)]
    pub on_close: Option<Message>,
    #[setters(skip)]
    pub(crate) on_surface_action:
        Option<Arc<dyn Fn(crate::surface::Action<Message>) -> Message + Send + Sync + 'static>>,
}

impl<Message: Clone + 'static> ContextMenu<'_, Message> {
    /// Publish `on_open`/`on_close` when the open state changed since the last report.
    fn report_open_state(
        &self,
        state: &mut LocalState<Message>,
        shell: &mut iced_core::Shell<'_, Message>,
    ) {
        let open = state.menu_bar_state.inner.with_data(|d| d.open);
        if open == state.reported_open {
            return;
        }
        state.reported_open = open;
        let message = if open { &self.on_open } else { &self.on_close };
        if let Some(message) = message.clone() {
            shell.publish(message);
        }
    }

    #[cfg(wayland_platform)]
    #[allow(clippy::too_many_lines)]
    fn create_popup(
        &mut self,
        layout: iced_core::Layout<'_>,
        view_cursor: iced_core::mouse::Cursor,
        renderer: &crate::Renderer,
        shell: &mut iced_core::Shell<'_, Message>,
        viewport: &iced::Rectangle,
        my_state: &mut LocalState<Message>,
    ) {
        if self.window_id != window::Id::NONE && self.on_surface_action.is_some() {
            use crate::surface::action::{LiveSettings, destroy_popup};
            use crate::theme::THEME;
            use crate::widget::menu::{Menu, StyleSheet as _};
            use iced_runtime::platform_specific::wayland::CornerRadius;
            use iced_runtime::platform_specific::wayland::popup::{
                SctkPopupSettings, SctkPositioner,
            };

            // Scrollables deliver the cursor in content coordinates. Native
            // popups need coordinates on the parent surface instead.
            let click = my_state.context_cursor - layout.virtual_offset();
            let bounds = iced::Rectangle::new(click, Size::new(1.0, 1.0));

            let (id, root_list) = my_state.menu_bar_state.inner.with_data_mut(|state| {
                if let Some(id) = state.popup_id.get(&self.window_id).copied() {
                    // close existing popups
                    state.menu_states.clear();
                    state.active_root.clear();

                    shell.publish(self.on_surface_action.as_ref().unwrap()(destroy_popup(id)));
                    state.view_cursor = view_cursor;
                }
                // A fresh id per popup, so the old popup's Done cannot be mistaken for the new one's
                (
                    window::Id::unique(),
                    layout.children().map(|lo| lo.bounds()).collect::<Vec<_>>(),
                )
            });
            let Some(context_menu) = self.context_menu.as_mut() else {
                return;
            };

            let mut popup_menu: Menu<'static, _> = Menu {
                tree: my_state.menu_bar_state.clone(),
                menu_roots: context_menu.clone(),
                bounds_expand: 16,
                menu_overlays_parent: true,
                close_condition: CloseCondition {
                    leave: false,
                    click_outside: true,
                    click_inside: true,
                },
                item_width: self.item_width,
                item_height: ItemHeight::Dynamic(40),
                bar_bounds: bounds,
                main_offset: 0,
                cross_offset: 0,
                root_bounds_list: vec![bounds],
                path_highlight: Some(PathHighlight::MenuActive),
                style: std::borrow::Cow::Owned(crate::theme::menu_bar::MenuBarStyle::Default),
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
                click,
                viewport.size(),
                Vector::new(0., 0.),
                bounds,
                0.0,
            );
            let (anchor_rect, gravity) = my_state.menu_bar_state.inner.with_data_mut(|state| {
                use iced::Rectangle;

                state.popup_id.insert(self.window_id, id);
                ({
                    let pos = click;
                    Rectangle {
                        x: pos.x as i32,
                        y: pos.y as i32,
                        width: 1,
                        height: 1,
                    }
                },
                match (state.horizontal_direction, state.vertical_direction) {
                    (Direction::Positive, Direction::Positive) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::BottomRight,
                    (Direction::Positive, Direction::Negative) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::TopRight,
                    (Direction::Negative, Direction::Positive) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::BottomLeft,
                    (Direction::Negative, Direction::Negative) => cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Gravity::TopLeft,
                })
            });

            let menu_node =
                popup_menu.layout(renderer, iced::Limits::NONE.min_width(1.).min_height(1.));
            let popup_size = menu_node.size();
            let positioner = SctkPositioner {
                size: Some((
                    popup_size.width.ceil() as u32 + 2,
                    popup_size.height.ceil() as u32 + 2,
                )),
                anchor_rect,
                anchor: cctk::wayland_protocols::xdg::shell::client::xdg_positioner::Anchor::None,
                gravity,
                reactive: true,
                ..Default::default()
            };
            let parent = self.window_id;
            let roots = my_state.roots.clone();
            let t = THEME.lock().unwrap();
            let styling = t.appearance(&crate::theme::menu_bar::MenuBarStyle::Default, false);
            drop(t);
            let rad = styling.menu_border_radius;

            shell.publish((self.on_surface_action.as_ref().unwrap())(
                crate::surface::action::simple_popup(
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
                        // Latest roots from the owner widget
                        let mut popup_menu = popup_menu.clone();
                        popup_menu.menu_roots = roots.with_data(Clone::clone);
                        crate::Element::from(
                            crate::widget::container(popup_menu).center(Length::Fill),
                        )
                        .map(crate::action::app)
                    }),
                ),
            ));
        }
    }

    pub fn on_surface_action(
        mut self,
        handler: impl Fn(crate::surface::Action<Message>) -> Message + Send + Sync + 'static,
    ) -> Self {
        self.on_surface_action = Some(Arc::new(handler));
        self
    }
}

impl<Message: 'static + Clone> Widget<Message, crate::Theme, crate::Renderer>
    for ContextMenu<'_, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<LocalState<Message>>()
    }

    fn state(&self) -> tree::State {
        #[allow(clippy::default_trait_access)]
        tree::State::new(LocalState {
            context_cursor: Point::default(),
            fingers_pressed: Default::default(),
            menu_bar_state: Default::default(),
            reported_open: false,
            roots: RcWrapper::new(self.context_menu.clone().unwrap_or_default()),
            reshape: false,
        })
    }

    fn children(&self) -> Vec<Tree> {
        let mut children = Vec::with_capacity(if self.context_menu.is_some() { 2 } else { 1 });

        children.push(Tree::new(self.content.as_widget()));

        // Assign the context menu's elements as this widget's children.
        if let Some(ref context_menu) = self.context_menu {
            let mut tree = Tree::empty();
            tree.children = context_menu
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
                .collect();

            children.push(tree);
        }

        children
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
        let state = tree.state.downcast_mut::<LocalState<Message>>();
        if let Some(context_menu) = self.context_menu.as_mut() {
            // The popup's item slots were measured at open from the item widgets. Items of
            // another kind or count, such as a divider in a new place, do not fit those slots,
            // so rebuild the popup on the next `update`. Same-shaped items, such as a
            // relabeled button, update in place.
            let popup_open = state
                .menu_bar_state
                .inner
                .with_data(|d| !d.popup_id.is_empty());
            let shape = |roots: &Vec<menu::Tree<Message>>| -> Vec<tree::Tag> {
                roots.first().map_or_else(Vec::new, |root| {
                    root.flattern().iter().map(|mt| mt.item.tag()).collect()
                })
            };
            if popup_open && state.roots.with_data(shape) != shape(context_menu) {
                state.reshape = true;
                return;
            }
            state.menu_bar_state.inner.with_data_mut(|inner| {
                menu_roots_diff(context_menu, &mut inner.tree);
            });
            state
                .roots
                .with_data_mut(|roots| roots.clone_from(context_menu));
        }

        // if let Some(ref mut context_menus) = self.context_menu {
        //     for (menu, tree) in context_menus
        //         .iter_mut()
        //         .zip(tree.children[1].children.iter_mut())
        //     {
        //         menu.item.as_widget_mut().diff(tree);
        //     }
        // }
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &crate::Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut crate::Renderer,
        theme: &crate::Theme,
        style: &iced_core::renderer::Style,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &crate::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn drag_destinations(
        &self,
        tree: &Tree,
        layout: iced_core::Layout<'_>,
        renderer: &crate::Renderer,
        dnd_rectangles: &mut iced_core::clipboard::DndDestinationRectangles,
    ) {
        self.content.as_widget().drag_destinations(
            &tree.children[0],
            layout,
            renderer,
            dnd_rectangles,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: iced_core::Layout<'_>,
        renderer: &crate::Renderer,
        operation: &mut dyn iced_core::widget::Operation<()>,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    #[allow(clippy::too_many_lines)]
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        renderer: &crate::Renderer,
        clipboard: &mut dyn iced_core::Clipboard,
        shell: &mut iced_core::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        let state = tree.state.downcast_mut::<LocalState<Message>>();
        let bounds = layout.bounds();

        // The compositor dismissed our popup: nothing else tells this state about it.
        #[cfg(wayland_platform)]
        if let iced::Event::PlatformSpecific(iced::event::PlatformSpecific::Wayland(
            iced::event::wayland::Event::Popup(iced::event::wayland::PopupEvent::Done, _, popup),
        )) = event
        {
            state.menu_bar_state.inner.with_data_mut(|d| {
                if d.popup_id.get(&self.window_id) == Some(popup) {
                    d.popup_id.remove(&self.window_id);
                    d.reset();
                }
            });
        }

        // The menu changed shape while open
        #[cfg(wayland_platform)]
        if state.reshape {
            state.reshape = false;
            if state.menu_bar_state.inner.with_data(|d| d.open) {
                if let Some(context_menu) = self.context_menu.as_mut() {
                    state.menu_bar_state.inner.with_data_mut(|inner| {
                        menu_roots_diff(context_menu, &mut inner.tree);
                    });
                    state
                        .roots
                        .with_data_mut(|roots| roots.clone_from(context_menu));
                }
                let view_cursor = state.menu_bar_state.inner.with_data(|d| d.view_cursor);
                self.create_popup(layout, view_cursor, renderer, shell, viewport, state);
            }
        }

        // XXX this should reset the state if there are no other copies of the state, which implies no dropdown menus open.
        let reset = self.window_id != window::Id::NONE
            && state
                .menu_bar_state
                .inner
                .with_data(|d| !d.open && !d.active_root.is_empty());

        let open = state.menu_bar_state.inner.with_data_mut(|state| {
            if reset
                && let Some(popup_id) = state.popup_id.get(&self.window_id).copied()
                && let Some(handler) = self.on_surface_action.as_ref()
            {
                shell.publish((handler)(crate::surface::Action::DestroyPopup(popup_id)));
                state.reset();
            }
            state.open
        });
        let mut was_open = false;
        if matches!(event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
            | Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Right | mouse::Button::Left,
            ))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if open )
        {
            state.menu_bar_state.inner.with_data_mut(|state| {
                was_open = true;
                state.menu_states.clear();
                state.active_root.clear();
                state.open = false;

                #[cfg(wayland_platform)]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland))
                    && let Some(id) = state.popup_id.remove(&self.window_id)
                {
                    {
                        let surface_action = self.on_surface_action.as_ref().unwrap();
                        shell.publish(surface_action(crate::surface::action::destroy_popup(id)));
                    }
                    state.view_cursor = cursor;
                }
            });
            // Esc closed the menu and ends here: the app's Esc shortcut does not run too
            if matches!(event, Event::Keyboard(_)) {
                shell.capture_event();
            }
        }

        if !was_open && cursor.is_over(bounds) {
            let fingers_pressed = state.fingers_pressed.len();

            match event {
                Event::Touch(touch::Event::FingerPressed { id, .. }) => {
                    state.fingers_pressed.insert(*id);
                }

                Event::Touch(touch::Event::FingerLifted { id, .. }) => {
                    state.fingers_pressed.remove(id);
                }

                _ => (),
            }

            // Present a context menu on a right click event.
            if !was_open
                && self.context_menu.is_some()
                && (right_button_released(event) || (touch_lifted(event) && fingers_pressed == 2))
            {
                state.context_cursor = cursor.position().unwrap_or_default();
                let state = tree.state.downcast_mut::<LocalState<Message>>();
                state.menu_bar_state.inner.with_data_mut(|state| {
                    state.open = true;
                    state.view_cursor = cursor;
                });
                #[cfg(wayland_platform)]
                if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland)) {
                    self.create_popup(layout, cursor, renderer, shell, viewport, state);
                }

                shell.request_redraw();
                shell.capture_event();
                self.report_open_state(tree.state.downcast_mut::<LocalState<Message>>(), shell);
                return;
            } else if !was_open && right_button_released(event)
                || (touch_lifted(event))
                || left_button_released(event)
            {
                state.menu_bar_state.inner.with_data_mut(|state| {
                    was_open = true;
                    state.menu_states.clear();
                    state.active_root.clear();
                    state.open = false;

                    #[cfg(wayland_platform)]
                    if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland))
                        && let Some(id) = state.popup_id.remove(&self.window_id)
                    {
                        {
                            let surface_action = self.on_surface_action.as_ref().unwrap();
                            shell
                                .publish(surface_action(crate::surface::action::destroy_popup(id)));
                        }
                        state.view_cursor = cursor;
                    }
                });
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        self.report_open_state(tree.state.downcast_mut::<LocalState<Message>>(), shell);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: iced_core::Layout<'b>,
        renderer: &crate::Renderer,
        viewport: &iced::Rectangle,
        translation: Vector,
    ) -> Option<iced_core::overlay::Element<'b, Message, crate::Theme, crate::Renderer>> {
        // The wrapped content's overlays (tooltips, dropdowns, ...) always pass through
        let content = self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        );

        #[cfg(wayland_platform)]
        if matches!(WINDOWING_SYSTEM.get(), Some(WindowingSystem::Wayland))
            && self.window_id != window::Id::NONE
            && self.on_surface_action.is_some()
        {
            let shield = menu::popup_shield(
                &tree
                    .state
                    .downcast_ref::<LocalState<Message>>()
                    .menu_bar_state,
                self.window_id,
                iced::Rectangle::default(),
                self.on_surface_action.as_ref(),
            );
            return match (content, shield) {
                (Some(content), Some(shield)) => {
                    Some(iced_core::overlay::Group::with_children(vec![content, shield]).overlay())
                }
                (content, shield) => content.or(shield),
            };
        }

        let state = tree.state.downcast_ref::<LocalState<Message>>();
        let Some(context_menu) = self.context_menu.as_mut() else {
            return content;
        };
        if !state.menu_bar_state.inner.with_data(|state| state.open) {
            return content;
        }

        // Anchor the menu to a 1x1 rectangle at the click, like the popup path does
        let bounds = iced::Rectangle::new(state.context_cursor, Size::new(1.0, 1.0));
        let menu = crate::widget::menu::Menu {
            tree: state.menu_bar_state.clone(),
            menu_roots: context_menu.clone(),
            bounds_expand: 16,
            menu_overlays_parent: true,
            close_condition: CloseCondition {
                leave: false,
                click_outside: true,
                click_inside: true,
            },
            item_width: self.item_width,
            item_height: ItemHeight::Dynamic(40),
            bar_bounds: bounds,
            main_offset: 0,
            cross_offset: 0,
            root_bounds_list: vec![bounds],
            path_highlight: Some(PathHighlight::MenuActive),
            style: std::borrow::Cow::Borrowed(&crate::theme::menu_bar::MenuBarStyle::Default),
            position: Point::new(translation.x, translation.y),
            is_overlay: true,
            window_id: window::Id::NONE,
            depth: 0,
            on_surface_action: None,
        }
        .overlay();

        Some(match content {
            Some(content) => {
                iced_core::overlay::Group::with_children(vec![content, menu]).overlay()
            }
            None => menu,
        })
    }

    #[cfg(feature = "a11y")]
    /// get the a11y nodes for the widget
    fn a11y_nodes(
        &self,
        layout: iced_core::Layout<'_>,
        state: &Tree,
        p: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        let c_state = &state.children[0];
        self.content.as_widget().a11y_nodes(layout, c_state, p)
    }
}

impl<'a, Message: Clone + 'static> From<ContextMenu<'a, Message>> for crate::Element<'a, Message> {
    fn from(widget: ContextMenu<'a, Message>) -> Self {
        Self::new(widget)
    }
}

fn right_button_released(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right,))
    )
}

fn left_button_released(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left,))
    )
}

fn touch_lifted(event: &Event) -> bool {
    matches!(event, Event::Touch(touch::Event::FingerLifted { .. }))
}

pub struct LocalState<Message> {
    context_cursor: Point,
    fingers_pressed: HashSet<Finger>,
    menu_bar_state: MenuBarState,
    reported_open: bool,
    roots: RcWrapper<Vec<menu::Tree<Message>>>,
    /// menu shape has changed, rebuild it on the next `update`
    reshape: bool,
}

/// An open context menu whose items change under it (the application
/// builds them from state that changed while it was open) stays whole:
/// no index past the end of a shorter menu, and every item in its own
/// slot, as if the menu had been opened so.
#[cfg(test)]
mod open_menu_change_tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use iced_core::renderer::Style;
    use iced_core::{Color, clipboard};
    use iced_runtime::user_interface::{Cache, UserInterface};

    #[derive(Clone, Debug)]
    struct Pick;

    const AT: Point = Point::new(60.0, 40.0);
    const SIZE: Size = Size::new(420.0, 900.0);

    fn renderer() -> crate::Renderer {
        iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::default(),
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer")
    }

    /// `n` items, item `divider` (if any) a divider
    fn page(n: usize, divider: Option<usize>) -> crate::Element<'static, Pick> {
        let items = (0..n)
            .map(|i| {
                if Some(i) == divider {
                    menu::Tree::new(crate::Element::from(crate::widget::divider::horizontal::default()))
                } else {
                    menu::Tree::new(crate::Element::from(
                        crate::widget::button::text(format!("Item {i}")).on_press(Pick),
                    ))
                }
            })
            .collect();
        let content = crate::widget::container(crate::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fill);
        // The overlay path: no native popup
        crate::Element::from(context_menu(content, Some(items)).window_id(window::Id::NONE))
    }

    /// Frames of `pages`, each with its events, the pointer at the click;
    /// the last frame's pixels
    fn frames(pages: Vec<(crate::Element<'static, Pick>, Vec<Event>)>) -> Vec<u8> {
        let mut renderer = renderer();
        let mut cache = Cache::default();
        let cursor = mouse::Cursor::Available(AT);
        let theme = crate::Theme::light();
        let mut pixels = Vec::new();
        for (element, mut events) in pages {
            let mut ui = UserInterface::build(element, SIZE, std::mem::take(&mut cache), &mut renderer);
            events.push(Event::Window(window::Event::RedrawRequested(std::time::Instant::now())));
            for event in events {
                let _ = ui.update(&[event], cursor, &mut renderer, &mut clipboard::Null, &mut Vec::new());
            }
            ui.draw(&mut renderer, &theme, &Style { icon_color: Color::BLACK, text_color: Color::BLACK, scale_factor: 1.0 }, cursor);
            pixels = renderer.screenshot(Size::new(SIZE.width as u32, SIZE.height as u32), 1.0, Color::WHITE);
            cache = ui.into_cache();
        }
        pixels
    }

    fn open() -> Vec<Event> {
        vec![
            Event::Mouse(mouse::Event::CursorMoved { position: AT }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        ]
    }

    /// Opened as `(n, divider)`, then shown as `then` for two frames
    fn changed(first: (usize, Option<usize>), then: (usize, Option<usize>)) -> Vec<u8> {
        frames(vec![
            (page(first.0, first.1), open()),
            (page(first.0, first.1), vec![]),
            (page(then.0, then.1), vec![]),
            (page(then.0, then.1), vec![]),
        ])
    }

    /// Opened as `then` itself, in as many frames
    fn fresh(then: (usize, Option<usize>)) -> Vec<u8> {
        frames(vec![
            (page(then.0, then.1), open()),
            (page(then.0, then.1), vec![]),
            (page(then.0, then.1), vec![]),
            (page(then.0, then.1), vec![]),
        ])
    }

    fn differing(a: &[u8], b: &[u8]) -> usize {
        a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count()
    }

    #[test]
    fn the_menu_opens() {
        let closed = frames(vec![(page(5, None), vec![])]);
        assert!(differing(&fresh((5, None)), &closed) > 1000);
    }

    #[test]
    fn a_menu_that_loses_an_item_while_open_does_not_panic_and_is_drawn_whole() {
        assert_eq!(differing(&changed((18, None), (17, None)), &fresh((17, None))), 0);
        // Down to one item
        assert_eq!(differing(&changed((18, None), (1, None)), &fresh((1, None))), 0);
    }

    #[test]
    fn a_menu_that_gains_an_item_while_open_shows_it() {
        assert_eq!(differing(&changed((17, None), (18, None)), &fresh((18, None))), 0);
    }

    #[test]
    fn an_item_that_changes_kind_while_open_takes_its_own_slot() {
        assert_eq!(differing(&changed((10, None), (10, Some(3))), &fresh((10, Some(3)))), 0);
    }
}

#[cfg(all(test, wayland_platform))]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use iced_core::widget::operation::scrollable;
    use iced_runtime::platform_specific::wayland::popup::SctkPopupSettings;
    use iced_runtime::user_interface::{Cache, UserInterface};

    #[derive(Clone)]
    enum Message {
        Click,
        Closed,
        Surface(crate::surface::Action<Self>),
    }

    #[test]
    fn a_context_menu_inherits_the_window_that_builds_it() {
        let previous = crate::widget::text_context_menu::current_window_id();
        for window in [window::Id::unique(), window::Id::unique()] {
            crate::widget::text_context_menu::set_current_window_id(window);
            let menu = context_menu(
                crate::widget::text("Link"),
                None::<Vec<menu::Tree<Message>>>,
            );
            assert_eq!(menu.window_id, window);
        }
        crate::widget::text_context_menu::set_current_window_id(previous);
    }

    #[test]
    fn a_scrolled_session_menu_is_anchored_at_the_window_cursor() {
        _ = WINDOWING_SYSTEM.set(WindowingSystem::Wayland);
        let mut renderer = iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::default(),
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer");
        let id = iced_core::widget::Id::unique();
        let parent = window::Id::unique();
        let rows = (0..40).map(|_| {
            let row = crate::widget::container(crate::widget::text("Session"))
                .width(Length::Fill)
                .height(32);
            let menu = vec![menu::Tree::new(crate::Element::from(
                crate::widget::button::text("Open").on_press(Message::Click),
            ))];
            crate::Element::from(
                context_menu(row, Some(menu))
                    .window_id(parent)
                    .on_surface_action(Message::Surface),
            )
        });
        let element = crate::widget::scrollable(crate::widget::column(rows))
            .id(id.clone())
            .width(300)
            .height(160);
        let mut ui = UserInterface::build(
            element,
            Size::new(300.0, 160.0),
            Cache::default(),
            &mut renderer,
        );
        // The column and its session container must pass on the scroll offset.
        // Exercise both zero offset and a far scrolled sidebar.
        for offset in [0.0, 1.0] {
            ui.operate(
                &renderer,
                &mut scrollable::snap_to(
                    id.clone(),
                    scrollable::RelativeOffset {
                        x: None,
                        y: Some(offset),
                    },
                ),
            );
            let click = Point::new(100.0, 100.0);
            let mut messages = Vec::new();
            _ = ui.update(
                &[Event::Mouse(mouse::Event::ButtonReleased(
                    mouse::Button::Right,
                ))],
                mouse::Cursor::Available(click),
                &mut renderer,
                &mut iced_core::clipboard::Null,
                &mut messages,
            );
            let settings = messages
                .iter()
                .find_map(|message| match message {
                    Message::Surface(crate::surface::Action::Popup(settings, _, _)) => {
                        Some(settings)
                    }
                    _ => None,
                })
                .expect("a native popup");
            let settings = settings
                .downcast_ref::<Box<dyn Fn() -> SctkPopupSettings + Send + Sync>>()
                .expect("popup settings")();
            assert_eq!(settings.parent, parent);
            assert_eq!(settings.positioner.anchor_rect.x, click.x as i32);
            assert_eq!(settings.positioner.anchor_rect.y, click.y as i32);
            assert_eq!(settings.positioner.anchor_rect.width, 1);
            assert_eq!(settings.positioner.anchor_rect.height, 1);
            // Close before clicking another row, as a user would.
            _ = ui.update(
                &[Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    physical_key: keyboard::key::Physical::Unidentified(
                        keyboard::key::NativeCode::Unidentified,
                    ),
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::default(),
                    text: None,
                    repeat: false,
                })],
                mouse::Cursor::Available(click),
                &mut renderer,
                &mut iced_core::clipboard::Null,
                &mut Vec::new(),
            );
        }
    }

    /// A button, then two rows with context menus; the first reports when it closes
    fn button_then_rows(window: Option<window::Id>) -> crate::Element<'static, Message> {
        let row = |closed: bool| {
            let content = crate::widget::container(crate::widget::text("Row"))
                .width(Length::Fill)
                .height(40);
            let menu = vec![menu::Tree::new(crate::Element::from(
                crate::widget::button::text("Open"),
            ))];
            let mut row = context_menu(content, Some(menu));
            if closed {
                row = row.on_close(Message::Closed);
            }
            if let Some(window) = window {
                row = row.window_id(window).on_surface_action(Message::Surface);
            }
            crate::Element::from(row)
        };
        crate::widget::column::with_children(vec![
            crate::widget::button::text("Under")
                .on_press(Message::Click)
                .width(300)
                .height(40)
                .into(),
            row(true),
            row(false),
        ])
        .into()
    }

    const UNDER: Point = Point::new(100.0, 20.0);
    const ROW_A: Point = Point::new(100.0, 60.0);
    const ROW_B: Point = Point::new(100.0, 100.0);

    /// Moves to `at`, then presses and releases `button` there, one frame each, then
    /// moves again as a pointer does. Returns every message, and whether the press
    /// was captured.
    fn click(
        ui: &mut UserInterface<'_, Message, crate::Theme, crate::Renderer>,
        renderer: &mut crate::Renderer,
        at: Point,
        button: mouse::Button,
    ) -> (Vec<Message>, bool) {
        let mut messages = Vec::new();
        let mut captured = false;
        for event in [
            mouse::Event::CursorMoved { position: at },
            mouse::Event::ButtonPressed(button),
            mouse::Event::ButtonReleased(button),
            mouse::Event::CursorMoved { position: at },
        ] {
            let (_, statuses) = ui.update(
                &[Event::Mouse(event)],
                mouse::Cursor::Available(at),
                renderer,
                &mut iced_core::clipboard::Null,
                &mut messages,
            );
            if matches!(event, mouse::Event::ButtonPressed(_)) {
                captured = statuses[0] == iced::event::Status::Captured;
            }
        }
        (messages, captured)
    }

    fn renderer() -> crate::Renderer {
        iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::default(),
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer")
    }

    fn has(messages: &[Message], f: impl Fn(&Message) -> bool) -> bool {
        messages.iter().any(f)
    }

    /// Whether a press of Esc with the pointer at `at` was captured, and what it published.
    fn press_escape(
        ui: &mut UserInterface<'_, Message, crate::Theme, crate::Renderer>,
        renderer: &mut crate::Renderer,
        at: Point,
    ) -> (Vec<Message>, bool) {
        let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
        let mut messages = Vec::new();
        let (_, statuses) = ui.update(
            &[Event::Keyboard(keyboard::Event::KeyPressed {
                key: escape.clone(),
                modified_key: escape,
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            })],
            mouse::Cursor::Available(at),
            renderer,
            &mut iced_core::clipboard::Null,
            &mut messages,
        );
        (messages, statuses[0] == iced::event::Status::Captured)
    }

    /// Esc that closes a menu is the menu's: the app's Esc shortcut must not run as
    /// well. With no menu open, Esc stays the app's. Both as an overlay in the window
    /// and as a native popup.
    #[test]
    fn esc_that_closes_a_context_menu_is_captured() {
        _ = WINDOWING_SYSTEM.set(WindowingSystem::Wayland);
        for window in [None, Some(window::Id::unique())] {
            let mut renderer = renderer();
            let mut ui = UserInterface::build(
                button_then_rows(window),
                Size::new(300.0, 200.0),
                Cache::default(),
                &mut renderer,
            );
            let (_, captured) = press_escape(&mut ui, &mut renderer, ROW_A);
            assert!(!captured, "no menu open, window {window:?}: Esc is the app's");
            let _ = click(&mut ui, &mut renderer, ROW_A, mouse::Button::Right);
            let (messages, captured) = press_escape(&mut ui, &mut renderer, ROW_A);
            assert!(has(&messages, |m| matches!(m, Message::Closed)));
            assert!(captured, "window {window:?}: the Esc that closed the menu leaked");
            let (_, captured) = press_escape(&mut ui, &mut renderer, ROW_A);
            assert!(!captured, "menu closed, window {window:?}: Esc is the app's again");
        }
    }

    #[test]
    fn a_click_outside_a_native_context_menu_only_closes_it() {
        _ = WINDOWING_SYSTEM.set(WindowingSystem::Wayland);
        let popup = |m: &Message| matches!(m, Message::Surface(crate::surface::Action::Popup(..)));
        let destroyed =
            |m: &Message| matches!(m, Message::Surface(crate::surface::Action::DestroyPopup(_)));
        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            button_then_rows(Some(window::Id::unique())),
            Size::new(300.0, 200.0),
            Cache::default(),
            &mut renderer,
        );
        let (messages, _) = click(&mut ui, &mut renderer, ROW_A, mouse::Button::Right);
        assert!(has(&messages, popup));
        let (messages, captured) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(captured, "the closing press is swallowed");
        assert!(!has(&messages, |m| matches!(m, Message::Click)));
        assert!(has(&messages, destroyed));
        assert!(has(&messages, |m| matches!(m, Message::Closed)));
        let (messages, _) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(has(&messages, |m| matches!(m, Message::Click)));

        // A right click on another row closes this menu and opens that row's
        _ = click(&mut ui, &mut renderer, ROW_A, mouse::Button::Right);
        let (messages, _) = click(&mut ui, &mut renderer, ROW_B, mouse::Button::Right);
        assert!(has(&messages, destroyed) && has(&messages, popup));
    }

    #[test]
    fn a_click_outside_an_in_window_context_menu_only_closes_it() {
        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            button_then_rows(None),
            Size::new(300.0, 200.0),
            Cache::default(),
            &mut renderer,
        );
        _ = click(&mut ui, &mut renderer, ROW_A, mouse::Button::Right);
        let (messages, captured) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(captured, "the closing press is swallowed");
        assert!(!has(&messages, |m| matches!(m, Message::Click)));
        assert!(has(&messages, |m| matches!(m, Message::Closed)));
        let (messages, _) = click(&mut ui, &mut renderer, UNDER, mouse::Button::Left);
        assert!(has(&messages, |m| matches!(m, Message::Click)));
    }

    /// Twenty 40 px rows, 240 px wide: an 800 px menu
    fn tall_menu() -> Vec<menu::Tree<Message>> {
        (0..20)
            .map(|_| {
                menu::Tree::new(crate::Element::from(
                    crate::widget::container(crate::widget::text("Row")).height(40),
                ))
            })
            .collect()
    }

    /// Where an in-window menu lies, in the window, after a right click at `at`
    fn in_window_menu_at(window: Size, at: Point) -> iced::Rectangle {
        let renderer = renderer();
        let page = crate::widget::container(crate::widget::text("Page"))
            .width(Length::Fill)
            .height(Length::Fill);
        let mut element = crate::Element::from(context_menu(page, Some(tall_menu())));
        let mut tree = Tree::new(&element);
        element.as_widget_mut().diff(&mut tree);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &iced_core::layout::Limits::new(Size::ZERO, window),
        );
        let layout = iced_core::Layout::new(&node);
        let viewport = iced::Rectangle::with_size(window);
        let cursor = mouse::Cursor::Available(at);
        let mut messages = Vec::new();
        let mut clipboard = iced_core::clipboard::Null;
        let mut shell = iced_core::Shell::new(&mut messages);
        element.as_widget_mut().update(
            &mut tree,
            &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
            layout,
            cursor,
            &renderer,
            &mut clipboard,
            &mut shell,
            &viewport,
        );
        let mut overlay = element
            .as_widget_mut()
            .overlay(&mut tree, layout, &renderer, &viewport, Vector::ZERO)
            .expect("the menu is open");
        // The menu places itself on the first event it sees
        let unplaced = overlay.as_overlay_mut().layout(&renderer, window);
        overlay.as_overlay_mut().update(
            &Event::Mouse(mouse::Event::CursorMoved { position: at }),
            iced_core::Layout::new(&unplaced),
            cursor,
            &renderer,
            &mut clipboard,
            &mut shell,
        );
        overlay
            .as_overlay_mut()
            .layout(&renderer, window)
            .children()[0]
            .bounds()
    }

    #[test]
    fn an_in_window_menu_that_fits_opens_at_the_pointer() {
        let window = Size::new(1000.0, 1000.0);
        // Down and to the right of the pointer, as the 1 x 1 anchor puts it
        let menu = in_window_menu_at(window, Point::new(100.0, 100.0));
        assert_eq!(menu.position(), Point::new(100.0, 101.0));
        // Flipped up beside the pointer: there is room above, none below
        let menu = in_window_menu_at(window, Point::new(100.0, 900.0));
        assert_eq!(menu.position(), Point::new(100.0, 100.0));
    }

    #[test]
    fn an_in_window_menu_shifts_just_enough_to_stay_inside() {
        let window = Size::new(1000.0, 1000.0);
        // Flipped up, it would start at y = -200: it comes down to the top
        let menu = in_window_menu_at(window, Point::new(100.0, 600.0));
        assert_eq!((menu.y, menu.height), (0.0, 800.0));
        // Opened down, it would end at 1251: it goes up to end at the bottom
        let menu = in_window_menu_at(window, Point::new(100.0, 450.0));
        assert_eq!((menu.y, menu.height), (200.0, 800.0));
        // Across as well: opened right at x = 100, 240 px wide, it would end
        // at 340 in a 300 px window
        let menu = in_window_menu_at(Size::new(300.0, 1000.0), Point::new(100.0, 100.0));
        assert_eq!(menu.x, 60.0);
    }

    #[test]
    fn an_in_window_menu_taller_than_the_window_is_not_moved() {
        let window = Size::new(1000.0, 500.0);
        // It scrolls from where it opened, as it always has
        let menu = in_window_menu_at(window, Point::new(100.0, 300.0));
        assert_eq!(menu.y, -500.0);
        let menu = in_window_menu_at(window, Point::new(100.0, 100.0));
        assert_eq!(menu.y, 101.0);
    }
}
