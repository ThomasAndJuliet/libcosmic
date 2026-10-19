// From iced_aw, license MIT

//! A tree structure for constructing a hierarchical menu

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use iced::advanced::widget::text::Style as TextStyle;
use iced_widget::core::{Element, renderer};

use crate::widget::menu::action::MenuAction;
use crate::widget::menu::key_bind::KeyBind;
use crate::widget::{Button, RcElementWrapper, icon};
use crate::{theme, widget};
use iced_core::{Alignment, Length};

/// Nested menu is essentially a tree of items, a menu is a collection of items
/// a menu itself can also be an item of another menu.
///
/// A `MenuTree` represents a node in the tree, it holds a widget as a menu item
/// for its parent, and a list of menu tree as child nodes.
/// Conceptually a node is either a menu(inner node) or an item(leaf node),
/// but there's no need to explicitly distinguish them here, if a menu tree
/// has children, it's a menu, otherwise it's an item
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct MenuTree<Message> {
    /// The menu tree will be flatten into a vector to build a linear widget tree,
    /// the `index` field is the index of the item in that vector
    pub(crate) index: usize,

    /// The item of the menu tree
    pub(crate) item: RcElementWrapper<Message>,
    /// The children of the menu tree
    pub(crate) children: Vec<MenuTree<Message>>,
    /// The width of the menu tree
    pub(crate) width: Option<u16>,
    /// The height of the menu tree
    pub(crate) height: Option<u16>,
}

impl<Message: Clone + 'static> MenuTree<Message> {
    /// Create a new menu tree from a widget
    pub fn new(item: impl Into<RcElementWrapper<Message>>) -> Self {
        Self {
            index: 0,
            item: item.into(),
            children: Vec::new(),
            width: None,
            height: None,
        }
    }

    /// Create a menu tree from a widget and a vector of sub trees
    pub fn with_children(
        item: impl Into<RcElementWrapper<Message>>,
        children: Vec<impl Into<MenuTree<Message>>>,
    ) -> Self {
        Self {
            index: 0,
            item: item.into(),
            children: children.into_iter().map(Into::into).collect(),
            width: None,
            height: None,
        }
    }

    /// Sets the width of the menu tree.
    /// See [`ItemWidth`]
    ///
    /// [`ItemWidth`]:`super::ItemWidth`
    #[must_use]
    pub fn width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the height of the menu tree.
    /// See [`ItemHeight`]
    ///
    /// [`ItemHeight`]: `super::ItemHeight`
    #[must_use]
    pub fn height(mut self, height: u16) -> Self {
        self.height = Some(height);
        self
    }

    /* Keep `set_index()` and `flattern()` recurse in the same order */

    /// Set the index of each item
    pub(crate) fn set_index(&mut self) {
        /// inner counting function.
        fn rec<Message: Clone + 'static>(mt: &mut MenuTree<Message>, count: &mut usize) {
            // keep items under the same menu line up
            mt.children.iter_mut().for_each(|c| {
                c.index = *count;
                *count += 1;
            });

            mt.children.iter_mut().for_each(|c| rec(c, count));
        }

        let mut count = 0;
        self.index = count;
        count += 1;
        rec(self, &mut count);
    }

    /// Flatten the menu tree
    pub(crate) fn flattern(&self) -> Vec<&Self> {
        /// Inner flattening function
        fn rec<'a, Message: Clone + 'static>(
            mt: &'a MenuTree<Message>,
            flat: &mut Vec<&'a MenuTree<Message>>,
        ) {
            mt.children.iter().for_each(|c| {
                flat.push(c);
            });

            mt.children.iter().for_each(|c| {
                rec(c, flat);
            });
        }

        let mut flat = Vec::new();
        flat.push(self);
        rec(self, &mut flat);

        flat
    }
}

impl<Message: Clone + 'static> From<crate::Element<'static, Message>> for MenuTree<Message> {
    fn from(value: crate::Element<'static, Message>) -> Self {
        Self::new(RcElementWrapper::new(value))
    }
}

