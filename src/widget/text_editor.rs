// Copyright 2025 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

//! A multi-line text editor

pub use iced_widget::text_editor::{
    Action, Binding, Catalog, Content, Cursor, Edit, Id, KeyPress, Line, LineEnding, Motion,
    Position, Selection, State, Status, Style, StyleFn,
};

use crate::widget::text_context_menu::{TextAction, TextKind, TextMenuContext, TextMenuTarget};
use iced_core::widget::text::HasSelectableText;

use iced_core::event::Event;
use iced_core::text::highlighter;
use iced_core::widget::Widget;
use iced_core::widget::tree::{self, Tree};
use iced_core::{
    Clipboard, Layout, Length, Rectangle, Shell, Size, Vector, mouse, overlay, renderer, window,
};

type InnerEditor<'a, Message> =
    iced_widget::TextEditor<'a, highlighter::PlainText, Message, crate::Theme, crate::Renderer>;

pub struct TextEditor<'a, Message> {
    inner: InnerEditor<'a, Message>,
    has_context_menu: bool,
    external_text_menu: bool,
    window_id: window::Id,
}

struct EditorWrapperState {
    text_menu_session: crate::widget::text_context_menu::TextMenuSession,
}

impl<'a, Message: Clone + 'static> TextEditor<'a, Message> {
    /// Creates a new [`TextEditor`] from the given [`Content`].
    pub fn new(content: &'a Content<crate::Renderer>) -> Self {
        Self {
            inner: iced_widget::text_editor(content)
                .selected_text_color(|theme: &crate::Theme, _| theme.cosmic().accent.on.into()),
            has_context_menu: true,
            external_text_menu: false,
            window_id: crate::widget::text_context_menu::current_window_id(),
        }
    }

    /// Controls whether the right-click context menu is shown.
    ///
    /// The context menu is enabled by default. Pass `false` to disable it.
    pub fn context_menu(mut self, enabled: bool) -> Self {
        self.has_context_menu = enabled;
        self
    }

    pub fn id(mut self, id: impl Into<iced_core::widget::Id>) -> Self {
        self.inner = self.inner.id(id);
        self
    }

    pub fn placeholder(mut self, placeholder: impl iced_core::text::IntoFragment<'a>) -> Self {
        self.inner = self.inner.placeholder(placeholder);
        self
    }

    pub fn width(mut self, width: impl Into<iced_core::Pixels>) -> Self {
        self.inner = self.inner.width(width);
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.inner = self.inner.height(height);
        self
    }

    pub fn min_height(mut self, min_height: impl Into<iced_core::Pixels>) -> Self {
        self.inner = self.inner.min_height(min_height);
        self
    }

    pub fn max_height(mut self, max_height: impl Into<iced_core::Pixels>) -> Self {
        self.inner = self.inner.max_height(max_height);
        self
    }

    pub fn on_action(mut self, on_edit: impl Fn(Action) -> Message + 'a) -> Self {
        self.inner = self.inner.on_action(on_edit);
        self
    }

    pub fn font(
        mut self,
        font: impl Into<<crate::Renderer as iced_core::text::Renderer>::Font>,
    ) -> Self {
        self.inner = self.inner.font(font);
        self
    }

    pub fn size(mut self, size: impl Into<iced_core::Pixels>) -> Self {
        self.inner = self.inner.size(size);
        self
    }

    /// Sets the foreground drawn over the text selection's fill.
    pub fn selected_text_color(
        mut self,
        color: impl Fn(&crate::Theme, &Style) -> iced_core::Color + 'a,
    ) -> Self {
        self.inner = self.inner.selected_text_color(color);
        self
    }

    /// Sets the fill drawn behind selected text, in place of the style's
    /// `selection`. The fill is drawn over the unselected text, so it should
    /// be opaque.
    pub fn selection_fill(
        mut self,
        color: impl Fn(&crate::Theme, &Style) -> iced_core::Color + 'a,
    ) -> Self {
        self.inner = self.inner.selection_fill(color);
        self
    }

    pub fn line_height(mut self, line_height: impl Into<iced_core::text::LineHeight>) -> Self {
        self.inner = self.inner.line_height(line_height);
        self
    }

    pub fn padding(mut self, padding: impl Into<iced_core::Padding>) -> Self {
        self.inner = self.inner.padding(padding);
        self
    }

    pub fn wrapping(mut self, wrapping: iced_core::text::Wrapping) -> Self {
        self.inner = self.inner.wrapping(wrapping);
        self
    }

    pub fn key_binding(
        mut self,
        key_binding: impl Fn(KeyPress) -> Option<Binding<Message>> + 'a,
    ) -> Self {
        self.inner = self.inner.key_binding(key_binding);
        self
    }

    #[must_use]
    pub fn style(mut self, style: impl Fn(&crate::Theme, Status) -> Style + 'a) -> Self
    where
        <crate::Theme as Catalog>::Class<'a>:
            From<iced_widget::text_editor::StyleFn<'a, crate::Theme>>,
    {
        self.inner = self.inner.style(style);
        self
    }
}

