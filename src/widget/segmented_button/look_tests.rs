// SPDX-License-Identifier: MPL-2.0

//! How a segmented control's parts look when focused, hovered or pressed,
//! drawn headless with tiny-skia

use super::{SingleSelectModel, StyleSheet};
use crate::theme::SegmentedButton as Style;
use crate::widget::segmented_control;
use iced::advanced::renderer::Headless;
use iced_core::widget::operation::{self, Outcome};
use iced_core::{Background, Color, Event, Length, Point, Size, clipboard, mouse, renderer};
use iced_runtime::user_interface::{Cache, UserInterface};
use std::borrow::Cow;

const WIDTH: u32 = 400;
const HEIGHT: u32 = 80;

/// A point inside the first part, clear of its title and rounded corners
const IN_FIRST: Point = Point::new(30.0, 16.0);
/// The same point in the second part
const IN_SECOND: Point = Point::new(WIDTH as f32 - 30.0, 16.0);

/// What is done to the control before it is drawn
#[derive(Clone, Copy)]
enum Then {
    Rest,
    Focus,
    Hover(Point),
    Press(Point),
}

/// Two parts, "One" and "Two", with the `chosen` one active
fn model(chosen: usize) -> SingleSelectModel {
    let mut ids = Vec::new();
    let mut model = SingleSelectModel::builder()
        .insert(|b| b.text("One").with_id(|id| ids.push(id)))
        .insert(|b| b.text("Two").with_id(|id| ids.push(id)))
        .build();
    model.activate(ids[chosen]);
    model
}

/// The default horizontal Control style with `change` made to it
fn control_with(change: impl Fn(&crate::Theme, &mut super::Appearance) + 'static) -> Style {
    Style::Custom(Box::new(move |theme| {
        let mut appearance = theme.horizontal(&Style::Control);
        change(theme, &mut appearance);
        appearance
    }))
}

/// A horizontal segmented control over `model` in `style`
fn control(model: &SingleSelectModel, style: Style) -> crate::Element<'_, ()> {
    segmented_control::horizontal(model)
        .width(Length::Fixed(WIDTH as f32))
        .style(style)
        .on_activate(|_| ())
        .into()
}

/// Draws `control` in `theme`, after `then`; returns the RGBA pixels
fn render(control: crate::Element<'_, ()>, theme: &crate::Theme, then: Then) -> Vec<u8> {
    {
        let mut font_system = iced::advanced::graphics::text::font_system()
            .write()
            .unwrap();
        font_system.load_font(Cow::Borrowed(
            &include_bytes!("../../../res/open-sans/OpenSans-Regular.ttf")[..],
        ));
    }
    let mut renderer = iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
        iced::Font::default(),
        iced::Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("a headless renderer");
    let mut ui = UserInterface::build(
        control,
        Size::new(WIDTH as f32, HEIGHT as f32),
        Cache::default(),
        &mut renderer,
    );

    let mut cursor = mouse::Cursor::Unavailable;
    let mut events = Vec::new();
    match then {
        Then::Rest => {}
        Then::Focus => {
            let mut focus: Box<dyn operation::Operation> =
                Box::new(operation::focusable::focus_next());
            loop {
                ui.operate(&renderer, focus.as_mut());
                match focus.finish() {
                    Outcome::Chain(next) => focus = next,
                    _ => break,
                }
            }
        }
        Then::Hover(at) => {
            cursor = mouse::Cursor::Available(at);
            events.push(Event::Mouse(mouse::Event::CursorMoved { position: at }));
        }
        Then::Press(at) => {
            cursor = mouse::Cursor::Available(at);
            events.push(Event::Mouse(mouse::Event::CursorMoved { position: at }));
            events.push(Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            )));
        }
    }
    let _ = ui.update(
        &events,
        cursor,
        &mut renderer,
        &mut clipboard::Null,
        &mut Vec::new(),
    );

    ui.draw(
        &mut renderer,
        theme,
        &renderer::Style {
            icon_color: Color::BLACK,
            text_color: Color::BLACK,
            scale_factor: 1.0,
        },
        cursor,
    );
    renderer.screenshot(Size::new(WIDTH, HEIGHT), 1.0, Color::WHITE)
}

/// The colour as 8-bit RGB
fn rgb8(colour: impl Into<Color>) -> [u8; 3] {
    let [r, g, b, _] = colour.into().into_rgba8();
    [r, g, b]
}

/// The pixel at `at`
fn pixel(pixels: &[u8], at: Point) -> [u8; 3] {
    let i = 4 * (at.y as usize * WIDTH as usize + at.x as usize);
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// Whether `a` and `b` are within `tolerance` in every channel
fn near(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|c| a[c].abs_diff(b[c]) <= tolerance)
}

/// The number of pixels of `colour` (±8) in the columns from `from` to `to`
fn count(pixels: &[u8], from: u32, to: u32, colour: [u8; 3]) -> usize {
    (0..HEIGHT)
        .flat_map(|y| (from..to).map(move |x| Point::new(x as f32, y as f32)))
        .filter(|&at| near(pixel(pixels, at), colour, 8))
        .count()
}

