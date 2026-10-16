// Copyright 2026 Thomas Wright <tom.tdw@gmail.com>
// SPDX-License-Identifier: MPL-2.0

//! A settings row with its control centred on the first lines of its text.

use crate::{Element, Renderer, Theme};
use iced_core::layout::{self, Layout, Node};
use iced_core::widget::{Id, Operation, Tree, text};
use iced_core::{Clipboard, Event, Length, Point, Rectangle, Shell, Size, Vector, Widget};
use iced_core::{mouse, overlay, renderer};

type Paragraph = <Renderer as iced_core::text::Renderer>::Paragraph;

/// Centres the control of a settings row (its last child) on the first
/// `lines` lines of the row's text, rather than on the whole row.
///
/// The text is the child before the control unless [`ControlLines::text_child`]
/// says otherwise. A text made of several parts, such as a label above a
/// description, counts the label's lines first. A row with fewer lines than
/// `lines` has its control centred on all of them, and a longer text runs
/// on below without moving the control further down. A control taller than
/// those lines stays at the top of the row, and one that a flex row has
/// wrapped below the text is left where it is.
pub fn control_lines<'a, Message>(
    row: impl Into<Element<'a, Message>>,
    lines: usize,
) -> ControlLines<'a, Message> {
    ControlLines {
        content: row.into(),
        lines,
        text: None,
    }
}

/// A settings row with its control centred on the first lines of its text.
///
/// Made by [`control_lines`].
#[must_use]
pub struct ControlLines<'a, Message> {
    content: Element<'a, Message>,
    lines: usize,
    text: Option<usize>,
}

impl<Message> ControlLines<'_, Message> {
    /// The index of the row's text among its children, where it is not the
    /// child just before the control (for example, with a preview between
    /// the two).
    pub fn text_child(mut self, index: usize) -> Self {
        self.text = Some(index);
        self
    }
}

/// The line height of `tree` and `node` if they are a text widget's, and
/// the number of lines laid out.
fn text_lines(tree: &Tree, node: &Node) -> Option<(f32, usize)> {
    if tree.tag != iced_core::widget::tree::Tag::of::<text::State<Paragraph>>() {
        return None;
    }
    let paragraph = tree
        .state
        .downcast_ref::<text::State<Paragraph>>()
        .paragraph
        .raw();
    let line = {
        use iced_core::text::Paragraph as _;
        paragraph.line_height().to_absolute(paragraph.size()).0
    };
    (line > 0.0).then(|| (line, ((node.size().height / line).round() as usize).max(1)))
}

/// The height, from the top of the text, of its first `lines` lines.
fn first_lines(tree: &Tree, node: &Node, lines: usize) -> f32 {
    let height = node.size().height;
    if let Some((line, count)) = text_lines(tree, node) {
        return (line * count.min(lines) as f32).min(height);
    }
    // A column of parts: count each part's lines, top to bottom
    let mut left = lines;
    let mut end = height;
    for (tree, node) in tree.children.iter().zip(node.children()) {
        if left == 0 {
            break;
        }
        let top = node.bounds().y;
        let (line, count) = text_lines(tree, node).unwrap_or((node.size().height, 1));
        let taken = count.min(left);
        end = top + line * taken as f32;
        left -= taken;
    }
    end.min(height)
}

/// Moves the control in the laid-out `node` of a row (with its `tree`).
fn align(node: Node, tree: &Tree, lines: usize, text: Option<usize>) -> Node {
    let children = node.children();
    let Some(control) = children.len().checked_sub(1).filter(|&c| c > 0) else {
        return node;
    };
    let text = text.unwrap_or(control - 1);
    let (Some(text_node), Some(text_tree)) = (children.get(text), tree.children.get(text)) else {
        return node;
    };
    if text == control {
        return node;
    }
    let text_bounds = text_node.bounds();
    let control_bounds = children[control].bounds();
    // Wrapped below the text, in a flex row too narrow for both
    if control_bounds.y >= text_bounds.y + text_bounds.height {
        return node;
    }
    let first = first_lines(text_tree, text_node, lines);
    let y = (text_bounds.y + (first - control_bounds.height) / 2.0).max(0.0);
    let mut children = children.to_vec();
    children[control].move_to_mut(Point::new(control_bounds.x, y));
    let bottom = children
        .iter()
        .map(|c| c.bounds().y + c.bounds().height)
        .fold(node.size().height, f32::max);
    Node::with_children(Size::new(node.size().width, bottom), children)
        .move_to(node.bounds().position())
}