/// Creates a new [`TextEditor`] from the given [`Content`].
pub fn text_editor<'a, Message: Clone + 'static>(
    content: &'a Content<crate::Renderer>,
) -> TextEditor<'a, Message> {
    TextEditor::new(content)
}

fn ew<'x, Message>(
    inner: &'x InnerEditor<'_, Message>,
) -> &'x dyn Widget<Message, crate::Theme, crate::Renderer> {
    inner
}

fn ew_mut<'x, Message>(
    inner: &'x mut InnerEditor<'_, Message>,
) -> &'x mut dyn Widget<Message, crate::Theme, crate::Renderer> {
    inner
}

impl<'a, Message: Clone + 'static> Widget<Message, crate::Theme, crate::Renderer>
    for TextEditor<'a, Message>
{
    fn tag(&self) -> tree::Tag {
        if self.has_context_menu {
            tree::Tag::of::<EditorWrapperState>()
        } else {
            ew::<Message>(&self.inner).tag()
        }
    }

    fn state(&self) -> tree::State {
        if self.has_context_menu {
            tree::State::new(EditorWrapperState {
                text_menu_session: Default::default(),
            })
        } else {
            ew::<Message>(&self.inner).state()
        }
    }

    fn children(&self) -> Vec<Tree> {
        if self.has_context_menu {
            vec![Tree::new(ew::<Message>(&self.inner))]
        } else {
            ew::<Message>(&self.inner).children()
        }
    }

    fn diff(&mut self, tree: &mut Tree) {
        if self.has_context_menu {
            if let Some(child) = tree.children.first_mut() {
                child.diff(
                    &mut self.inner as &mut dyn Widget<Message, crate::Theme, crate::Renderer>,
                );
            }
        } else {
            ew_mut::<Message>(&mut self.inner).diff(tree);
        }
    }

    fn size(&self) -> Size<Length> {
        ew::<Message>(&self.inner).size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &crate::Renderer,
        limits: &iced_core::layout::Limits,
    ) -> iced_core::layout::Node {
        let inner_tree = if self.has_context_menu {
            &mut tree.children[0]
        } else {
            tree
        };
        ew_mut::<Message>(&mut self.inner).layout(inner_tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut crate::Renderer,
        theme: &crate::Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let inner_tree = if self.has_context_menu {
            &tree.children[0]
        } else {
            tree
        };
        ew::<Message>(&self.inner).draw(
            inner_tree, renderer, theme, defaults, layout, cursor, viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &crate::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.has_context_menu {
            if let Some(action) = crate::widget::text_context_menu::take_stock_action(
                &mut tree
                    .state
                    .downcast_mut::<EditorWrapperState>()
                    .text_menu_session,
            ) {
                self.execute_text_menu_action(tree, action, clipboard, shell);
            }
            tree.state
                .downcast_mut::<EditorWrapperState>()
                .text_menu_session
                .dismiss_on_event(event);
            ew_mut::<Message>(&mut self.inner).update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );

            use iced_core::widget::text::HasSelectableText;
            if self.inner.is_focused(&tree.children[0])
                && matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonPressed(_))
                        | Event::Touch(iced_core::touch::Event::FingerPressed { .. })
                )
                && cursor.is_over(layout.bounds())
            {
                crate::widget::text_input::notify_focus_change();
            }

            if !self.external_text_menu {
                if let Some(at) = self.take_text_menu_request(tree) {
                    let view =
                        crate::widget::text_context_menu::stock_view(self.text_menu_context(tree));
                    tree.state
                        .downcast_mut::<EditorWrapperState>()
                        .text_menu_session
                        .open(view, at, renderer, viewport, cursor, self.window_id);
                }
            }
        } else {
            ew_mut::<Message>(&mut self.inner).update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &crate::Renderer,
    ) -> mouse::Interaction {
        let inner_tree = if self.has_context_menu {
            &tree.children[0]
        } else {
            tree
        };
        ew::<Message>(&self.inner).mouse_interaction(inner_tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &crate::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, crate::Theme, crate::Renderer>> {
        if self.has_context_menu {
            if !self.external_text_menu {
                let session = &mut tree
                    .state
                    .downcast_mut::<EditorWrapperState>()
                    .text_menu_session;
                if session.is_open() {
                    return session.overlay(translation);
                }
            }

            ew_mut::<Message>(&mut self.inner).overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        } else {
            ew_mut::<Message>(&mut self.inner).overlay(
                tree,
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &crate::Renderer,
        operation: &mut dyn iced_core::widget::Operation,
    ) {
        let inner_tree = if self.has_context_menu {
            &mut tree.children[0]
        } else {
            tree
        };
        ew_mut::<Message>(&mut self.inner).operate(inner_tree, layout, renderer, operation);
    }

    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        state: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        let inner_state = if self.has_context_menu {
            &state.children[0]
        } else {
            state
        };
        ew::<Message>(&self.inner).a11y_nodes(layout, inner_state, cursor)
    }

    fn id(&self) -> Option<iced_core::widget::Id> {
        // With the context menu the inner editor's tree is a child of this
        // one, and carries the id itself. Were both trees to carry it, the
        // named-state restoration would hand the editor's state to this
        // wrapper and leave the editor stateless
        if self.has_context_menu {
            None
        } else {
            ew::<Message>(&self.inner).id()
        }
    }

    fn set_id(&mut self, id: iced_core::widget::Id) {
        ew_mut::<Message>(&mut self.inner).set_id(id);
    }
}

impl<'a, Message: Clone + 'static> From<TextEditor<'a, Message>> for crate::Element<'a, Message> {
    fn from(editor: TextEditor<'a, Message>) -> Self {
        Self::new(editor)
    }
}

impl<Message> TextMenuTarget<Message> for InnerEditor<'_, Message> {
    fn use_external_text_menu(&mut self) {}

    fn take_text_menu_request(&self, tree: &mut Tree) -> Option<iced_core::Point> {
        let position = self.context_menu_position(tree);
        self.set_context_menu_position(tree, None);
        position
    }

    fn text_menu_context(&self, tree: &Tree) -> TextMenuContext {
        TextMenuContext {
            kind: if self.is_editable() {
                TextKind::Editable
            } else {
                TextKind::ReadOnlyInput
            },
            secure: false,
            has_selection: self.selected_text(tree).is_some(),
            has_text: self.has_text(tree),
            clipboard_has_text: self.clipboard_has_text(tree),
        }
    }

    fn execute_text_menu_action(
        &mut self,
        tree: &mut Tree,
        action: TextAction,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let context = self.text_menu_context(tree);
        if action == TextAction::Paste {
            if context.kind != TextKind::Editable {
                return;
            }
        } else if !context.allows(action) {
            return;
        }
        match action {
            TextAction::Copy => self.copy_to_clipboard(tree, clipboard),
            TextAction::Cut => {
                self.copy_to_clipboard(tree, clipboard);
                self.dispatch_action(Action::Edit(Edit::Delete), shell);
            }
            TextAction::Paste => {
                let Some(text) = clipboard
                    .read(iced_core::clipboard::Kind::Standard)
                    .filter(|text| !text.is_empty())
                else {
                    return;
                };
                self.dispatch_action(Action::Edit(Edit::Paste(std::sync::Arc::new(text))), shell);
            }
            TextAction::SelectAll => self.dispatch_action(Action::SelectAll, shell),
        }
        shell.request_redraw();
    }
}

impl<Message: Clone + 'static> TextMenuTarget<Message> for TextEditor<'_, Message> {
    fn use_external_text_menu(&mut self) {
        self.external_text_menu = true;
    }

    fn take_text_menu_request(&self, tree: &mut Tree) -> Option<iced_core::Point> {
        let inner_tree = if self.has_context_menu {
            &mut tree.children[0]
        } else {
            tree
        };
        self.inner.take_text_menu_request(inner_tree)
    }

    fn text_menu_context(&self, tree: &Tree) -> TextMenuContext {
        let inner_tree = if self.has_context_menu {
            &tree.children[0]
        } else {
            tree
        };
        self.inner.text_menu_context(inner_tree)
    }

    fn execute_text_menu_action(
        &mut self,
        tree: &mut Tree,
        action: TextAction,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let inner_tree = if self.has_context_menu {
            &mut tree.children[0]
        } else {
            tree
        };
        self.inner
            .execute_text_menu_action(inner_tree, action, clipboard, shell);
    }
}
