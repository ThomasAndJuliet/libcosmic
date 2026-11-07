// SPDX-License-Identifier: MPL-2.0

//! The selection fill covers the first text pass: text inputs and text
//! editors, drawn headless with tiny-skia (needs a font for "M")

use crate::{Element, Theme, widget};
use iced_core::layout::Limits;
use iced_core::renderer::Headless;
use iced_core::widget::{Id, Tree, operation};
use iced_core::{Color, Layout, Pixels, Rectangle, Size, mouse, renderer};
use std::sync::Arc;

const SIZE: Size<u32> = Size::new(260, 80);
const BIG: f32 = 48.0;

/// The light theme with a dark-red accent, #7f2400, far from the grey text
fn theme() -> Theme {
    let accent = palette::Srgb::new(0x7f as f32 / 255.0, 0x24 as f32 / 255.0, 0.0);
    let theme = cosmic_theme::ThemeBuilder::light().accent(accent).build();
    Theme::custom(Arc::new(theme))
}

/// `colour` as the 0xRRGGBB the checks take
fn rgb(colour: impl Into<Color>) -> u32 {
    let [r, g, b, _] = colour.into().into_rgba8();
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

fn renderer() -> crate::Renderer {
    iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
        crate::font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("a headless tiny-skia renderer")
}

/// Lets `prepare` act on the widget tree, lays `element` out, focuses `id`,
/// draws it and returns the RGBA pixels
fn render(mut element: Element<'_, ()>, id: &Id, prepare: impl FnOnce(&mut Tree)) -> Vec<u8> {
    let mut renderer = renderer();
    let theme = theme();
    let bounds = Rectangle::with_size(Size::new(SIZE.width as f32, SIZE.height as f32));
    let mut tree = Tree::new(&element);
    prepare(&mut tree);
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &Limits::new(Size::ZERO, bounds.size()),
    );
    let layout = Layout::new(&node);
    element.as_widget_mut().operate(
        &mut tree,
        layout,
        &renderer,
        &mut operation::focusable::focus(id.clone()),
    );
    let style = renderer::Style {
        icon_color: Color::BLACK,
        text_color: Color::BLACK,
        scale_factor: 1.0,
    };
    element.as_widget().draw(
        &tree,
        &mut renderer,
        &theme,
        &style,
        layout,
        mouse::Cursor::Unavailable,
        &bounds,
    );
    renderer.screenshot(SIZE, 1.0, Color::WHITE)
}

/// Inside the selection rect (the box of the pixels at `fill`) every pixel
/// is a blend of `fill` and `text` (±4 per channel): none carries the
/// unselected text's colour, whole or as a fringe; and the selected text is
/// there
fn assert_only_fill_and_text(pixels: &[u8], fill: u32, text: u32) {
    let rgb = |c: u32| [(c >> 16) as f32, (c >> 8 & 0xff) as f32, (c & 0xff) as f32];
    let (fill, text) = (rgb(fill), rgb(text));
    let at = |i: usize| {
        [
            pixels[4 * i] as f32,
            pixels[4 * i + 1] as f32,
            pixels[4 * i + 2] as f32,
        ]
    };
    let is_fill = |p: [f32; 3]| (0..3).all(|c| (p[c] - fill[c]).abs() <= 1.0);
    let (w, h) = (SIZE.width as usize, SIZE.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for i in (0..w * h).filter(|&i| is_fill(at(i))) {
        (x0, x1) = (x0.min(i % w), x1.max(i % w));
        (y0, y1) = (y0.min(i / w), y1.max(i / w));
    }
    assert!(
        x0 <= x1 && y0 <= y1,
        "no selection fill #{fill:?} was drawn"
    );
    let dir = [text[0] - fill[0], text[1] - fill[1], text[2] - fill[2]];
    let len2: f32 = dir.iter().map(|d| d * d).sum();
    let mut solid_text = 0;
    for (x, y) in (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))) {
        let p = at(y * w + x);
        let k = (0..3).map(|c| (p[c] - fill[c]) * dir[c]).sum::<f32>() / len2;
        let blend = |c: usize| fill[c] + k * dir[c];
        assert!(
            (0..3).all(|c| (p[c] - blend(c)).abs() <= 4.0),
            "pixel ({x}, {y}) {p:?} is no blend of {fill:?} and {text:?}"
        );
        solid_text += usize::from(k > 0.9);
    }
    assert!(
        solid_text > 20,
        "the selected text was not drawn in {text:?}"
    );
}