pub fn menu_button<'a, Message>(
    children: Vec<crate::Element<'a, Message>>,
) -> crate::widget::Button<'a, Message>
where
    Message: std::clone::Clone + 'a,
{
    widget::button::custom(
        widget::Row::from_vec(children)
            .align_y(Alignment::Center)
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .height(Length::Fixed(36.0))
    .padding([4, 16])
    .width(Length::Fill)
    .class(theme::Button::MenuItem)
}

#[derive(Clone)]
/// Represents a menu item that performs an action when selected or a separator between menu items.
///
/// - `Action` - Represents a menu item that performs an action when selected.
///     - `L` - The label of the menu item.
///     - `A` - The action to perform when the menu item is selected, the action must implement the `MenuAction` trait.
/// - `CheckBox` - Represents a checkbox menu item.
///     - `L` - The label of the menu item.
///     - `bool` - The state of the checkbox.
///     - `A` - The action to perform when the menu item is selected, the action must implement the `MenuAction` trait.
/// - `Folder` - Represents a folder menu item.
///     - `L` - The label of the menu item.
///     - `Vec<MenuItem<A, L>>` - A vector of menu items.
/// - `Divider` - Represents a divider between menu items.
pub enum MenuItem<A: MenuAction, L: Into<Cow<'static, str>>> {
    /// Represents a button menu item.
    Button(L, Option<icon::Handle>, A),
    /// Represents a button menu item that is disabled.
    ButtonDisabled(L, Option<icon::Handle>, A),
    /// Represents a checkbox menu item.
    CheckBox(L, Option<icon::Handle>, bool, A),
    /// Represents a folder menu item.
    Folder(L, Vec<MenuItem<A, L>>),
    /// Represents a divider between menu items.
    Divider,
    /// A menu entry with every option available; see [`Entry`].
    Entry(Entry<A, L>),
}

impl<A: MenuAction, L: Into<Cow<'static, str>>> MenuItem<A, L> {
    /// Create an [`Entry`] menu item, configure it with the builder methods on [`Entry`].
    pub fn entry(label: L, action: A) -> Self {
        MenuItem::Entry(Entry::new(label, action))
    }
}

/// The leading icon column of a menu entry.
#[derive(Clone, Debug, Default)]
pub enum IconSlot {
    /// No icon and no space reserved for one.
    #[default]
    None,
    /// No icon, but the space for an icon is reserved (indented entry)
    Reserved,
    /// An icon.
    Icon(icon::Handle),
}

impl From<Option<icon::Handle>> for IconSlot {
    fn from(icon: Option<icon::Handle>) -> Self {
        icon.map_or(IconSlot::None, IconSlot::Icon)
    }
}

/// A menu entry: label, optional leading icon, optional check column or selected fill, enabled state and action.
#[derive(Clone)]
pub struct Entry<A, L> {
    label: L,
    icon: IconSlot,
    /// `Some` draws the check column
    checked: Option<bool>,
    /// Drawn in the check column in place of the tick, see [`Entry::mark`]
    mark: Option<icon::Handle>,
    /// Drawn as the chosen one of several, see [`Entry::selected`]
    selected: bool,
    enabled: bool,
    action: A,
}

impl<A, L> Entry<A, L> {
    pub fn new(label: L, action: A) -> Self {
        Self {
            label,
            icon: IconSlot::None,
            checked: None,
            mark: None,
            selected: false,
            enabled: true,
            action,
        }
    }

    /// Draw a leading icon
    #[must_use]
    pub fn icon(mut self, icon: icon::Handle) -> Self {
        self.icon = IconSlot::Icon(icon);
        self
    }

    /// Draw no icon, but resever the space
    #[must_use]
    pub fn reserve_icon(mut self) -> Self {
        self.icon = IconSlot::Reserved;
        self
    }

    /// Show a check column, ticked when `checked` is true, empty sapce when false
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Draw `mark` in the check column, 16 px, in place of the tick: a
    /// radio mark for one choice of several, say. It takes the row's icon
    /// colour, which is the accent text colour on a [`Entry::selected`] row
    #[must_use]
    pub fn mark(mut self, mark: icon::Handle) -> Self {
        self.mark = Some(mark);
        self
    }

