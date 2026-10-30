// Copyright 2022 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! Implementation details for the horizontal layout of a segmented button.

use super::model::{Model, Selectable};
use super::style::StyleSheet;
use super::widget::{ItemBounds, LocalState, SegmentedButton, SegmentedVariant};

use iced::{Length, Rectangle, Size};
use iced_core::layout;
use iced_core::text::Renderer;

/// Horizontal [`SegmentedButton`].
pub type HorizontalSegmentedButton<'a, SelectionMode, Message> =
    SegmentedButton<'a, Horizontal, SelectionMode, Message>;

/// A type marker defining the horizontal variant of a [`SegmentedButton`].
pub struct Horizontal;

/// Horizontal implementation of the [`SegmentedButton`].
///
/// For details on the model, see the [`segmented_button`](super) module for more details.
pub fn horizontal<SelectionMode: Default, Message: Clone + 'static>(
    model: &Model<SelectionMode>,
) -> SegmentedButton<'_, Horizontal, SelectionMode, Message>
where
    Model<SelectionMode>: Selectable,
{
    SegmentedButton::new(model)
}

impl<SelectionMode, Message: Clone + 'static> SegmentedVariant
    for SegmentedButton<'_, Horizontal, SelectionMode, Message>
where
    Model<SelectionMode>: Selectable,
    SelectionMode: Default,
{
    const VERTICAL: bool = false;

    fn variant_appearance(
        theme: &crate::Theme,
        style: &crate::theme::SegmentedButton,
    ) -> super::Appearance {
        theme.horizontal(style)
    }

    #[allow(clippy::cast_precision_loss)]
    fn variant_bounds<'b>(
        &'b self,
        state: &'b LocalState,
        mut bounds: Rectangle,
    ) -> Box<dyn Iterator<Item = ItemBounds> + 'b> {
        let num = state.buttons_visible;
        let spacing = f32::from(self.spacing);
        let mut homogenous_width = 0.0;

        if Length::Shrink != self.width || state.collapsed {
            let mut width_offset = 0.0;
            if state.collapsed {
                bounds.x += f32::from(self.button_height);
                width_offset = f32::from(self.button_height) * 2.0;
            }

            homogenous_width = ((num as f32).mul_add(-spacing, bounds.width - width_offset)
                + spacing)
                / num as f32;
        }

        Box::new(
            self.model
                .order
                .iter()
                .copied()
                .enumerate()
                .skip(state.buttons_offset)
                .take(state.buttons_visible)
                .flat_map(move |(nth, key)| {
                    let mut layout_bounds = bounds;

                    let layout_size = &state.internal_layout[nth].0;

                    if !state.collapsed && Length::Shrink == self.width {
                        layout_bounds.width = layout_size.width;
                    } else {
                        layout_bounds.width = homogenous_width;
                    }

                    bounds.x += layout_bounds.width + spacing;

                    let button_bounds = ItemBounds::Button(key, layout_bounds);
                    let mut divider = None;

                    // The dividers the control was built with, whatever its
                    // style: a custom style keeps them as `Control` draws them.
                    if self.dividers && nth + 1 < num {
                        divider = Some(ItemBounds::Divider(
                            Rectangle {
                                width: 1.0,
                                ..bounds
                            },
                            true,
                        ));

                        bounds.x += 1.0;
                    }

                    std::iter::once(button_bounds).chain(divider)
                }),
        )
    }

    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_sign_loss)]
    fn variant_layout(
        &self,
        state: &mut LocalState,
        renderer: &crate::Renderer,
        limits: &layout::Limits,
    ) -> Size {
        state.internal_layout.clear();
        let num = self.model.order.len();
        let spacing = f32::from(self.spacing);
        let size;

        let mut reduce_button_offset = false;
        if state.known_length != num {
            if state.known_length > num {
                state.buttons_offset -= state.buttons_offset.min(state.known_length - num);
            } else {
                reduce_button_offset = true;
            }

            state.known_length = num;
        }

        if let Length::Shrink = self.width {
            // Buttons will be rendered at their smallest widths possible.
            let max_height = self.max_button_dimensions(state, renderer).1;

            // Get the max available width for placing buttons into.
            let max_size = limits.height(Length::Fixed(max_height)).resolve(
                Length::Fill,
                max_height,
                Size::new(limits.max().width, max_height),
            );

            let mut visible_width = 0.0;
            state.buttons_visible = 0;

            for (button_size, _actual_size) in &state.internal_layout {
                visible_width += button_size.width;

                if max_size.width - spacing >= visible_width {
                    state.buttons_visible += 1;
                } else {
                    visible_width = max_size.width - max_height;
                    break;
                }

                visible_width += spacing;
            }

            visible_width -= spacing;

            state.collapsed = num > 1 && state.buttons_visible != num;

            size = limits
                .height(Length::Fixed(max_height))
                .min_width(visible_width)
                .min();
        } else {
            // Buttons will be rendered with equal widths.
            state.buttons_visible = self.model.items.len();

            let mut width = 0.0f32;
            let font = renderer.default_font();

            for key in self.model.order.iter().copied() {
                let (button_width, button_height) = self.button_dimensions(state, font, key);

                state.internal_layout.push((
                    Size::new(button_width, button_height),
                    Size::new(
                        button_width
                            - f32::from(self.button_padding[0])
                            - f32::from(self.button_padding[2]),
                        button_height,
                    ),
                ));

                width = width.max(button_width);
            }

            let height = f32::from(self.button_height);

            size = limits.height(Length::Fixed(height)).max();

            let actual_width = size.width as usize;
            let minimum_width = state.buttons_visible * self.minimum_button_width as usize;
            state.collapsed = actual_width < minimum_width;

            if state.collapsed {
                state.buttons_visible =
                    (actual_width / self.minimum_button_width as usize).min(state.buttons_visible);
            }
        }

        if !state.collapsed {
            state.buttons_offset = 0;
        } else if reduce_button_offset {
            state.buttons_offset = num - state.buttons_visible;
        }

        // Resize paragraph bounds so that text ellipsis can take effect.
        if !matches!(self.width, Length::Shrink) || state.collapsed {
            let num = state.buttons_visible.max(1) as f32;
            let spacing = f32::from(self.spacing);
            let mut width_offset = 0.0;
            if state.collapsed {
                width_offset = f32::from(self.button_height) * 2.0;
            }
            let button_width = ((num).mul_add(-spacing, size.width - width_offset) + spacing) / num;
            self.resize_paragraphs(state, button_width);
        }

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::segmented_button::SingleSelectModel;
    use iced::advanced::renderer::Headless;
    use iced_core::Color;
    use iced_core::mouse::Cursor;
    use iced_core::renderer::Style;
    use iced_runtime::user_interface::{Cache, UserInterface};
    use std::borrow::Cow;

    const WIDTH: u32 = 400;
    const HEIGHT: u32 = 48;

    #[derive(Clone)]
    enum Message {}

    /// Two tabs of the same wide word, drawn in `theme`; returns the RGBA pixels
    fn drawn(model: &SingleSelectModel, theme: &crate::Theme) -> Vec<u8> {
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
        let bar: crate::Element<'_, Message> = horizontal::<_, Message>(model)
            .width(Length::Fixed(WIDTH as f32))
            .button_height(HEIGHT as u16)
            .into();
        let mut ui = UserInterface::build(
            bar,
            Size::new(WIDTH as f32, HEIGHT as f32),
            Cache::default(),
            &mut renderer,
        );
        ui.draw(
            &mut renderer,
            theme,
            &Style {
                icon_color: Color::BLACK,
                text_color: Color::BLACK,
                scale_factor: 1.0,
            },
            Cursor::Unavailable,
        );
        renderer.screenshot(Size::new(WIDTH, HEIGHT), 1.0, Color::TRANSPARENT)
    }

    /// Pixels of `colour` (red here) in the columns from `from` to `to`
    fn red_in(pixels: &[u8], from: u32, to: u32) -> usize {
        let mut count = 0;
        for y in 0..HEIGHT {
            for x in from..to {
                let at = ((y * WIDTH + x) * 4) as usize;
                let [r, g, b, a] = [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]];
                if a > 200 && r > 180 && g < 90 && b < 90 {
                    count += 1;
                }
            }
        }
        count
    }

    /// An item's own title colour is drawn on its title, only on it, and an item
    /// without one keeps its state's colour; in light and dark, for the
    /// active tab and the inactive one alike
    #[test]
    fn an_items_own_title_colour_is_drawn_on_its_title_alone() {
        for theme in [crate::Theme::light(), crate::Theme::dark()] {
            let mut ids = Vec::new();
            let mut model = SingleSelectModel::builder()
                .insert(|b| b.text("Mmmmmmmm").with_id(|id| ids.push(id)))
                .insert(|b| b.text("Mmmmmmmm").with_id(|id| ids.push(id)))
                .build();
            model.activate(ids[0]);
            let none = drawn(&model, &theme);
            assert_eq!(
                red_in(&none, 0, WIDTH),
                0,
                "no red without a colour of its own"
            );

            // The inactive second tab: red there and nowhere else
            model.text_color_set(ids[1], Some(Color::from_rgb(1.0, 0.0, 0.0)));
            let second = drawn(&model, &theme);
            assert!(
                red_in(&second, WIDTH / 2, WIDTH) > 20,
                "the second title is red"
            );
            assert_eq!(red_in(&second, 0, WIDTH / 2), 0, "the first title is not");

            // The active first tab keeps its own, too
            model.text_color_set(ids[0], Some(Color::from_rgb(1.0, 0.0, 0.0)));
            let both = drawn(&model, &theme);
            assert!(red_in(&both, 0, WIDTH / 2) > 20, "the active title is red");

            // Cleared: back to the state's colour
            model.text_color_set(ids[0], None);
            model.text_color_set(ids[1], None);
            assert_eq!(
                drawn(&model, &theme),
                none,
                "a cleared colour draws as before"
            );
        }
    }
}