fn text_editor_style(_: &Theme, _: widget::text_editor::Status) -> widget::text_editor::Style {
    widget::text_editor::Style {
        background: iced_core::Background::Color(Color::from_rgb8(0xf5, 0xf5, 0xf5)),
        border: iced_core::Border::default(),
        placeholder: Color::from_rgb8(0x88, 0x88, 0x88),
        // Neither the fill nor the selected text, so a trace of it shows
        value: Color::from_rgb8(0x8a, 0x8a, 0x8a),
        selection: Color::from_rgb8(0x7f, 0x24, 0x00),
    }
}

fn selected_content() -> widget::text_editor::Content<crate::Renderer> {
    let mut content = widget::text_editor::Content::with_text("MMMM");
    content.perform(iced_widget::text_editor::Action::SelectAll);
    content
}

fn selected_editor<'a>(
    content: &'a widget::text_editor::Content<crate::Renderer>,
    id: &Id,
) -> widget::text_editor::TextEditor<'a, ()> {
    widget::text_editor::text_editor(content)
        .id(id.clone())
        .on_action(|_| ())
        .size(BIG)
        .style(text_editor_style)
}

/// A text editor's selection fill, the style's `selection`, covers the
/// unselected grey text, and the selected text is the theme's `accent.on`
#[test]
fn text_editor_selection_covers_the_unselected_text() {
    let (id, content) = (Id::unique(), selected_content());
    let pixels = render(selected_editor(&content, &id).into(), &id, |_| {});
    assert_only_fill_and_text(&pixels, 0x7f2400, rgb(theme().cosmic().accent.on));
}

/// An app's own fill, through `selection_fill`, covers the unselected text
/// the same way, with the app's selected text colour on it
#[test]
fn text_editor_selection_fill_covers_the_unselected_text() {
    let (id, content) = (Id::unique(), selected_content());
    let editor = selected_editor(&content, &id)
        .selection_fill(|_, _| Color::from_rgb8(0x2a, 0x5d, 0x8f))
        .selected_text_color(|_, _| Color::from_rgb8(0xff, 0xff, 0xff));
    let pixels = render(editor.into(), &id, |_| {});
    assert_only_fill_and_text(&pixels, 0x2a5d8f, 0xffffff);
}

/// A text input's selection fill, the style's `selected_fill` (the accent),
/// covers the unselected text, and the selected text is the style's
/// `selected_text_color` (the theme's on-accent colour)
#[test]
fn text_input_selection_covers_the_unselected_text() {
    let id = Id::unique();
    let input = widget::text_input("", "MMMM").id(id.clone()).size(BIG);
    let pixels = render(input.into(), &id, |tree| {
        let state = tree.state.downcast_mut::<widget::text_input::State>();
        // Its text is shaped in the layout only when the state is dirty
        state.dirty = true;
        state.focus();
        state.select_all();
    });
    let cosmic = theme();
    let cosmic = cosmic.cosmic();
    assert_only_fill_and_text(
        &pixels,
        rgb(cosmic.accent_color()),
        rgb(cosmic.on_accent_color()),
    );
}

// Native menu operations share these selection fixtures with the rendering tests.
use crate::widget::text_context_menu::{HasSelectableText, TextAction, TextMenuTarget};
use iced_core::widget::Widget;
use iced_core::{Clipboard, Event, Shell, clipboard, window};

#[derive(Default)]
struct MenuClipboard {
    text: Option<String>,
    writes: Vec<String>,
}

impl Clipboard for MenuClipboard {
    fn read(&self, _: clipboard::Kind) -> Option<String> {
        self.text.clone()
    }

    fn write(&mut self, _: clipboard::Kind, text: String) {
        self.writes.push(text.clone());
        self.text = Some(text);
    }
}

#[derive(Clone, Debug, PartialEq)]
enum MenuMessage {
    Input(String),
    Paste(String),
    Edit(widget::text_editor::Action),
}

fn target_tree(target: &mut impl Widget<MenuMessage, Theme, crate::Renderer>) -> Tree {
    let mut tree = Tree::new(&*target as &dyn Widget<MenuMessage, Theme, crate::Renderer>);
    target.diff(&mut tree);
    tree
}

fn menu_action(
    target: &mut impl TextMenuTarget<MenuMessage>,
    tree: &mut Tree,
    action: TextAction,
    clipboard: &mut MenuClipboard,
) -> Vec<MenuMessage> {
    let mut messages = Vec::new();
    target.execute_text_menu_action(tree, action, clipboard, &mut Shell::new(&mut messages));
    messages
}