    /// Draw the entry as one choice of several, with no tick (a
    /// [`Entry::mark`] may stand in the check column instead): when
    /// `selected` is true the row is filled with the theme's selected
    /// colour and its label and icon drawn in the selected text colour,
    /// semibold, as a dropdown draws its chosen option; when false it is
    /// an ordinary entry. Use it in place of [`Entry::checked`]
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Disabled entries are drawn dimmed and do not react to presses
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Create a root menu item.
///
/// # Arguments
/// - `label` - The label of the menu item.
///
/// # Returns
/// - A button for the root menu item.
pub fn menu_root<'a, Message, Renderer: renderer::Renderer>(
    label: impl Into<Cow<'a, str>> + 'a,
) -> Button<'a, Message>
where
    Element<'a, Message, crate::Theme, Renderer>: From<widget::Button<'a, Message>>,
    Message: std::clone::Clone + 'a,
{
    widget::button::custom(widget::text(label))
        .padding([4, 12])
        .class(theme::Button::MenuRoot)
}

fn entry_tree<
    A: MenuAction<Message = Message>,
    L: Into<Cow<'static, str>>,
    Message: Clone + 'static,
>(
    entry: Entry<A, L>,
    key_binds: &HashMap<KeyBind, A>,
    key_class: theme::Text,
) -> MenuTree<Message> {
    let Entry {
        label,
        icon,
        checked,
        mark,
        selected,
        enabled,
        action,
    } = entry;
    let spacing = crate::theme::spacing();
    // Several bindings may map to one action
    // choose based on these rules:
    // - the fewest modifiers win (Delete vs Ctrl+D)
    // - if equal then character key over a named one (Ctrl+C over Ctrl+Insert)
    // - if equal then `Ord` to stay consistent since hashmap is arbitrary
    let rank = |k: &KeyBind| {
        (
            k.modifiers.len(),
            matches!(k.key, iced_core::keyboard::Key::Named(_)),
        )
    };
    let key = key_binds
        .iter()
        .filter(|(_, a)| **a == action)
        .map(|(k, _)| k)
        .min_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)))
        .map_or_else(String::new, ToString::to_string);

    let mut items: Vec<crate::Element<'static, Message>> = Vec::with_capacity(7);

    if let Some(mark) = mark {
        items.push(
            widget::icon::icon(mark)
                .size(16)
                .width(Length::Fixed(16.0))
                .into(),
        );
        items.push(widget::space::horizontal().width(spacing.space_xxs).into());
    } else if let Some(checked) = checked {
        items.push(if checked {
            widget::icon::from_name("object-select-symbolic")
                .size(16)
                .icon()
                .class(theme::Svg::Custom(Rc::new(|theme| {
                    iced_widget::svg::Style {
                        color: Some(theme.cosmic().accent_text_color().into()),
                    }
                })))
                .width(Length::Fixed(16.0))
                .into()
        } else {
            widget::space::horizontal()
                .width(Length::Fixed(16.0))
                .into()
        });
        items.push(widget::space::horizontal().width(spacing.space_xxs).into());
    }

    match icon {
        IconSlot::Icon(icon) => {
            items.push(widget::icon::icon(icon).size(14).into());
            items.push(widget::space::horizontal().width(spacing.space_xxs).into());
        }
        IconSlot::Reserved => {
            items.push(
                widget::space::horizontal()
                    .width(Length::Fixed(14.0))
                    .into(),
            );
            items.push(widget::space::horizontal().width(spacing.space_xxs).into());
        }
        IconSlot::None => {}
    }

    let ellipsize =
        iced_core::text::Ellipsize::Middle(iced_core::text::EllipsizeHeightLimit::Lines(1));
    let label = widget::text(label.into()).ellipsize(ellipsize);
    items.push(if selected {
        label.font(crate::font::semibold()).into()
    } else {
        label.into()
    });
    items.push(widget::space::horizontal().into());
    items.push(
        widget::text(key)
            .class(key_class)
            .ellipsize(ellipsize)
            .into(),
    );

    let mut button = menu_button(items);
    if selected {
        button = button.selected(true).class(selected_menu_item());
    }
    if enabled {
        button = button.on_press(action.message());
    }
    MenuTree::from(Element::from(button))
}