/// The focus ring on an unchosen part is drawn in the theme's accent text
/// colour, as the default styles draw it, and not in the text colour the
/// style gives the chosen part: here white, to go on the chosen part's own
/// dark fill, which would leave the ring near-invisible on the light control
#[test]
fn the_focus_ring_is_the_themes_colour_not_the_chosen_parts_text() {
    for theme in [crate::Theme::light(), crate::Theme::dark()] {
        let style = control_with(|_, appearance| {
            appearance.active.background =
                Some(Background::Color(Color::from_rgb8(0x20, 0x20, 0x60)));
            appearance.active.text_color = Color::WHITE;
        });
        // The second part is chosen; focus goes to the first
        let pixels = render(control(&model(1), style), &theme, Then::Focus);
        let ring = rgb8(theme.cosmic().accent_text_color());
        let half = WIDTH / 2 - 4;
        assert!(
            count(&pixels, 0, half, ring) > 100,
            "the first part has a ring in the accent text colour {ring:?}"
        );
    }
}

/// `top`, of opacity `alpha`, laid over the opaque `under`
fn over(top: Color, under: Color) -> [u8; 3] {
    let mix = |t: f32, u: f32| t * top.a + u * (1.0 - top.a);
    rgb8(Color::from_rgb(
        mix(top.r, under.r),
        mix(top.g, under.g),
        mix(top.b, under.b),
    ))
}

/// A hovered or pressed part lays the style's wash over its own fill: the
/// chosen part's, and an unchosen part's when the style gives it one
#[test]
fn hover_and_press_lay_the_wash_over_the_parts_own_fill() {
    let chosen = Color::from_rgb8(0xc0, 0x10, 0x10);
    let unchosen = Color::from_rgb8(0x10, 0x30, 0xc0);
    let style = || {
        control_with(move |_, appearance| {
            appearance.active.background = Some(Background::Color(chosen));
            appearance.inactive.background = Some(Background::Color(unchosen));
        })
    };
    for theme in [crate::Theme::light(), crate::Theme::dark()] {
        let defaults = theme.horizontal(&Style::Control);
        let wash = |status: super::ItemStatusAppearance| match status.background {
            Some(Background::Color(colour)) => colour,
            _ => panic!("the Control style has a plain wash"),
        };
        let (hover, press) = (wash(defaults.hover), wash(defaults.pressed));

        // The first part is chosen
        let model = model(0);
        let at_rest = render(control(&model, style()), &theme, Then::Rest);
        assert_eq!(pixel(&at_rest, IN_FIRST), rgb8(chosen), "the chosen fill");
        assert_eq!(
            pixel(&at_rest, IN_SECOND),
            rgb8(unchosen),
            "the unchosen fill"
        );

        for (then, at, wash, own, part) in [
            (
                Then::Hover(IN_FIRST),
                IN_FIRST,
                hover,
                chosen,
                "hovered chosen",
            ),
            (
                Then::Hover(IN_SECOND),
                IN_SECOND,
                hover,
                unchosen,
                "hovered unchosen",
            ),
            (
                Then::Press(IN_FIRST),
                IN_FIRST,
                press,
                chosen,
                "pressed chosen",
            ),
            (
                Then::Press(IN_SECOND),
                IN_SECOND,
                press,
                unchosen,
                "pressed unchosen",
            ),
        ] {
            let pixels = render(control(&model, style()), &theme, then);
            let expected = over(wash, own);
            assert!(
                near(pixel(&pixels, at), expected, 2),
                "the {part} part is {:?}, not the wash over its fill, {expected:?}",
                pixel(&pixels, at)
            );
        }
    }
}

/// A hovered or pressed chosen part keeps the chosen text colour, meant for
/// its own fill, instead of taking the hover's text colour
#[test]
fn the_chosen_part_keeps_its_text_colour_when_hovered_or_pressed() {
    let label = Color::from_rgb8(0xff, 0xe0, 0x00);
    let style = || {
        control_with(move |_, appearance| {
            appearance.active.background =
                Some(Background::Color(Color::from_rgb8(0x20, 0x20, 0x60)));
            appearance.active.text_color = label;
        })
    };
    for theme in [crate::Theme::light(), crate::Theme::dark()] {
        let hover_text = rgb8(theme.horizontal(&Style::Control).hover.text_color);
        // The first part is chosen
        let model = model(0);
        for (then, state) in [
            (Then::Rest, "at rest"),
            (Then::Hover(IN_FIRST), "hovered"),
            (Then::Press(IN_FIRST), "pressed"),
        ] {
            let pixels = render(control(&model, style()), &theme, then);
            let half = WIDTH / 2 - 4;
            assert!(
                count(&pixels, 0, half, rgb8(label)) > 20,
                "the chosen label {state} is in the chosen text colour"
            );
            assert_eq!(
                count(&pixels, 0, half, hover_text),
                0,
                "the chosen part {state} has nothing in the hover's text colour"
            );
        }
    }
}