fn redraw_target(
    target: &mut impl Widget<MenuMessage, Theme, crate::Renderer>,
    tree: &mut Tree,
    renderer: &crate::Renderer,
    clipboard: &mut MenuClipboard,
) -> Vec<MenuMessage> {
    let bounds = Rectangle::with_size(Size::new(260.0, 80.0));
    let node = target.layout(tree, renderer, &Limits::new(Size::ZERO, bounds.size()));
    let mut messages = Vec::new();
    target.update(
        tree,
        &Event::Window(window::Event::RedrawRequested(
            iced_core::time::Instant::now(),
        )),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        renderer,
        clipboard,
        &mut Shell::new(&mut messages),
        &bounds,
    );
    messages
}

#[test]
fn text_menu_secure_input_rejects_copy_and_cut_after_opening() {
    let mut input = widget::text_input("", "secret").on_input(MenuMessage::Input);
    let mut tree = target_tree(&mut input);
    tree.state
        .downcast_mut::<widget::text_input::State>()
        .select_all();
    assert!(input.text_menu_context(&tree).allows(TextAction::Copy));
    let mut input = input.password();
    let mut clipboard = MenuClipboard::default();
    for action in [TextAction::Copy, TextAction::Cut] {
        assert!(menu_action(&mut input, &mut tree, action, &mut clipboard).is_empty());
    }
    assert!(
        clipboard.writes.is_empty(),
        "a stale menu cannot export a password"
    );
    assert_eq!(
        tree.state
            .downcast_ref::<widget::text_input::State>()
            .tracked_value
            .to_string(),
        "secret"
    );
    assert!(
        input.selected_text(&tree).is_none(),
        "the legacy copy path must also hide passwords"
    );
}

#[test]
fn text_menu_read_only_input_rechecks_permissions_before_editing() {
    let mut input = widget::text_input("", "kept").on_input(MenuMessage::Input);
    let mut tree = target_tree(&mut input);
    tree.state
        .downcast_mut::<widget::text_input::State>()
        .select_all();
    assert!(input.text_menu_context(&tree).allows(TextAction::Cut));
    let mut input = widget::text_input("", "kept")
        .on_input(MenuMessage::Input)
        .editing(false);
    let mut clipboard = MenuClipboard {
        text: Some("replacement".into()),
        ..Default::default()
    };
    for action in [TextAction::Cut, TextAction::Paste] {
        assert!(menu_action(&mut input, &mut tree, action, &mut clipboard).is_empty());
    }
    assert!(clipboard.writes.is_empty());
    assert_eq!(
        tree.state
            .downcast_ref::<widget::text_input::State>()
            .tracked_value
            .to_string(),
        "kept"
    );
    menu_action(&mut input, &mut tree, TextAction::Copy, &mut clipboard);
    assert_eq!(clipboard.writes, ["kept"]);

    let mut disabled = widget::text_input("", "kept");
    assert!(!disabled.text_menu_context(&tree).allows(TextAction::Cut));
    assert!(menu_action(&mut disabled, &mut tree, TextAction::Paste, &mut clipboard).is_empty());
}

#[test]
fn text_menu_paste_filters_controls_replaces_selection_and_prefers_on_paste() {
    let mut input = widget::text_input("", "aéz")
        .on_input(MenuMessage::Input)
        .on_paste(MenuMessage::Paste);
    let mut tree = target_tree(&mut input);
    tree.state
        .downcast_mut::<widget::text_input::State>()
        .select_range(1, 2);
    let mut clipboard = MenuClipboard {
        text: Some("世\n界\t".into()),
        ..Default::default()
    };
    // The snapshot can be stale: execution must read the live clipboard.
    assert!(!input.text_menu_context(&tree).clipboard_has_text);
    assert_eq!(
        menu_action(&mut input, &mut tree, TextAction::Paste, &mut clipboard),
        [MenuMessage::Paste("a世界z".into())]
    );
    let state = tree.state.downcast_ref::<widget::text_input::State>();
    assert_eq!(state.tracked_value.to_string(), "a世界z");
    assert!(state.cursor().selection(&state.tracked_value).is_none());
    assert!(
        state.dirty,
        "the native paragraph must be rebuilt after a menu edit"
    );
}

