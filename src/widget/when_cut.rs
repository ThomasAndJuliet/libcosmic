// Copyright 2026 the household (ThomasAndJuliet)
// SPDX-License-Identifier: MPL-2.0

//! A tooltip shown only when its text is cut off (household rule 14: any
//! text cut off with an ellipsis shows the full text as a tooltip).
//!
//! [`WhenCut`] wraps an element whose content is text that may be cut off
//! (an ellipsized title), usually a [`super::tooltip::Tooltip`] holding the
//! full text. It lays the content out in the room it is given and, when the
//! content fills that room, once more without a limit across, to learn
//! whether it was cut; its overlays (the tooltip) are shown only if so.
//! Text well short of its room is laid out once, as before; the tooltip's
//! own delay and position apply as they are.

use iced_core::layout::{Limits, Node};
use iced_core::widget::{Operation, Tree, tree};
use iced_core::{Clipboard, Event, Layout, Length, Rectangle, Shell, Size, Vector, Widget, mouse, overlay, renderer};

use crate::{Element, Renderer, Theme};

/// See the module's description.
pub struct WhenCut<'a, Message> {
    content: Element<'a, Message>,
}

/// `content`'s overlays (its tooltip) shown only while its text is cut off
/// in the room it is given.
pub fn when_cut<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> WhenCut<'a, Message> {
    WhenCut { content: content.into() }
}

/// Whether the content was cut off at its last layout.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct State {
    cut: bool,
}

impl<Message> WhenCut<'_, Message> {
    /// Whether `tree`'s content was cut off at its last layout (tests).
    pub fn is_cut(tree: &Tree) -> bool {
        tree.state.downcast_ref::<State>().cut
    }
}

impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for WhenCut<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let node = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let room = limits.max().width;
        // Only content that (nearly) fills its room can have been cut: an
        // ellipsized line ends short of the room by less than an ellipsis
        // and a glyph, taken here as two of its lines' heights
        let near = (2.0 * node.size().height).max(24.0);
        let cut = if room.is_finite() && node.size().width >= room - near {
            let free = Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
            let natural = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, &free);
            natural.size().width > node.size().width + 0.5
        } else {
            false
        };
        tree.state.downcast_mut::<State>().cut = cut;
        if cut {
            // The content's state as laid out in its room again
            self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
        } else {
            node
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if !tree.state.downcast_ref::<State>().cut {
            return None;
        }
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<WhenCut<'a, Message>> for Element<'a, Message> {
    fn from(widget: WhenCut<'a, Message>) -> Self {
        Element::new(widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use iced_core::{Color, Point, clipboard};
    use iced_runtime::user_interface::{Cache, UserInterface};

    #[derive(Clone, Debug)]
    struct Nothing;

    const SIZE: Size = Size::new(400.0, 200.0);

    fn renderer() -> Renderer {
        iced::futures::executor::block_on(<Renderer as Headless>::new(iced::Font::default(), iced::Pixels(14.0), Some("tiny-skia")))
            .expect("a headless renderer")
    }

    /// `text` ellipsized in a box `width` wide at the top left, with the
    /// full text as its tooltip, below it
    fn page(text: &'static str, width: f32) -> Element<'static, Nothing> {
        let label = crate::widget::text(text).ellipsize(iced_core::text::Ellipsize::End(iced_core::text::EllipsizeHeightLimit::Lines(1)));
        let tip = super::super::tooltip::tooltip(label, crate::widget::text("The full text of the title"), super::super::tooltip::Position::Bottom);
        crate::widget::container(when_cut(tip)).width(Length::Fixed(width)).into()
    }

    /// The frame with the pointer at `at` (two frames: it moves there, then
    /// a redraw opens the tooltip)
    fn frame(text: &'static str, width: f32, at: Option<Point>) -> Vec<u8> {
        let mut renderer = renderer();
        let mut cache = Cache::default();
        let cursor = at.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
        let events = [
            at.map(|position| Event::Mouse(mouse::Event::CursorMoved { position })),
            Some(Event::Window(iced_core::window::Event::RedrawRequested(std::time::Instant::now()))),
        ];
        for event in events.into_iter().flatten() {
            let mut ui = UserInterface::build(page(text, width), SIZE, std::mem::take(&mut cache), &mut renderer);
            let _ = ui.update(&[event], cursor, &mut renderer, &mut clipboard::Null, &mut Vec::new());
            cache = ui.into_cache();
        }
        let mut ui = UserInterface::build(page(text, width), SIZE, cache, &mut renderer);
        let _ = ui.update(&[], cursor, &mut renderer, &mut clipboard::Null, &mut Vec::new());
        ui.draw(&mut renderer, &crate::Theme::light(), &renderer::Style { icon_color: Color::BLACK, text_color: Color::BLACK, scale_factor: 1.0 }, cursor);
        renderer.screenshot(Size::new(SIZE.width as u32, SIZE.height as u32), 1.0, Color::WHITE)
    }

    fn differing(a: &[u8], b: &[u8]) -> usize {
        a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count()
    }

    const LONG: &str = "A title far too long for the room it is given here";
    const OVER: Point = Point::new(20.0, 8.0);

    #[test]
    fn cut_text_shows_its_tooltip() {
        let away = frame(LONG, 120.0, None);
        let over = frame(LONG, 120.0, Some(OVER));
        assert!(differing(&away, &over) > 100, "the full text is shown below the cut title");
    }

    #[test]
    fn text_that_fits_shows_none() {
        let away = frame("Short", 120.0, None);
        let over = frame("Short", 120.0, Some(OVER));
        assert_eq!(differing(&away, &over), 0, "no tooltip over text that is not cut");
        // The same long text in room enough
        let away = frame(LONG, 390.0, None);
        let over = frame(LONG, 390.0, Some(OVER));
        assert_eq!(differing(&away, &over), 0, "no tooltip when the room is wide enough");
    }

    #[test]
    fn the_cut_text_is_drawn_as_without_the_wrapper() {
        let plain = |text: &'static str| -> Element<'static, Nothing> {
            let label = crate::widget::text(text).ellipsize(iced_core::text::Ellipsize::End(iced_core::text::EllipsizeHeightLimit::Lines(1)));
            crate::widget::container(label).width(Length::Fixed(120.0)).into()
        };
        let mut renderer = renderer();
        let mut draw = |element: Element<'static, Nothing>| {
            let mut ui = UserInterface::build(element, SIZE, Cache::default(), &mut renderer);
            let _ = ui.update(&[], mouse::Cursor::Unavailable, &mut renderer, &mut clipboard::Null, &mut Vec::new());
            ui.draw(&mut renderer, &crate::Theme::light(), &renderer::Style { icon_color: Color::BLACK, text_color: Color::BLACK, scale_factor: 1.0 }, mouse::Cursor::Unavailable);
            renderer.screenshot(Size::new(SIZE.width as u32, SIZE.height as u32), 1.0, Color::WHITE)
        };
        let wrapped = draw(page(LONG, 120.0));
        let bare = draw(plain(LONG));
        assert_eq!(differing(&wrapped, &bare), 0, "laid out again in its room: the same ellipsized line");
    }
}
