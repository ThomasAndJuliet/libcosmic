//! Operate on widgets that can be focused.
use crate::Rectangle;
use crate::id::IdEq;
use crate::widget::Id;
use crate::widget::operation::{self, Operation, Outcome};

/// The internal state of a widget that can be focused.
pub trait Focusable {
    /// Returns whether the widget is focused or not.
    fn is_focused(&self) -> bool;

    /// Focuses the widget.
    fn focus(&mut self);

    /// Unfocuses the widget.
    fn unfocus(&mut self);
}

/// A summary of the focusable widgets present on a widget tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Count {
    /// The index of the current focused widget, if any.
    pub focused: Option<usize>,

    /// The total amount of focusable widgets.
    pub total: usize,
}

/// Produces an [`Operation`] that focuses the widget with the given [`Id`].
/// Native windows keep independent focus: siblings are unfocused only in
/// windows containing the target. A target that has closed changes no window.
pub fn focus<T>(target: Id) -> impl Operation<T> {
    struct Locate {
        target: Id,
        current_window: Option<crate::window::Id>,
        target_windows: Vec<crate::window::Id>,
    }

    struct Focus {
        target: Id,
        current_window: Option<crate::window::Id>,
        target_windows: Vec<crate::window::Id>,
    }

    fn apply(target: &Id, id: Option<&Id>, state: &mut dyn Focusable) {
        if id.is_some_and(|id| IdEq::eq(&id.0, &target.0)) {
            state.focus();
        } else {
            state.unfocus();
        }
    }

    impl<T> Operation<T> for Locate {
        fn focusable(
            &mut self,
            id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            let Some(window) = self.current_window else {
                // Direct widget-tree callers operate on a single tree and
                // do not run a native window traversal or chained pass.
                apply(&self.target, id, state);
                return;
            };
            if id.is_some_and(|id| IdEq::eq(&id.0, &self.target.0))
                && !self.target_windows.contains(&window)
            {
                self.target_windows.push(window);
            }
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn set_window_id(&mut self, id: crate::window::Id) {
            self.current_window = Some(id);
        }

        fn finish(&self) -> Outcome<T> {
            if self.target_windows.is_empty() {
                Outcome::None
            } else {
                Outcome::Chain(Box::new(Focus {
                    target: self.target.clone(),
                    current_window: None,
                    target_windows: self.target_windows.clone(),
                }))
            }
        }
    }

    impl<T> Operation<T> for Focus {
        fn focusable(
            &mut self,
            id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            if self
                .current_window
                .is_some_and(|window| self.target_windows.contains(&window))
            {
                apply(&self.target, id, state);
            }
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            if self
                .current_window
                .is_some_and(|window| self.target_windows.contains(&window))
            {
                operate(self);
            }
        }

        fn set_window_id(&mut self, id: crate::window::Id) {
            self.current_window = Some(id);
        }
    }

    Locate {
        target,
        current_window: None,
        target_windows: Vec::new(),
    }
}

/// Produces an [`Operation`] that unfocuses the focused widget.
pub fn unfocus<T>() -> impl Operation<T> {
    struct Unfocus;

    impl<T> Operation<T> for Unfocus {
        fn focusable(
            &mut self,
            _id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            state.unfocus();
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }
    }

    Unfocus
}

/// Produces an [`Operation`] that generates a [`Count`] and chains it with the
/// provided function to build a new [`Operation`].
pub fn count() -> impl Operation<Count> {
    struct CountFocusable {
        count: Count,
    }

    impl Operation<Count> for CountFocusable {
        fn focusable(
            &mut self,
            _id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            if state.is_focused() {
                self.count.focused = Some(self.count.total);
            }

            self.count.total += 1;
        }

        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn Operation<Count>),
        ) {
            operate(self);
        }

        fn finish(&self) -> Outcome<Count> {
            Outcome::Some(self.count)
        }
    }

    CountFocusable {
        count: Count::default(),
    }
}

/// Produces an [`Operation`] that searches for the current focused widget, and
/// - if found, focuses the previous focusable widget.
/// - if not found, focuses the last focusable widget.
pub fn focus_previous<T>() -> impl Operation<T>
where
    T: Send + 'static,
{
    struct FocusPrevious {
        count: Count,
        current: usize,
    }

    impl<T> Operation<T> for FocusPrevious {
        fn focusable(
            &mut self,
            _id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            if self.count.total == 0 {
                return;
            }

            match self.count.focused {
                None if self.current == self.count.total - 1 => state.focus(),
                Some(0) if self.current == 0 && self.count.total == 1 => {}
                Some(0) if self.current == 0 => state.unfocus(),
                Some(0) if self.current == self.count.total - 1 => {
                    state.focus()
                }
                Some(0) => {}
                Some(focused) if focused == self.current => state.unfocus(),
                Some(focused) if focused - 1 == self.current => state.focus(),
                _ => {}
            }

            self.current += 1;
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }
    }

    operation::then(count(), |count| FocusPrevious { count, current: 0 })
}

/// Produces an [`Operation`] that searches for the current focused widget, and
/// - if found, focuses the next focusable widget.
/// - if not found, focuses the first focusable widget.
pub fn focus_next<T>() -> impl Operation<T>
where
    T: Send + 'static,
{
    struct FocusNext {
        count: Count,
        current: usize,
    }

    impl<T> Operation<T> for FocusNext {
        fn focusable(
            &mut self,
            _id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            match self.count.focused {
                None if self.current == 0 => state.focus(),
                Some(focused)
                    if focused == self.current && self.count.total == 1 => {}
                Some(focused) if focused == self.current => state.unfocus(),
                Some(focused) if focused + 1 == self.current => state.focus(),
                Some(focused)
                    if focused == self.count.total - 1 && self.current == 0 =>
                {
                    state.focus()
                }
                _ => {}
            }

            self.current += 1;
        }

        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }
    }

    operation::then(count(), |count| FocusNext { count, current: 0 })
}

/// Produces an [`Operation`] that searches for the current focused widget
/// and stores its ID. This ignores widgets that do not have an ID.
pub fn find_focused() -> impl Operation<Id> {
    struct FindFocused {
        focused: Option<Id>,
    }

    impl Operation<Id> for FindFocused {
        fn focusable(
            &mut self,
            id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            if state.is_focused() && id.is_some() {
                self.focused = id.cloned();
            }
        }

        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn Operation<Id>),
        ) {
            operate(self);
        }

        fn finish(&self) -> Outcome<Id> {
            if let Some(id) = &self.focused {
                Outcome::Some(id.clone())
            } else {
                Outcome::None
            }
        }
    }

    FindFocused { focused: None }
}

/// Produces an [`Operation`] that searches for the focusable widget
/// and stores whether it is focused or not. This ignores widgets that
/// do not have an ID.
pub fn is_focused(target: Id) -> impl Operation<bool> {
    struct IsFocused {
        target: Id,
        is_focused: Option<bool>,
    }

    impl Operation<bool> for IsFocused {
        fn focusable(
            &mut self,
            id: Option<&Id>,
            _bounds: Rectangle,
            state: &mut dyn Focusable,
        ) {
            if id.is_some_and(|id| *id == self.target) {
                self.is_focused = Some(state.is_focused());
            }
        }

        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn Operation<bool>),
        ) {
            if self.is_focused.is_some() {
                return;
            }

            operate(self);
        }

        fn finish(&self) -> Outcome<bool> {
            self.is_focused.map_or(Outcome::None, Outcome::Some)
        }
    }

    IsFocused {
        target,
        is_focused: None,
    }
}
