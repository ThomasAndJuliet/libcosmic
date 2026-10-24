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
