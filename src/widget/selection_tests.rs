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

/// Lets `prepare` act on the widget tree, lays `element` out, focuses `id`,
/// draws it and returns the RGBA pixels
fn render(mut element: Element<'_, ()>, id: &Id, prepare: impl FnOnce(&mut Tree)) -> Vec<u8> {
    let mut renderer = iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
        crate::font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("a headless tiny-skia renderer");
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