impl<Message> Widget<Message, Theme, Renderer> for ControlLines<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> Node {
        let tree = &mut tree.children[0];
        let node = self.content.as_widget_mut().layout(tree, renderer, limits);
        align(node, tree, self.lines, self.text)
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        state: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout, &state.children[0], cursor)
    }

    fn id(&self) -> Option<Id> {
        self.content.as_widget().id()
    }

    fn set_id(&mut self, id: Id) {
        self.content.as_widget_mut().set_id(id);
    }

    fn drag_destinations(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        dnd_rectangles: &mut iced_core::clipboard::DndDestinationRectangles,
    ) {
        self.content.as_widget().drag_destinations(
            &tree.children[0],
            layout,
            renderer,
            dnd_rectangles,
        );
    }
}

impl<'a, Message: 'a> From<ControlLines<'a, Message>> for Element<'a, Message> {
    fn from(row: ControlLines<'a, Message>) -> Self {
        Element::new(row)
    }
}

#[cfg(test)]
mod tests {
    use crate::widget::{settings, space};
    use crate::{Element, Renderer};
    use iced::advanced::renderer::Headless;
    use iced_core::layout::{Limits, Node};
    use iced_core::widget::Tree;
    use iced_core::{Font, Pixels, Size};

    const LONG: &str = "A description long enough to wrap onto several lines \
        in a narrow settings row, so that the row grows well past the \
        height of its control";

    fn laid_out(mut element: Element<'static, ()>, width: f32) -> Node {
        let renderer = iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer");
        let mut tree = Tree::new(&element);
        element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &Limits::new(Size::ZERO, Size::new(width, 2000.0)),
        )
    }

    #[test]
    fn settings_flex_row_centres_its_control() {
        let row = settings::flex_item_row(vec![
            space().width(100.0).height(80.0).into(),
            space().width(50.0).height(20.0).into(),
        ]);
        let node = laid_out(row.into(), 400.0);
        assert_eq!(node.children()[1].bounds().y, 30.0);
    }

    #[test]
    fn settings_control_lines_centres_on_the_first_lines() {
        let control = || space().width(40.0).height(20.0);
        let item = || settings::item::builder("Title").description(LONG);
        let lined = laid_out(item().control_lines(2, control()).into(), 300.0);
        let text = &lined.children()[0];
        let description = text.children()[1].bounds();
        assert!(
            description.height >= 34.0,
            "the description wraps: {description:?}"
        );
        // The title and the description's first 17 px line
        let expected = (text.bounds().y + description.y + 17.0 - 20.0) / 2.0;
        assert_eq!(lined.children()[1].bounds().y, expected);
        let row = laid_out(item().control(control()).into(), 300.0);
        assert!(
            row.children()[1].bounds().y > expected,
            "a row centres it lower"
        );
        assert_eq!(lined.size(), row.size());

        // Fewer lines than asked for: centred on all of them, as a row does
        let short = || settings::item::builder("Title").description("Short");
        let lined = laid_out(short().control_lines(3, control()).into(), 300.0);
        let row = laid_out(short().control(control()).into(), 300.0);
        assert_eq!(lined.children()[1].bounds(), row.children()[1].bounds());

        // A control that has wrapped under its text stays there
        let wide = || space().width(250.0).height(20.0);
        let lined = laid_out(item().flex_control_lines(2, wide()).into(), 300.0);
        let flex = laid_out(item().flex_control(wide()).into(), 300.0);
        assert!(lined.children()[1].bounds().y >= lined.children()[0].bounds().height);
        assert_eq!(lined.children()[1].bounds(), flex.children()[1].bounds());
    }
}
