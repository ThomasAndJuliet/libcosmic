//! Use the built-in widgets or create your own.
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/iced-rs/iced/9ab6923e943f784985e9ef9ca28b10278297225d/docs/logo.svg"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
pub use iced_renderer as renderer;
pub use iced_renderer::core;
pub use iced_renderer::graphics;

pub use core::widget::Id;

pub(crate) const DRAG_DEADBAND_DISTANCE: f32 = 10.0;

mod action;
mod mouse_area;
mod pin;
mod responsive;
mod stack;
mod themer;

pub mod button;
pub mod checkbox;
pub mod column;
pub mod combo_box;
pub mod container;
pub mod float;
pub mod grid;
pub mod keyed;
pub mod list;
pub mod overlay;
pub mod pane_grid;
pub mod pick_list;
pub mod progress_bar;
pub mod radio;
pub mod row;
pub mod rule;
pub mod scrollable;
pub mod sensor;
pub mod slider;
pub mod space;
pub mod table;
pub mod text;
pub mod text_editor;
pub mod text_input;
pub mod toggler;
pub mod tooltip;
pub mod vertical_slider;

mod helpers;

pub use helpers::*;

#[cfg(feature = "lazy")]
mod lazy;

#[cfg(feature = "lazy")]
pub use crate::lazy::helpers::*;

#[doc(no_inline)]
pub use button::Button;
#[doc(no_inline)]
pub use checkbox::Checkbox;
#[doc(no_inline)]
pub use column::Column;
#[doc(no_inline)]
pub use combo_box::ComboBox;
#[doc(no_inline)]
pub use container::Container;
#[doc(no_inline)]
pub use float::Float;
#[doc(no_inline)]
pub use grid::Grid;
#[doc(no_inline)]
pub use list::List;
#[doc(no_inline)]
pub use mouse_area::MouseArea;
#[doc(no_inline)]
pub use pane_grid::PaneGrid;
#[doc(no_inline)]
pub use pick_list::PickList;
#[doc(no_inline)]
pub use pin::Pin;
#[doc(no_inline)]
pub use progress_bar::ProgressBar;
#[doc(no_inline)]
pub use radio::Radio;
#[doc(no_inline)]
pub use responsive::Responsive;
#[doc(no_inline)]
pub use row::Row;
#[doc(no_inline)]
pub use rule::Rule;
#[doc(no_inline)]
pub use scrollable::Scrollable;
#[doc(no_inline)]
pub use sensor::Sensor;
#[doc(no_inline)]
pub use slider::Slider;
#[doc(no_inline)]
pub use space::Space;
#[doc(no_inline)]
pub use stack::Stack;
#[doc(no_inline)]
pub use text::Text;
#[doc(no_inline)]
pub use text_editor::TextEditor;
#[doc(no_inline)]
pub use text_input::TextInput;
#[doc(no_inline)]
pub use themer::Themer;
#[doc(no_inline)]
pub use toggler::Toggler;
#[doc(no_inline)]
pub use tooltip::Tooltip;
#[doc(no_inline)]
pub use vertical_slider::VerticalSlider;

#[cfg(feature = "wgpu")]
pub mod shader;

#[cfg(feature = "wgpu")]
#[doc(no_inline)]
pub use shader::Shader;

#[cfg(feature = "svg")]
pub mod svg;

#[cfg(feature = "svg")]
#[doc(no_inline)]
pub use svg::Svg;

#[cfg(feature = "image")]
pub mod image;

#[cfg(feature = "image")]
#[doc(no_inline)]
pub use image::Image;

#[cfg(feature = "canvas")]
pub mod canvas;

#[cfg(feature = "canvas")]
#[doc(no_inline)]
pub use canvas::Canvas;

#[cfg(feature = "qr_code")]
pub mod qr_code;

#[cfg(feature = "qr_code")]
#[doc(no_inline)]
pub use qr_code::QRCode;

#[cfg(feature = "markdown")]
pub mod markdown;

pub use crate::core::theme::{self, Theme};
pub use action::Action;
pub use renderer::Renderer;

#[cfg(test)]
pub(crate) mod drop_target {
    //! A widget that takes drops over its own bounds, for the wrappers'
    //! tests: a wrapper must leave its content's drop targets where the
    //! content is laid out

    use crate::core::clipboard::DndDestinationRectangles;
    use crate::core::widget::Tree;
    use crate::core::{Element, Layout, Length, Rectangle, Size, Widget};
    use crate::core::{layout, mouse, renderer};

    pub struct Target;

    impl<Message, Theme> Widget<Message, Theme, ()> for Target {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fixed(40.0), Length::Fixed(20.0))
        }

        fn layout(
            &mut self,
            _tree: &mut Tree,
            _renderer: &(),
            _limits: &layout::Limits,
        ) -> layout::Node {
            layout::Node::new(Size::new(40.0, 20.0))
                .move_to(crate::core::Point::new(7.0, 3.0))
        }

        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut (),
            _theme: &Theme,
            _style: &renderer::Style,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
        }

        fn drag_destinations(
            &self,
            _state: &Tree,
            layout: Layout<'_>,
            _renderer: &(),
            dnd_rectangles: &mut DndDestinationRectangles,
        ) {
            let bounds = layout.bounds();
            dnd_rectangles.push(dnd::DndDestinationRectangle {
                id: 1,
                rectangle: dnd::Rectangle {
                    x: f64::from(bounds.x),
                    y: f64::from(bounds.y),
                    width: f64::from(bounds.width),
                    height: f64::from(bounds.height),
                },
                mime_types: vec!["x-test/target".into()],
                actions: dnd::DndAction::Move,
                preferred: dnd::DndAction::Move,
            });
        }
    }

    impl<Message, Theme> From<Target> for Element<'_, Message, Theme, ()> {
        fn from(target: Target) -> Self {
            Element::new(target)
        }
    }

    /// The drop targets `element` registers, laid out at the origin
    pub fn targets<Message, Theme>(
        mut element: Element<'_, Message, Theme, ()>,
    ) -> Vec<(f64, f64, f64, f64)> {
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
        );
        let mut rectangles = DndDestinationRectangles::new();
        element.as_widget().drag_destinations(
            &tree,
            Layout::new(&node),
            &(),
            &mut rectangles,
        );
        rectangles
            .into_rectangles()
            .into_iter()
            .map(|r| {
                (
                    r.rectangle.x,
                    r.rectangle.y,
                    r.rectangle.width,
                    r.rectangle.height,
                )
            })
            .collect()
    }
}
