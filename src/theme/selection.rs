// SPDX-License-Identifier: MPL-2.0

//! Selection colours by the household fill rule (household-ui RULES.md
//! rule 6, "What is drawn on a fill" and "What is highlighted, and how"):
//! iced colours in, iced colours out, the maths in `household-tones`.

use household_tones::{Srgb, Srgba};
use iced_core::Color;

fn rgb(color: Color) -> Srgb {
    Srgb::from([color.r, color.g, color.b])
}

fn with_alpha(color: Srgb, a: f32) -> Color {
    let [r, g, b]: [f32; 3] = color.into();
    Color::from_rgba(r, g, b, a)
}

/// `top` laid over `beneath`, as an opaque colour
pub(crate) fn opaque_over(top: Color, beneath: Color) -> Color {
    let top = Srgba::from([top.r, top.g, top.b, top.a]);
    let beneath = Srgba::from([beneath.r, beneath.g, beneath.b, 1.0]);
    with_alpha(household_tones::over(top, beneath).rgb(), 1.0)
}

/// What `text` is drawn in on the opaque solid `fill`, keeping its alpha
pub(crate) fn on_solid(text: Color, fill: Color) -> Color {
    with_alpha(household_tones::on_solid(rgb(text), rgb(fill)), text.a)
}

/// The selection tint of `accent` as it shows over `beneath`, opaque
pub(crate) fn tint(accent: Color, beneath: Color) -> Color {
    with_alpha(household_tones::tint(rgb(accent), rgb(beneath)), 1.0)
}

