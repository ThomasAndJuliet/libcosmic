// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use super::{Builder, ButtonClass};
use crate::widget::{icon, row, tooltip};
use crate::{Apply, Element};
use iced_core::font::Weight;
use iced_core::text::LineHeight;
use iced_core::widget::Id;
use iced_core::{Alignment, Length, Padding};
use std::borrow::Cow;

pub type Button<'a, Message> = Builder<'a, Message, Text>;

/// A text button with the destructive style
pub fn destructive<'a, Message>(label: impl Into<Cow<'a, str>>) -> Button<'a, Message> {
    Button::new(Text::new())
        .label(label)
        .class(ButtonClass::Destructive)
}

/// A text button with the suggested style
pub fn suggested<'a, Message>(label: impl Into<Cow<'a, str>>) -> Button<'a, Message> {
    Button::new(Text::new())
        .label(label)
        .class(ButtonClass::Suggested)
}

/// A text button with the standard style
pub fn standard<'a, Message>(label: impl Into<Cow<'a, str>>) -> Button<'a, Message> {
    Button::new(Text::new()).label(label)
}

/// A text button with the text style
pub fn text<'a, Message>(label: impl Into<Cow<'a, str>>) -> Button<'a, Message> {
    Button::new(Text::new())
        .label(label)
        .class(ButtonClass::Text)
}

/// The text variant of a button.
pub struct Text {
    pub(super) leading_icon: Option<icon::Handle>,
    pub(super) trailing_icon: Option<icon::Handle>,
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl Text {
    pub const fn new() -> Self {
        Self {
            leading_icon: None,
            trailing_icon: None,
        }
    }
}

impl<Message> Button<'_, Message> {
    pub fn new(text: Text) -> Self {
        let guard = crate::theme::THEME.lock().unwrap();
        let theme = guard.cosmic();
        Self {
            id: Id::unique(),
            label: Cow::Borrowed(""),
            #[cfg(feature = "a11y")]
            name: Cow::Borrowed(""),
            #[cfg(feature = "a11y")]
            description: Cow::Borrowed(""),
            tooltip: Cow::Borrowed(""),
            on_press: None,
            width: Length::Shrink,
            height: Length::Fixed(theme.space_l().into()),
            padding: Padding::from([0, theme.space_s()]),
            spacing: theme.space_xxxs(),
            icon_size: 16,
            line_height: 20,
            font_size: 14,
            font_weight: Weight::Normal,
            class: ButtonClass::Standard,
            variant: text,
        }
    }

    pub fn leading_icon(mut self, icon: impl Into<icon::Handle>) -> Self {
        self.variant.leading_icon = Some(icon.into());
        self
    }

    pub fn trailing_icon(mut self, icon: impl Into<icon::Handle>) -> Self {
        self.variant.trailing_icon = Some(icon.into());
        self
    }
}

impl<'a, Message: Clone + 'static> From<Button<'a, Message>> for Element<'a, Message> {
    fn from(mut builder: Button<'a, Message>) -> Element<'a, Message> {
        let trailing_icon = builder
            .variant
            .trailing_icon
            .map(crate::widget::icon::Handle::icon);

        let leading_icon = builder
            .variant
            .leading_icon
            .map(crate::widget::icon::Handle::icon);

        let label: Option<Element<'_, _>> = (!builder.label.is_empty()).then(|| {
            let font = crate::font::Font {
                weight: builder.font_weight,
                ..crate::font::default()
            };

            // TODO: Avoid allocation
            crate::widget::text(builder.label.to_string())
                .size(builder.font_size)
                .line_height(LineHeight::Absolute(builder.line_height.into()))
                .font(font)
                .into()
        });