/// A chosen entry's look ([`Entry::selected`]): the menu item's own, filled
/// with the menu's selected colour, its label and icon in the accent text
/// colour, as libcosmic draws a dropdown's chosen option
fn selected_menu_item() -> theme::Button {
    use crate::widget::button::{Catalog, Style};
    use iced_core::{Background, Color};

    fn chosen(mut style: Style, theme: &crate::Theme, pressed: bool) -> Style {
        let cosmic = theme.cosmic();
        let component = &cosmic.background(theme.transparent).component;
        let fill = if pressed {
            component.pressed
        } else {
            component.selected_state_color()
        };
        let text = Color::from(cosmic.accent_text_color());
        style.background = Some(Background::Color(fill.into()));
        style.text_color = Some(text);
        style.icon_color = Some(text);
        style
    }

    theme::Button::Custom {
        active: Box::new(|focused, theme| {
            chosen(
                theme.active(focused, true, &theme::Button::MenuItem),
                theme,
                false,
            )
        }),
        disabled: Box::new(|theme| theme.disabled(&theme::Button::MenuItem)),
        hovered: Box::new(|focused, theme| {
            chosen(
                theme.hovered(focused, true, &theme::Button::MenuItem),
                theme,
                false,
            )
        }),
        pressed: Box::new(|focused, theme| {
            chosen(
                theme.pressed(focused, true, &theme::Button::MenuItem),
                theme,
                true,
            )
        }),
    }
}

/// Create a list of menu items from a vector of `MenuItem`.
///
/// The `MenuItem` can be either an action or a separator.
///
/// # Arguments
/// - `key_binds` - A reference to a `HashMap` that maps `KeyBind` to `A`.
/// - `children` - A vector of `MenuItem`.
///
/// # Returns
/// - A vector of `MenuTree`.
#[must_use]
pub fn menu_items<
    A: MenuAction<Message = Message>,
    L: Into<Cow<'static, str>> + 'static,
    Message: 'static + std::clone::Clone,
>(
    key_binds: &HashMap<KeyBind, A>,
    children: Vec<MenuItem<A, L>>,
) -> Vec<MenuTree<Message>> {
    fn key_style(theme: &crate::Theme) -> TextStyle {
        let mut color = theme.cosmic().background(theme.transparent).component.on;
        color.alpha *= 0.75;
        TextStyle {
            color: Some(color.into()),
            ..Default::default()
        }
    }
    let key_class = theme::Text::Custom(key_style);

    let size = children.len();

    children
        .into_iter()
        .enumerate()
        .flat_map(|(i, item)| {
            let mut trees = vec![];

            match item {
                MenuItem::Button(label, icon, action) => {
                    let mut entry = Entry::new(label, action);
                    entry.icon = icon.into();
                    trees.push(entry_tree(entry, key_binds, key_class.clone()));
                }
                MenuItem::ButtonDisabled(label, icon, action) => {
                    let mut entry = Entry::new(label, action).enabled(false);
                    entry.icon = icon.into();
                    trees.push(entry_tree(entry, key_binds, key_class.clone()));
                }
                MenuItem::CheckBox(label, icon, value, action) => {
                    let mut entry = Entry::new(label, action).checked(value);
                    entry.icon = icon.into();
                    trees.push(entry_tree(entry, key_binds, key_class.clone()));
                }
                MenuItem::Entry(entry) => {
                    trees.push(entry_tree(entry, key_binds, key_class.clone()));
                }
                MenuItem::Folder(label, children) => {
                    let l: Cow<'static, str> = label.into();

                    trees.push(MenuTree::<Message>::with_children(
                        RcElementWrapper::new(crate::Element::from(
                            menu_button::<'static, _>(vec![
                                widget::text(l.clone())
                                    .ellipsize(iced_core::text::Ellipsize::Middle(
                                        iced_core::text::EllipsizeHeightLimit::Lines(1),
                                    ))
                                    .into(),
                                widget::space::horizontal().into(),
                                widget::icon::from_name("pan-end-symbolic")
                                    .size(16)
                                    .icon()
                                    .into(),
                            ])
                            .class(
                                // Menu folders have no on_press so they take on the disabled style by default
                                if children.is_empty() {
                                    // This will make the folder use the disabled style if it has no children
                                    theme::Button::MenuItem
                                } else {
                                    // This will make the folder use the enabled style if it has children
                                    theme::Button::MenuFolder
                                },
                            ),
                        )),
                        menu_items(key_binds, children),
                    ));
                }
                MenuItem::Divider => {
                    // A divider at either end separates nothing
                    if i != 0 && i != size - 1 {
                        trees.push(MenuTree::<Message>::from(Element::from(
                            widget::divider::horizontal::light(),
                        )));
                    }
                }
            }
            trees
        })
        .collect()
}