#[test]
fn text_menu_paste_uses_on_input_and_preserves_managed_values_without_callbacks() {
    let mut input = widget::text_input("", "old").on_input(MenuMessage::Input);
    let mut tree = target_tree(&mut input);
    tree.state
        .downcast_mut::<widget::text_input::State>()
        .select_all();
    let mut clipboard = MenuClipboard {
        text: Some("new".into()),
        ..Default::default()
    };
    assert_eq!(
        menu_action(&mut input, &mut tree, TextAction::Paste, &mut clipboard),
        [MenuMessage::Input("new".into())]
    );
    let mut managed = widget::text_input("", "").manage_value(true);
    let mut tree = target_tree(&mut managed);
    assert!(menu_action(&mut managed, &mut tree, TextAction::Paste, &mut clipboard).is_empty());
    managed.diff(&mut tree);
    assert_eq!(
        tree.state
            .downcast_ref::<widget::text_input::State>()
            .tracked_value
            .to_string(),
        "new"
    );
    clipboard.text = None;
    tree.state
        .downcast_mut::<widget::text_input::State>()
        .select_all();
    menu_action(&mut managed, &mut tree, TextAction::Paste, &mut clipboard);
    assert_eq!(
        tree.state
            .downcast_ref::<widget::text_input::State>()
            .tracked_value
            .to_string(),
        "new",
        "a vanished clipboard must not delete the selection"
    );
    clipboard.text = Some(String::new());
    menu_action(&mut managed, &mut tree, TextAction::Paste, &mut clipboard);
    assert_eq!(
        tree.state
            .downcast_ref::<widget::text_input::State>()
            .tracked_value
            .to_string(),
        "new",
        "an empty clipboard must not delete the selection"
    );
}

#[test]
fn text_menu_editor_emits_each_typed_action_once_before_another_event() {
    use widget::text_editor::{Action, Edit};
    let renderer = renderer();
    let mut content = widget::text_editor::Content::with_text("selected");
    content.perform(Action::SelectAll);
    let mut editor = widget::text_editor::text_editor(&content).on_action(MenuMessage::Edit);
    let mut tree = target_tree(&mut editor);
    let mut clipboard = MenuClipboard {
        text: Some("paste\ntext".into()),
        ..Default::default()
    };
    for (choice, expected) in [
        (TextAction::SelectAll, Action::SelectAll),
        (TextAction::Cut, Action::Edit(Edit::Delete)),
        (
            TextAction::Paste,
            Action::Edit(Edit::Paste(Arc::new("selected".into()))),
        ),
    ] {
        assert_eq!(
            menu_action(&mut editor, &mut tree, choice, &mut clipboard),
            [MenuMessage::Edit(expected)]
        );
        assert!(
            redraw_target(&mut editor, &mut tree, &renderer, &mut clipboard).is_empty(),
            "the emitted menu action must not be retained for another event"
        );
        assert_eq!(
            content.text(),
            "selected",
            "only the app performs edits on Content"
        );
    }
    assert_eq!(clipboard.writes, ["selected"]);
}

#[test]
fn text_menu_read_only_editor_selects_and_copies_without_granting_edits() {
    let content = widget::text_editor::Content::with_text("read only");
    let mut editor = widget::text_editor::text_editor(&content);
    let mut tree = target_tree(&mut editor);
    let mut clipboard = MenuClipboard {
        text: Some("replacement".into()),
        ..Default::default()
    };
    assert!(
        menu_action(
            &mut editor,
            &mut tree,
            TextAction::SelectAll,
            &mut clipboard
        )
        .is_empty()
    );
    assert_eq!(content.selection().as_deref(), Some("read only"));
    for choice in [TextAction::Cut, TextAction::Paste] {
        assert!(menu_action(&mut editor, &mut tree, choice, &mut clipboard).is_empty());
    }
    menu_action(&mut editor, &mut tree, TextAction::Copy, &mut clipboard);
    assert_eq!(clipboard.writes, ["read only"]);
    assert_eq!(content.text(), "read only");
}

#[test]
fn text_menu_selectable_text_only_copies_and_selects() {
    let renderer = renderer();
    let mut text = widget::selectable_text("copy only");
    <_ as TextMenuTarget<MenuMessage>>::use_external_text_menu(&mut text);
    let mut tree = target_tree(&mut text);
    let mut clipboard = MenuClipboard {
        text: Some("replacement".into()),
        ..Default::default()
    };
    redraw_target(&mut text, &mut tree, &renderer, &mut clipboard);
    menu_action(&mut text, &mut tree, TextAction::SelectAll, &mut clipboard);
    for choice in [TextAction::Cut, TextAction::Paste] {
        assert!(menu_action(&mut text, &mut tree, choice, &mut clipboard).is_empty());
    }
    menu_action(&mut text, &mut tree, TextAction::Copy, &mut clipboard);
    assert_eq!(clipboard.writes, ["copy only"]);
}

