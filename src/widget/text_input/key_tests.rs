//! Which key presses a focused text input types, sent through the widget's
//! real update as a window sends them. The window reports a key's character
//! whatever modifiers are held: xkb gives "x" for Alt+x and "=" for Ctrl+=.

use super::input::text_input;
use iced::advanced::renderer::Headless;
use iced_core::keyboard::key::{NativeCode, Physical};
use iced_core::keyboard::{self, Key, Location, Modifiers};
use iced_core::{Event, Point, Size, clipboard, event, input_method, mouse};
use iced_runtime::user_interface::{Cache, UserInterface};

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Input(String),
    Submit(String),
}

const SIZE: Size = Size::new(300.0, 60.0);
const INSIDE: Point = Point::new(100.0, 20.0);

/// A text input as an app holds one: the app keeps the value
struct Field {
    value: String,
    submitted: Option<String>,
    renderer: crate::Renderer,
    cache: Cache,
}

impl Field {
    /// A field holding `value`, clicked into
    fn focused(value: &str) -> Self {
        let renderer =
            iced::futures::executor::block_on(<crate::Renderer as Headless>::new(
                iced::Font::default(),
                iced::Pixels(14.0),
                Some("tiny-skia"),
            ))
            .expect("a headless renderer");
        let mut field = Self {
            value: value.to_owned(),
            submitted: None,
            renderer,
            cache: Cache::default(),
        };
        field.send(Event::Mouse(mouse::Event::CursorMoved { position: INSIDE }));
        field.send(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)));
        field.send(Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
        field
    }

    fn send(&mut self, event: Event) -> event::Status {
        let element: crate::Element<'_, Message> = text_input("", self.value.clone())
            .on_input(Message::Input)
            .on_submit(Message::Submit)
            .into();
        let mut ui = UserInterface::build(
            element,
            SIZE,
            std::mem::take(&mut self.cache),
            &mut self.renderer,
        );
        let mut messages = Vec::new();
        let (_, statuses) = ui.update(
            &[event],
            mouse::Cursor::Available(INSIDE),
            &mut self.renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        self.cache = ui.into_cache();
        for message in messages {
            match message {
                Message::Input(value) => self.value = value,
                Message::Submit(value) => self.submitted = Some(value),
            }
        }
        statuses[0]
    }

    /// A press of `key`, as `KeyPressed` carries it, with its `text`
    fn press(&mut self, key: Key, modifiers: Modifiers, text: Option<&str>) -> event::Status {
        self.send(Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)));
        self.send(Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        }))
    }

    fn press_character(&mut self, modifiers: Modifiers, text: &str) -> event::Status {
        self.press(Key::Character(text.into()), modifiers, Some(text))
    }
}

const ALT: Modifiers = Modifiers::ALT;
const SUPER: Modifiers = Modifiers::LOGO;
const CTRL: Modifiers = Modifiers::CTRL;
const SHIFT: Modifiers = Modifiers::SHIFT;

#[test]
fn a_key_held_with_alt_or_super_is_not_typed_and_is_left_to_the_app() {
    let mut wrong = Vec::new();
    for (modifiers, text) in [
        (ALT, ","),
        (ALT, "x"),
        (ALT, " "),
        // Shift does not make it typing
        (ALT.union(SHIFT), ">"),
        // Nor does a layout that isn't Latin
        (ALT, "ф"),
        (ALT, "ع"),
        (SUPER, ","),
        (SUPER, "x"),
        (ALT.union(SUPER), "x"),
    ] {
        let mut field = Field::focused("note");
        let status = field.press_character(modifiers, text);
        // Captured, it would never reach the app's shortcuts
        if field.value != "note" || status != event::Status::Ignored {
            wrong.push(format!("{modifiers:?} {text:?}: {:?}, {status:?}", field.value));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn a_key_held_with_ctrl_is_not_typed() {
    for (modifiers, text) in [
        (CTRL, "="),
        (CTRL, "0"),
        (CTRL.union(SHIFT), "+"),
        (CTRL.union(ALT), "="),
    ] {
        let mut field = Field::focused("note");
        field.press_character(modifiers, text);
        assert_eq!(field.value, "note", "{modifiers:?} {text:?} was typed");
    }
}

/// xkb reports AltGr as level 3 (Mod5), which iced has no modifier for, so an
/// AltGr character arrives with no Alt in `modifiers`. That holds for all 594
/// layouts and variants of xkeyboard-config: on none does Alt or Super change
/// a key's text, so neither is how anyone types a character on Linux.
#[test]
fn shift_altgr_and_dead_keys_still_type() {
    for (modifiers, text, typed) in [
        (Modifiers::empty(), ",", "note,"),
        (Modifiers::empty(), " ", "note "),
        (SHIFT, ">", "note>"),
        (SHIFT, "A", "noteA"),
        (Modifiers::CAPS_LOCK, "A", "noteA"),
        // Layouts that aren't Latin
        (Modifiers::empty(), "ф", "noteф"),
        (SHIFT, "Ф", "noteФ"),
        (Modifiers::empty(), "ع", "noteع"),
        // AltGr+e and AltGr+q on a German layout, AltGr+Shift+p on a US
        // international one
        (Modifiers::empty(), "€", "note€"),
        (Modifiers::empty(), "@", "note@"),
        (SHIFT, "Ö", "noteÖ"),
    ] {
        let mut field = Field::focused("note");
        let status = field.press_character(modifiers, text);
        assert_eq!(field.value, typed, "{modifiers:?} {text:?}");
        assert_eq!(status, event::Status::Captured, "{modifiers:?} {text:?}");
    }

    // A dead key types nothing itself; the next key carries the composed text
    let mut field = Field::focused("caf");
    field.press(Key::Unidentified, Modifiers::empty(), None);
    field.press(Key::Character("e".into()), Modifiers::empty(), Some("é"));
    assert_eq!(field.value, "café");
}

#[test]
fn an_input_method_types_while_alt_is_held() {
    let mut field = Field::focused("note");
    field.send(Event::Keyboard(keyboard::Event::ModifiersChanged(ALT)));
    field.send(Event::InputMethod(input_method::Event::Commit("日本".into())));
    assert_eq!(field.value, "note日本");
}

#[test]
fn alt_keeps_the_editing_keys() {
    use keyboard::key::Named;

    let mut field = Field::focused("note");
    field.press(Key::Named(Named::Backspace), ALT, Some("\u{8}"));
    assert_eq!(field.value, "not");

    field.press(Key::Named(Named::Enter), ALT, Some("\r"));
    assert_eq!(field.submitted.as_deref(), Some("not"));
}