/// Create a menu tree from a widget and a vector of sub trees
pub fn nav_context<
    A: MenuAction<Message = Message>,
    L: Into<Cow<'static, str>> + From<&'static str> + 'static,
    Message: 'static + std::clone::Clone,
>(
    key_binds: &HashMap<KeyBind, A>,
    children: Vec<Vec<MenuItem<A, L>>>,
) -> Vec<MenuTree<Message>> {
    let menus = children
        .into_iter()
        .map(|m| MenuItem::<A, L>::Folder(L::from(""), m));
    let root = vec![MenuItem::<A, L>::Folder(L::from(""), menus.collect())];
    menu_items(key_binds, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use iced_core::layout::{Limits, Node};
    use iced_core::mouse::Cursor;
    use iced_core::widget::Tree;
    use iced_core::{Color, Rectangle, Size};
    use iced_runtime::user_interface::{Cache, UserInterface};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Pick;

    impl MenuAction for Pick {
        type Message = ();

        fn message(&self) {}
    }

    type TestEntry = Entry<Pick, &'static str>;

    const WIDTH: f32 = 300.0;
    const HEIGHT: f32 = 36.0;

    /// The interface font's weights, as an app loads them at start, so that the
    /// regular and semibold labels are measured with their own faces
    fn load_fonts() {
        static LOADED: std::sync::Once = std::sync::Once::new();
        LOADED.call_once(|| {
            let mut font_system = iced::advanced::graphics::text::font_system()
                .write()
                .unwrap();
            for font in [
                &include_bytes!("../../../res/open-sans/OpenSans-Regular.ttf")[..],
                &include_bytes!("../../../res/open-sans/OpenSans-Semibold.ttf")[..],
            ] {
                font_system.load_font(Cow::Borrowed(font));
            }
        });
    }

    fn renderer() -> crate::Renderer {
        load_fonts();
        iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::default(),
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer")
    }

    fn element(entry: TestEntry) -> crate::Element<'static, ()> {
        let tree = entry_tree(entry, &HashMap::new(), theme::Text::default());
        crate::Element::from(tree.item)
    }

    fn mark_handle() -> icon::Handle {
        icon::from_svg_bytes(
            &br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="4" fill="#000"/></svg>"##[..],
        )
        .symbolic(true)
    }

    fn glyph_handle() -> icon::Handle {
        icon::from_svg_bytes(
            &br##"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14"><rect width="14" height="14" fill="#000"/></svg>"##[..],
        )
        .symbolic(true)
    }

    /// Every leaf widget of the laid out row, in order, in the row's coordinates
    fn leaves(entry: TestEntry) -> Vec<Rectangle> {
        fn walk(node: &Node, origin: iced_core::Point, out: &mut Vec<Rectangle>) {
            let bounds = node.bounds();
            let at = iced_core::Point::new(origin.x + bounds.x, origin.y + bounds.y);
            if node.children().is_empty() {
                out.push(Rectangle::new(at, bounds.size()));
            }
            for child in node.children() {
                walk(child, at, out);
            }
        }
        let renderer = renderer();
        let mut element = element(entry);
        let mut tree = Tree::new(&element);
        let limits = Limits::new(Size::ZERO, Size::new(WIDTH, HEIGHT)).width(WIDTH);
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let mut out = Vec::new();
        walk(&node, iced_core::Point::ORIGIN, &mut out);
        out
    }

    /// The label is the third from last leaf: the label, the space that
    /// pushes the key to the right, then the key
    fn label_of(leaves: &[Rectangle]) -> Rectangle {
        leaves[leaves.len() - 3]
    }

    fn glyph_of(leaves: &[Rectangle]) -> Rectangle {
        *leaves
            .iter()
            .find(|leaf| leaf.width == 14.0 && leaf.height == 14.0)
            .expect("the row's own icon")
    }

    fn sixteen(leaves: &[Rectangle]) -> Vec<Rectangle> {
        leaves
            .iter()
            .copied()
            .filter(|leaf| leaf.width == 16.0)
            .collect()
    }

    fn style_of(entry: TestEntry, theme: &crate::Theme) -> (Color, Option<Color>) {
        let mut renderer = renderer();
        let mut ui = UserInterface::build(
            element(entry),
            Size::new(WIDTH, HEIGHT),
            Cache::default(),
            &mut renderer,
        );
        ui.draw(
            &mut renderer,
            theme,
            &iced_core::renderer::Style {
                icon_color: Color::BLACK,
                text_color: Color::BLACK,
                scale_factor: 1.0,
            },
            Cursor::Unavailable,
        );
        let pixels = renderer.screenshot(
            Size::new(WIDTH as u32, HEIGHT as u32),
            1.0,
            Color::TRANSPARENT,
        );
        // The middle of the row's free space, right of the label
        let at = ((HEIGHT as usize / 2) * WIDTH as usize + 200) * 4;
        let pixel = Color::from_rgba8(
            pixels[at],
            pixels[at + 1],
            pixels[at + 2],
            f32::from(pixels[at + 3]) / 255.0,
        );
        (pixel, None)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() <= 2.0 / 255.0
    }

    fn rgb(color: Color) -> [f32; 3] {
        [color.r, color.g, color.b]
    }

    #[derive(Clone, Copy)]
    enum State {
        Rest,
        Hovered,
        Pressed,
    }

    fn chosen_style(state: State, theme: &crate::Theme) -> crate::widget::button::Style {
        let class = selected_menu_item();
        let theme::Button::Custom {
            active,
            hovered,
            pressed,
            ..
        } = &class
        else {
            panic!("a custom class");
        };
        match state {
            State::Rest => active(false, theme),
            State::Hovered => hovered(false, theme),
            State::Pressed => pressed(false, theme),
        }
    }

    /// (a) A chosen entry has no tick, and no column of its own either: its
    /// row has the leaves of an ordinary entry
    #[test]
    fn a_selected_entry_has_no_tick_and_no_check_column() {
        let plain = leaves(Entry::new("View", Pick).icon(glyph_handle()));
        let chosen = leaves(Entry::new("View", Pick).icon(glyph_handle()).selected(true));
        assert_eq!(chosen.len(), plain.len(), "no leaf added for a tick");
        assert!(sixteen(&chosen).is_empty(), "no 16 px tick: {chosen:?}");
        assert_eq!(
            glyph_of(&chosen).x,
            glyph_of(&plain).x,
            "the icon is where an ordinary row has it"
        );
        // An entry that is not selected is an ordinary entry
        let not = leaves(
            Entry::new("View", Pick)
                .icon(glyph_handle())
                .selected(false),
        );
        assert_eq!(not, plain);
    }

    /// (b) The chosen row is filled with the theme's selected colour, and an
    /// ordinary row is not, in light and dark
    #[test]
    fn a_selected_entry_is_filled_with_the_themes_selected_colour() {
        for theme in [crate::Theme::light(), crate::Theme::dark()] {
            let cosmic = theme.cosmic();
            let selected = cosmic
                .background(theme.transparent)
                .component
                .selected_state_color();
            let expected = rgb(Color::from(selected));
            let (chosen, _) = style_of(Entry::new("Chosen", Pick).selected(true), &theme);
            assert!(chosen.a > 0.99, "an opaque fill: {chosen:?}");
            let got = rgb(chosen);
            assert!(
                got.iter().zip(expected).all(|(a, b)| near(*a, b)),
                "{} fill {got:?} is the selected colour {expected:?}",
                if cosmic.is_dark { "dark" } else { "light" }
            );
            let (plain, _) = style_of(Entry::new("Chosen", Pick), &theme);
            assert!(
                plain.a < 0.99 || rgb(plain) != got,
                "an ordinary row is not filled the same: {plain:?}"
            );
        }
    }

    /// (b) The label and icon of a chosen row take the accent text colour at
    /// rest, hovered and pressed, on the selected fill (pressed: the pressed
    /// fill)
    #[test]
    fn a_selected_entry_draws_in_the_accent_text_colour() {
        for theme in [crate::Theme::light(), crate::Theme::dark()] {
            let cosmic = theme.cosmic();
            let text = Color::from(cosmic.accent_text_color());
            let component = &cosmic.background(theme.transparent).component;
            let selected = Color::from(component.selected_state_color());
            let pressed = Color::from(component.pressed);
            for (state, fill) in [
                (State::Rest, selected),
                (State::Hovered, selected),
                (State::Pressed, pressed),
            ] {
                let style = chosen_style(state, &theme);
                assert_eq!(style.text_color, Some(text));
                assert_eq!(
                    style.icon_color,
                    Some(text),
                    "the mark takes the row's icon colour"
                );
                assert_eq!(style.background, Some(iced_core::Background::Color(fill)));
            }
        }
    }

    /// (e) A disabled chosen row is drawn as any disabled menu item is: no
    /// selected fill, no accent text
    #[test]
    fn a_disabled_selected_entry_is_drawn_as_a_disabled_menu_item() {
        use crate::widget::button::Catalog;
        for theme in [crate::Theme::light(), crate::Theme::dark()] {
            let class = selected_menu_item();
            let theme::Button::Custom { disabled, .. } = &class else {
                panic!("a custom class");
            };
            let got = disabled(&theme);
            let plain = theme.disabled(&theme::Button::MenuItem);
            assert_eq!(got.background, plain.background);
            assert_eq!(got.text_color, plain.text_color);
            assert_eq!(got.icon_color, plain.icon_color);
        }
        // And the row it makes takes no press: a disabled entry is not filled
        let theme = crate::Theme::dark();
        let (disabled, _) = style_of(
            Entry::new("Gone", Pick).selected(true).enabled(false),
            &theme,
        );
        let selected = Color::from(
            theme
                .cosmic()
                .background(theme.transparent)
                .component
                .selected_state_color(),
        );
        assert!(
            disabled.a < 0.99
                || !rgb(disabled)
                    .iter()
                    .zip(rgb(selected))
                    .all(|(a, b)| near(*a, b)),
            "a disabled chosen row is not filled with the selected colour: {disabled:?}"
        );
    }

    /// (c) A mark stands in the check column where a tick would, 16 px wide, and
    /// the icon and label keep the places a ticked row gives them
    #[test]
    fn a_mark_takes_the_ticks_column_so_icons_and_labels_line_up() {
        let ticked = leaves(Entry::new("View", Pick).icon(glyph_handle()).checked(true));
        let blank = leaves(Entry::new("View", Pick).icon(glyph_handle()).checked(false));
        let marked = leaves(
            Entry::new("View", Pick)
                .icon(glyph_handle())
                .mark(mark_handle())
                .selected(true),
        );
        let first = |leaves: &[Rectangle]| leaves[0];
        for (name, row) in [("ticked", &ticked), ("blank", &blank), ("marked", &marked)] {
            assert_eq!(first(row).width, 16.0, "{name}: the check column is 16 px");
            assert_eq!(
                first(row).x,
                first(&ticked).x,
                "{name}: the column starts in the same place"
            );
            assert_eq!(
                glyph_of(row).x,
                glyph_of(&ticked).x,
                "{name}: the icon lines up"
            );
            assert_eq!(
                label_of(row).x,
                label_of(&ticked).x,
                "{name}: the label lines up"
            );
        }
    }

    /// (c) A mark is drawn in its own handle, 16 px square
    #[test]
    fn a_mark_is_16_px_square() {
        let marked = leaves(Entry::new("View", Pick).mark(mark_handle()));
        assert_eq!((marked[0].width, marked[0].height), (16.0, 16.0));
    }

    /// (d) A chosen row without an icon still keeps the column that rows with a
    /// blank column give: choices and ticked rows of one menu share their labels' x
    #[test]
    fn a_row_with_a_blank_column_and_a_marked_row_share_the_labels_place() {
        let blank = leaves(Entry::new("Other view", Pick).checked(false));
        let marked = leaves(
            Entry::new("This view", Pick)
                .mark(mark_handle())
                .selected(true),
        );
        assert_eq!(label_of(&blank).x, label_of(&marked).x);
    }

    /// (e) A check box item is unchanged: the tick column, a blank one when
    /// unchecked, and no selected fill
    #[test]
    fn a_check_box_item_keeps_its_tick_column_and_has_no_fill() {
        let theme = crate::Theme::dark();
        let items: Vec<MenuItem<Pick, &'static str>> = vec![
            MenuItem::CheckBox("On", None, true, Pick),
            MenuItem::CheckBox("Off", None, false, Pick),
        ];
        let trees = menu_items::<Pick, &'static str, ()>(&HashMap::new(), items);
        assert_eq!(trees.len(), 2);
        let on = leaves(Entry::new("On", Pick).checked(true));
        let off = leaves(Entry::new("Off", Pick).checked(false));
        assert_eq!(
            on.len(),
            off.len(),
            "a tick and a blank column take the same leaves"
        );
        assert_eq!(sixteen(&on).len(), 1);
        assert_eq!(sixteen(&on)[0].x, sixteen(&off)[0].x);
        assert_eq!(label_of(&on).x, label_of(&off).x);
        // The item from `menu_items` is the entry's own row: same size
        let mut renderer = renderer();
        for (tree, entry) in trees.into_iter().zip([
            Entry::new("On", Pick).checked(true),
            Entry::new("Off", Pick).checked(false),
        ]) {
            let mut item = crate::Element::from(tree.item);
            let mut state = Tree::new(&item);
            let limits = Limits::new(Size::ZERO, Size::new(WIDTH, HEIGHT)).width(WIDTH);
            let made = item.as_widget_mut().layout(&mut state, &renderer, &limits);
            let mut direct = element(entry);
            let mut state = Tree::new(&direct);
            let want = direct
                .as_widget_mut()
                .layout(&mut state, &renderer, &limits);
            assert_eq!(made.bounds(), want.bounds());
            assert_eq!(made.children().len(), want.children().len());
        }
        let _ = &mut renderer;
        // No fill from a check box
        let (checked, _) = style_of(Entry::new("On", Pick).checked(true), &theme);
        let selected = Color::from(
            theme
                .cosmic()
                .background(theme.transparent)
                .component
                .selected_state_color(),
        );
        assert!(
            checked.a < 0.99
                || !rgb(checked)
                    .iter()
                    .zip(rgb(selected))
                    .all(|(a, b)| near(*a, b)),
            "a ticked row is not filled with the selected colour: {checked:?}"
        );
    }

    /// (f) A chosen row's label is semibold, so it is wider than the ordinary row's
    #[test]
    fn a_selected_entry_has_a_semibold_label() {
        let plain = leaves(Entry::new("Terminal and paper", Pick));
        let chosen = leaves(Entry::new("Terminal and paper", Pick).selected(true));
        assert!(
            label_of(&chosen).width > label_of(&plain).width,
            "semibold {} is wider than regular {}",
            label_of(&chosen).width,
            label_of(&plain).width
        );
    }
}
