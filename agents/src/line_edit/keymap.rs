use super::{EditCommand, Motion, WordKind};
use EditCommand::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// CSI-u can report an ASCII control code instead of its letter and Control modifier.
pub fn normalize_control_code(mut key: KeyEvent) -> KeyEvent {
    if let KeyCode::Char(character @ '\u{1}'..='\u{1a}') = key.code {
        key.code = KeyCode::Char(char::from(character as u8 - 1 + b'a'));
        key.modifiers |= KeyModifiers::CONTROL;
    }
    key
}

/// GNU readline emacs bindings; unowned keys and releases return None.
pub fn command_for_key(key: KeyEvent) -> Option<EditCommand> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let modifiers = key.modifiers - KeyModifiers::SHIFT;
    Some(match (key.code, modifiers) {
        (KeyCode::Char('h'), KeyModifiers::CONTROL) | (KeyCode::Backspace, KeyModifiers::NONE) => {
            Delete(Motion::Left)
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::Delete, KeyModifiers::NONE) => {
            Delete(Motion::Right)
        }
        (KeyCode::Char('b'), KeyModifiers::CONTROL) | (KeyCode::Left, KeyModifiers::NONE) => {
            Move(Motion::Left)
        }
        (KeyCode::Char('f'), KeyModifiers::CONTROL) | (KeyCode::Right, KeyModifiers::NONE) => {
            Move(Motion::Right)
        }
        (KeyCode::Char('a'), KeyModifiers::CONTROL) => Move(Motion::LineStart),
        (KeyCode::Char('e'), KeyModifiers::CONTROL) => Move(Motion::LineEnd),
        (KeyCode::Home, KeyModifiers::NONE) => Move(Motion::TextStart),
        (KeyCode::End, KeyModifiers::NONE) => Move(Motion::TextEnd),
        (KeyCode::Char('b'), KeyModifiers::ALT) => Move(Motion::WordLeft(WordKind::Alphanumeric)),
        (KeyCode::Char('f'), KeyModifiers::ALT) => Move(Motion::WordRight(WordKind::Alphanumeric)),
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => Kill(Motion::LineEnd),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => Kill(Motion::LineStart),
        (KeyCode::Char('w'), KeyModifiers::CONTROL) => Kill(Motion::WordLeft(WordKind::Whitespace)),
        (KeyCode::Backspace, KeyModifiers::ALT) => Kill(Motion::WordLeft(WordKind::Alphanumeric)),
        (KeyCode::Char('d'), KeyModifiers::ALT) => Kill(Motion::WordRight(WordKind::Alphanumeric)),
        (KeyCode::Char('y'), KeyModifiers::CONTROL) => Yank,
        (KeyCode::Char(c), KeyModifiers::NONE) if !c.is_control() => Insert(c.to_string()),
        _ => return None,
    })
}