/// What `text` is drawn in on the tint of `accent` over `beneath`, greys
/// going to `strongest`, keeping its alpha
pub(crate) fn on_tint(text: Color, accent: Color, beneath: Color, strongest: Color) -> Color {
    with_alpha(
        household_tones::on_tint(rgb(text), rgb(accent), rgb(beneath), rgb(strongest)),
        text.a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(rgb: u32) -> Color {
        Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// The largest 8-bit channel difference between two colours
    fn off(a: Color, b: Color) -> i32 {
        let (a, b) = (a.into_rgba8(), b.into_rgba8());
        (0..3)
            .map(|i| (i32::from(a[i]) - i32::from(b[i])).abs())
            .max()
            .unwrap()
    }

    /// Greys on Juliet's dark-red accent go to white, as in a text box on a
    /// light theme (rule 6, 25 Oct)
    #[test]
    fn grey_text_in_a_text_box_is_selected_in_white() {
        let fill = opaque_over(hex(0x7f2400), hex(0xf5f5f5));
        for grey in [0x222222, 0x555555] {
            let got = on_solid(hex(grey), fill);
            assert!(off(got, Color::WHITE) <= 2, "#{grey:06x} came out {got:?}");
        }
    }

    /// A colour keeps its hue on the solid fill: red goes to #ff7f78 (rule 6,
    /// 25 Oct)
    #[test]
    fn coloured_text_keeps_its_hue_on_the_solid_fill() {
        let got = on_solid(hex(0xc01c28), hex(0x7f2400));
        assert!(off(got, hex(0xff7f78)) <= 2, "came out {got:?}");
    }

    /// A translucent colour made opaque over an opaque base is the source-over
    /// composite (rule 6, 25 Oct: the fill covers the first text pass)
    #[test]
    fn opaque_over_is_the_source_over_composite() {
        let (top, base) = (Color::from_rgba(0.5, 0.25, 0.0, 0.4), hex(0xebebeb));
        let got = opaque_over(top, base);
        assert_eq!(got.a, 1.0);
        for (t, b, g) in [
            (top.r, base.r, got.r),
            (top.g, base.g, got.g),
            (top.b, base.b, got.b),
        ] {
            assert!((g - (t * 0.4 + b * 0.6)).abs() < 1e-3, "{got:?}");
        }
        assert_eq!(opaque_over(hex(0x123456), base), hex(0x123456));
    }

    /// The tint of #7f2400 over #ebebeb is opaque #c8aba0, and grey text on it
    /// goes to the strongest text colour (rule 6, 25 Oct)
    #[test]
    fn the_tint_is_opaque_and_grey_text_on_it_goes_to_the_strongest() {
        let (accent, beneath, strongest) = (hex(0x7f2400), hex(0xebebeb), hex(0x222222));
        let fill = tint(accent, beneath);
        assert_eq!(fill.a, 1.0);
        assert!(off(fill, hex(0xc8aba0)) <= 2, "tint came out {fill:?}");
        let got = on_tint(hex(0x555555), accent, beneath, strongest);
        assert!(off(got, strongest) <= 2, "text came out {got:?}");
    }

    /// Text keeps its alpha through the fill rule, as a disabled input's text
    /// does (rule 6, 25 Oct)
    #[test]
    fn selected_text_keeps_its_alpha() {
        let text = Color {
            a: 0.5,
            ..hex(0x222222)
        };
        assert_eq!(on_solid(text, hex(0x7f2400)).a, 0.5);
        assert_eq!(
            on_tint(text, hex(0x7f2400), hex(0xebebeb), hex(0x222222)).a,
            0.5
        );
    }
}

/// The draw-order fix, drawn headless with tiny-skia (needs a font for
/// "M")
#[cfg(test)]
mod render {
    use crate::{Element, Theme, widget};
    use iced_core::layout::Limits;
    use iced_core::renderer::Headless;
    use iced_core::widget::{Id, Tree, operation};
    use iced_core::{Color, Layout, Pixels, Rectangle, Size, mouse, renderer};
    use std::sync::Arc;

    const SIZE: Size<u32> = Size::new(260, 80);
    const BIG: f32 = 48.0;

    /// The light theme with Juliet's dark-red accent, #7f2400
    fn theme() -> Theme {
        let accent = palette::Srgb::new(0x7f as f32 / 255.0, 0x24 as f32 / 255.0, 0.0);
        let theme = cosmic_theme::ThemeBuilder::light().accent(accent).build();
        Theme::custom(Arc::new(theme))
    }

    /// Lets `prepare` act on the widget tree, lays `element` out, focuses
    /// `id`, draws it and returns the RGBA pixels
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

    /// Inside the selection rect (the box of the pixels at `fill`) every
    /// pixel is a blend of `fill` and `text` (±4 per channel): none carries
    /// the unselected text's colour, whole or as a fringe; and the selected
    /// text is there
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
            // Not the colour the fill rule picks, so a trace of it shows
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

    /// A text editor's selection is a solid accent with white text, and the
    /// unselected grey text does not show under it (rule 6, Juliet, 25 Oct)
    #[test]
    fn text_editor_selection_covers_the_unselected_text() {
        let (id, content) = (Id::unique(), selected_content());
        let pixels = render(selected_editor(&content, &id).into(), &id, |_| {});
        assert_only_fill_and_text(&pixels, 0x7f2400, 0xffffff);
    }

    /// With `selection_tint` the fill is the opaque tint, #c8aba0 over
    /// #ebebeb, and the grey text is #222222 on it, with no trace of the
    /// unselected grey (rule 6, Juliet, 25 Oct)
    #[test]
    fn text_editor_tint_covers_the_unselected_text() {
        let (id, content) = (Id::unique(), selected_content());
        let editor = selected_editor(&content, &id).selection_tint(
            Some(Color::from_rgb8(0xeb, 0xeb, 0xeb)),
            Some(Color::from_rgb8(0x22, 0x22, 0x22)),
        );
        let pixels = render(editor.into(), &id, |_| {});
        assert_only_fill_and_text(&pixels, 0xc8aba0, 0x222222);
    }

    /// A text input's selection is a solid accent with white text, and the
    /// unselected text does not show under it (rule 6, Juliet, 25 Oct)
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
        assert_only_fill_and_text(&pixels, 0x7f2400, 0xffffff);
    }
}
