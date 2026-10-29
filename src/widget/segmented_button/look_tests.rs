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