        let mut button: super::Button<'a, Message> = row::with_capacity(3)
            // Optional icon to place before label.
            .push_maybe(leading_icon)
            // Optional label between icons.
            .push_maybe(label)
            // Optional icon to place behind the label.
            .push_maybe(trailing_icon)
            .padding(builder.padding)
            .spacing(builder.spacing)
            .align_y(Alignment::Center)
            .apply(super::custom)
            // The button takes the size, so a fixed or Fill width is honoured,
            // and its label and icons sit in the middle of it.
            .width(builder.width)
            .height(builder.height)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .padding(0)
            .id(builder.id)
            .on_press_maybe(builder.on_press.take())
            .class(builder.class);

        #[cfg(feature = "a11y")]
        {
            if !builder.label.is_empty() {
                button = button.name(builder.label)
            }

            button = button.description(builder.description);
        }

        if builder.tooltip.is_empty() {
            button.into()
        } else {
            tooltip(
                button,
                crate::widget::text(builder.tooltip)
                    .size(builder.font_size)
                    .font(crate::font::Font {
                        weight: builder.font_weight,
                        ..crate::font::default()
                    }),
                tooltip::Position::Top,
            )
            .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Element;
    use iced_core::layout::{Limits, Node};
    use iced_core::widget::Tree;
    use iced_core::{Length, Rectangle, Size};

    fn laid_out(mut element: Element<'static, ()>, max: Size) -> Node {
        let renderer = iced::futures::executor::block_on(
            <crate::Renderer as iced_core::renderer::Headless>::new(
                crate::font::default(),
                iced_core::Pixels(14.0),
                Some("tiny-skia"),
            ),
        )
        .expect("a headless renderer");
        let mut tree = Tree::new(&element);
        element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &Limits::new(Size::ZERO, max))
    }

    /// The union of the leaves under `node`'s content, in `node`'s frame.
    fn content(node: &Node) -> Rectangle {
        fn walk(node: &Node, at: iced_core::Vector, out: &mut Option<Rectangle>) {
            let bounds = node.bounds() + at;
            if node.children().is_empty() {
                *out = Some(out.map_or(bounds, |r| r.union(&bounds)));
            }
            for child in node.children() {
                walk(child, iced_core::Vector::new(bounds.x, bounds.y), out);
            }
        }
        let mut out = None;
        for child in node.children() {
            walk(child, iced_core::Vector::ZERO, &mut out);
        }
        out.expect("content")
    }

    #[test]
    fn text_buttons_centre_their_content() {
        let icon = || crate::widget::icon::from_name("list-add-symbolic");
        let cases: Vec<(&str, Element<'static, ()>, Option<f32>)> = vec![
            ("shrink", super::standard("Send").into(), None),
            (
                "fixed",
                super::suggested("Send").width(200.0).into(),
                Some(200.0),
            ),
            (
                "fill",
                super::text("Steer").width(Length::Fill).into(),
                Some(300.0),
            ),
            (
                "fixed with icon",
                super::standard("Add")
                    .leading_icon(icon())
                    .width(160.0)
                    .into(),
                Some(160.0),
            ),
            (
                "icon button",
                crate::widget::button::icon(icon())
                    .label("Add")
                    .width(Length::Fill)
                    .into(),
                Some(300.0),
            ),
        ];
        for (name, element, width) in cases {
            let node = laid_out(element, Size::new(300.0, 200.0));
            let button = Rectangle::new(iced_core::Point::ORIGIN, node.size());
            if let Some(width) = width {
                assert_eq!(button.width, width, "{name}: button width");
            }
            let label = content(&node);
            if width.is_none() {
                // As wide as it needs: the size it always had
                assert_eq!(button.width - label.width, 32.0, "{name}: side padding");
                assert_eq!(button.height, 32.0, "{name}: height");
            }
            assert!(
                (label.center_x() - button.center_x()).abs() <= 0.5
                    && (label.center_y() - button.center_y()).abs() <= 0.5,
                "{name}: content {label:?} not centred in {button:?}"
            );
        }

        // A custom button keeps its content at the start unless asked
        let custom = crate::widget::button::custom(crate::widget::text("Row")).width(200.0);
        let node = laid_out(custom.into(), Size::new(300.0, 200.0));
        assert_eq!(content(&node).position(), iced_core::Point::new(5.0, 5.0));
    }
}