fn external_menu_request(
    target: &mut (impl Widget<MenuMessage, Theme, crate::Renderer> + TextMenuTarget<MenuMessage>),
    renderer: &crate::Renderer,
) {
    target.use_external_text_menu();
    let mut tree = target_tree(target);
    let bounds = Rectangle::with_size(Size::new(260.0, 80.0));
    let node = target.layout(&mut tree, renderer, &Limits::new(Size::ZERO, bounds.size()));
    let layout = Layout::new(&node);
    let at = layout.bounds().center();
    let mut messages = Vec::new();
    target.update(
        &mut tree,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        layout,
        mouse::Cursor::Available(at),
        renderer,
        &mut MenuClipboard::default(),
        &mut Shell::new(&mut messages),
        &bounds,
    );
    assert!(
        target
            .overlay(
                &mut tree,
                layout,
                renderer,
                &bounds,
                iced_core::Vector::ZERO
            )
            .is_none(),
        "external presentation must suppress the stock overlay"
    );
    assert_eq!(target.take_text_menu_request(&mut tree), Some(at));
    assert_eq!(
        target.take_text_menu_request(&mut tree),
        None,
        "each native request is consumed once"
    );
    #[cfg(wayland_platform)]
    assert!(
        crate::widget::text_context_menu::take_popup_requests(|_| true).is_empty(),
        "external presentation must suppress stock native surfaces"
    );
}

#[test]
fn text_menu_external_presentation_preserves_native_requests() {
    let renderer = renderer();
    let mut input = widget::text_input("", "input").on_input(MenuMessage::Input);
    external_menu_request(&mut input, &renderer);
    let content = widget::text_editor::Content::with_text("editor");
    let mut editor = widget::text_editor::text_editor(&content).on_action(MenuMessage::Edit);
    external_menu_request(&mut editor, &renderer);
    let mut text = widget::selectable_text("text");
    external_menu_request(&mut text, &renderer);
    let mut plain = widget::text("plain").selectable();
    external_menu_request(&mut plain, &renderer);
}

#[test]
fn named_selectable_text_preserves_its_selection_through_tree_reconciliation() {
    use iced_core::widget::tree::NAMED;
    let renderer = renderer();
    let limits = Limits::new(Size::ZERO, Size::new(260.0, 80.0));
    for (context_menu, external) in [(true, false), (true, true), (false, false)] {
        let id = Id::new("named-selectable-text");
        let target = || {
            let mut text =
                widget::selectable_text("keep this selection").context_menu(context_menu);
            <_ as Widget<MenuMessage, Theme, crate::Renderer>>::set_id(&mut text, id.clone());
            if external {
                <_ as TextMenuTarget<MenuMessage>>::use_external_text_menu(&mut text);
            }
            text
        };
        let mut old = target();
        let mut tree = target_tree(&mut old);
        let _ = <_ as Widget<MenuMessage, Theme, crate::Renderer>>::layout(
            &mut old, &mut tree, &renderer, &limits,
        );
        let mut clipboard = MenuClipboard::default();
        menu_action(&mut old, &mut tree, TextAction::SelectAll, &mut clipboard);

        // This is the extraction/reconciliation cycle used by UserInterface::build.
        NAMED.with(|named| *named.borrow_mut() = tree.take_all_named());
        let mut rebuilt = target();
        tree.diff(&mut rebuilt as &mut dyn Widget<MenuMessage, Theme, crate::Renderer>);
        let _ = <_ as Widget<MenuMessage, Theme, crate::Renderer>>::layout(
            &mut rebuilt,
            &mut tree,
            &renderer,
            &limits,
        );
        NAMED.with(|named| named.borrow_mut().clear());
        menu_action(&mut rebuilt, &mut tree, TextAction::Copy, &mut clipboard);
        assert_eq!(
            clipboard.writes,
            ["keep this selection"],
            "named native text state retains selection with stock, external or disabled menus"
        );
        assert_eq!(tree.id, (!context_menu).then_some(id.clone()));
        if context_menu {
            assert_eq!(
                tree.children[0].id,
                Some(id),
                "the native text child is the sole owner of the name"
            );
        }
    }
}
