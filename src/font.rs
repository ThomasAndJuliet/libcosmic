// Copyright 2022 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! Select preferred fonts.

pub use iced::Font;
use iced_core::font::Weight;

#[inline]
pub fn default() -> Font {
    Font::from(crate::config::interface_font())
}

#[inline]
pub fn light() -> Font {
    Font {
        weight: Weight::Light,
        ..default()
    }
}

#[inline]
pub fn semibold() -> Font {
    Font {
        weight: Weight::Semibold,
        ..default()
    }
}

#[inline]
pub fn bold() -> Font {
    Font {
        weight: Weight::Bold,
        ..default()
    }
}

#[inline]
pub fn mono() -> Font {
    Font::from(crate::config::monospace_font())
}

/// All headless comparison fixtures share the same interface faces before
/// shaping. Loading fonts midway through another test's before/after pair
/// changes fallback selection and invalidates the comparison.
#[cfg(test)]
pub(crate) fn load_test_fonts() {
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(|| {
        let mut fonts = iced::advanced::graphics::text::font_system().write().unwrap();
        for font in [
            &include_bytes!("../res/open-sans/OpenSans-Regular.ttf")[..],
            &include_bytes!("../res/open-sans/OpenSans-Semibold.ttf")[..],
        ] {
            fonts.load_font(std::borrow::Cow::Borrowed(font));
        }
    });
}
