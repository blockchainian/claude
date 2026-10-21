//! Draws a session terminal's screen and turns dashboard input into the bytes it expects.

use super::{Panel, View};
use crate::model::Action;
use crate::providers::SessionKey;
use crate::session_terminal::Screen;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

impl View {
    /// Shows `screen` in the event detail panel.
    pub fn show_terminal(&mut self, id: SessionKey, screen: Screen) {
        let activate = !self.terminal_requested();
        self.request_terminal();
        self.terminal = Some((id, screen));
        if activate {
            self.set_active_panel(Panel::EventDetail);
        }
    }

    pub fn hide_terminal(&mut self) {
        if self.terminal.as_ref().is_none_or(|(id, _)| {
            self.panels_task
                .as_ref()
                .and_then(super::draft_controls::TaskKey::session)
                == Some(id)
        }) {
            self.event_detail.mode = self.event_detail.return_mode;
        }
        self.terminal = None;
        self.viewport.borrow_mut().terminal = Rect::default();
    }

    pub(crate) fn request_terminal(&mut self) {
        if !self.terminal_requested() {
            self.event_detail.return_mode = self.event_detail.mode;
            self.event_detail.mode = super::task_ui::EventDetailMode::Terminal;
        }
    }

    pub(crate) fn terminal_requested(&self) -> bool {
        self.event_detail.mode == super::task_ui::EventDetailMode::Terminal
    }

    /// Focuses an explicitly opened terminal, including one waiting for its client to close.
    pub(crate) fn focus_terminal(&mut self) {
        if self.terminal_requested() {
            self.set_active_panel(Panel::EventDetail);
        }
    }

    /// The rows and columns a terminal gets in the event detail panel, below its top rule.
    pub fn terminal_size(&self) -> Option<(u16, u16)> {
        let viewport = self.viewport.borrow();
        let area = if viewport.terminal.is_empty() {
            let detail = viewport.event_detail;
            Rect {
                y: detail.y + 1,
                height: detail.height.saturating_sub(1),
                ..detail
            }
        } else {
            viewport.terminal
        };
        (!area.is_empty()).then_some((area.height, area.width))
    }

    /// The open terminal's screen while its session is the selected one.
    pub(super) fn shown_terminal(&self) -> Option<&Screen> {
        let (id, screen) = self.terminal.as_ref()?;
        (self.selected_id() == Some(id)).then_some(screen)
    }

    fn terminal_focused(&self) -> bool {
        self.confirm_dialog.is_none()
            && self.active_panel == Panel::EventDetail
            && self.shown_terminal().is_some()
    }

    pub fn terminal_paste(&self, text: &str) -> Option<Action> {
        self.terminal_focused()
            .then(|| Action::TerminalPaste(text.to_owned()))
    }

    /// Every key goes to the focused terminal; returns `None` when it is not focused.
    pub(super) fn terminal_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        if !self.terminal_focused() {
            return None;
        }
        let parser = self.shown_terminal()?.lock().expect("terminal screen lock");
        let bytes = key_bytes(key, parser.screen().application_cursor());
        Some((!bytes.is_empty()).then_some(Action::TerminalInput(bytes)))
    }

    /// Forwards mouse events over the terminal to its program as far as the program asked
    /// for them; the right button always switches back to the event detail. Returns `None`
    /// for events elsewhere.
    pub(super) fn terminal_mouse(&mut self, mouse: MouseEvent) -> Option<Option<Action>> {
        let area = self.viewport.borrow().terminal;
        let position = (mouse.column, mouse.row).into();
        if self.confirm_dialog.is_some()
            || !area.contains(position)
            || self.shown_terminal().is_none()
            || matches!(
                mouse.kind,
                MouseEventKind::Down(MouseButton::Right)
                    | MouseEventKind::Up(MouseButton::Right)
                    | MouseEventKind::Drag(MouseButton::Right)
            )
        {
            return None;
        }
        let parser = self.shown_terminal()?.lock().expect("terminal screen lock");
        let screen = parser.screen();
        if mouse.kind == MouseEventKind::Down(MouseButton::Middle)
            && screen.mouse_protocol_mode() == vt100::MouseProtocolMode::None
        {
            return Some(Some(Action::PasteText));
        }
        Some(
            mouse_bytes(
                mouse,
                mouse.column - area.x,
                mouse.row - area.y,
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
            )
            .map(Action::TerminalInput),
        )
    }

    /// Draws the shown terminal below the panel's top rule; returns false when none is shown.
    pub(super) fn render_terminal(
        &self,
        area: Rect,
        buf: &mut Buffer,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let Some(screen) = self.shown_terminal() else {
            return false;
        };
        let area = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
        self.viewport.borrow_mut().terminal = area;
        let screen = screen.lock().expect("terminal screen lock");
        if screen.ready(std::time::Instant::now()) {
            render(screen.screen(), area, buf);
        } else if !area.is_empty() {
            let x = super::centering::centered_x(area, area, 3);
            buf.set_stringn(
                x,
                area.y,
                super::tool_detail::running_frame(now),
                usize::from(area.width),
                super::style::event_styles(&crate::event::Kind::Tool)
                    .1
                    .add_modifier(Modifier::DIM),
            );
        }
        true
    }

    /// Where the focused terminal's program has its cursor.
    pub(super) fn terminal_cursor(&self) -> Option<(u16, u16)> {
        if !self.terminal_focused() {
            return None;
        }
        let parser = self.shown_terminal()?.lock().expect("terminal screen lock");
        let screen = parser.screen();
        if !parser.ready(std::time::Instant::now()) || screen.hide_cursor() {
            return None;
        }
        let area = self.viewport.borrow().terminal;
        let (row, col) = screen.cursor_position();
        (row < area.height && col < area.width).then_some((area.x + col, area.y + row))
    }
}

/// Copies the visible part of `screen` into `area`.
pub(super) fn render(screen: &vt100::Screen, area: Rect, buf: &mut Buffer) {
    for row in 0..area.height {
        for col in 0..area.width {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let target = &mut buf[(area.x + col, area.y + row)];
            let contents = cell.contents();
            target.set_symbol(if contents.is_empty() { " " } else { contents });
            target.set_style(cell_style(cell));
        }
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::new()
        .fg(color(cell.fgcolor()))
        .bg(color(cell.bgcolor()));
    for (on, modifier) in [
        (cell.bold(), Modifier::BOLD),
        (cell.dim(), Modifier::DIM),
        (cell.italic(), Modifier::ITALIC),
        (cell.underline(), Modifier::UNDERLINED),
        (cell.inverse(), Modifier::REVERSED),
    ] {
        if on {
            style = style.add_modifier(modifier);
        }
    }
    style
}

fn color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// The bytes an xterm sends for `key`; empty for keys it has no encoding for.
pub(super) fn key_bytes(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
    let escaped = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if alt {
            out.push(0x1b);
        }
        out.extend_from_slice(bytes);
        out
    };
    // Cursor and editing keys: `ESC [ 1 ; m X` with modifiers, `ESC O X` in application mode.
    let cursor = |letter: u8| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{}", letter as char).into_bytes()
        } else if application_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    let tilde = |number: u8| {
        if modifier > 1 {
            format!("\x1b[{number};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    match key.code {
        KeyCode::Char(c) if control => match control_byte(c) {
            Some(byte) => escaped(&[byte]),
            None => Vec::new(),
        },
        KeyCode::Char(c) => escaped(c.to_string().as_bytes()),
        // Shift+Enter sends a newline as ESC CR.
        KeyCode::Enter if shift => b"\x1b\r".to_vec(),
        KeyCode::Enter => escaped(b"\r"),
        KeyCode::Tab => escaped(b"\t"),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace if control => escaped(&[0x08]),
        KeyCode::Backspace => escaped(&[0x7f]),
        KeyCode::Esc => escaped(&[0x1b]),
        KeyCode::Up => cursor(b'A'),
        KeyCode::Down => cursor(b'B'),
        KeyCode::Right => cursor(b'C'),
        KeyCode::Left => cursor(b'D'),
        KeyCode::Home => cursor(b'H'),
        KeyCode::End => cursor(b'F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) if modifier == 1 => vec![0x1b, b'O', b'P' + n - 1],
        KeyCode::F(n @ 1..=4) => {
            format!("\x1b[1;{modifier}{}", (b'P' + n - 1) as char).into_bytes()
        }
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        _ => Vec::new(),
    }
}

fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        ' ' | '@' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// The report for `mouse` at `col`, `row` inside the terminal, when the program enabled
/// reporting for that kind of event. The right button stays with the dashboard.
pub(super) fn mouse_bytes(
    mouse: MouseEvent,
    col: u16,
    row: u16,
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    use vt100::MouseProtocolMode as Mode;
    let button = |button: MouseButton| match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => None,
    };
    let (code, release) = match mouse.kind {
        MouseEventKind::Down(pressed) if mode != Mode::None => (button(pressed)?, false),
        MouseEventKind::Up(pressed) if mode != Mode::None && mode != Mode::Press => {
            (button(pressed)?, true)
        }
        MouseEventKind::Drag(pressed) if matches!(mode, Mode::ButtonMotion | Mode::AnyMotion) => {
            (button(pressed)? + 32, false)
        }
        MouseEventKind::ScrollUp if mode != Mode::None => (64, false),
        MouseEventKind::ScrollDown if mode != Mode::None => (65, false),
        _ => return None,
    };
    let modifiers = 4 * u8::from(mouse.modifiers.contains(KeyModifiers::SHIFT))
        + 8 * u8::from(mouse.modifiers.contains(KeyModifiers::ALT))
        + 16 * u8::from(mouse.modifiers.contains(KeyModifiers::CONTROL));
    let code = code + modifiers;
    let (x, y) = (col + 1, row + 1);
    Some(match encoding {
        vt100::MouseProtocolEncoding::Sgr => {
            format!("\x1b[<{code};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
        }
        _ => {
            // The default encoding has no release button and caps coordinates at 223.
            let code = if release { 3 + modifiers } else { code };
            let cap = |value: u16| (value.min(223) + 32) as u8;
            vec![0x1b, b'[', b'M', code + 32, cap(x), cap(y)]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
        key_bytes(KeyEvent::new(code, modifiers), false)
    }

    #[test]
    fn keys_encode_as_xterm_does() {
        assert_eq!(key(KeyCode::Char('a'), KeyModifiers::NONE), b"a");
        assert_eq!(key(KeyCode::Char('é'), KeyModifiers::NONE), "é".as_bytes());
        assert_eq!(key(KeyCode::Char('c'), KeyModifiers::CONTROL), [3]);
        assert_eq!(key(KeyCode::Char('b'), KeyModifiers::ALT), b"\x1bb");
        assert_eq!(key(KeyCode::Enter, KeyModifiers::NONE), b"\r");
        assert_eq!(key(KeyCode::Enter, KeyModifiers::SHIFT), b"\x1b\r");
        assert_eq!(key(KeyCode::Esc, KeyModifiers::NONE), [0x1b]);
        assert_eq!(key(KeyCode::Backspace, KeyModifiers::NONE), [0x7f]);
        assert_eq!(key(KeyCode::BackTab, KeyModifiers::SHIFT), b"\x1b[Z");
        assert_eq!(key(KeyCode::Up, KeyModifiers::NONE), b"\x1b[A");
        assert_eq!(
            key_bytes(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true),
            b"\x1bOA"
        );
        assert_eq!(key(KeyCode::Left, KeyModifiers::CONTROL), b"\x1b[1;5D");
        assert_eq!(key(KeyCode::Delete, KeyModifiers::NONE), b"\x1b[3~");
        assert_eq!(key(KeyCode::F(1), KeyModifiers::NONE), b"\x1bOP");
        assert_eq!(key(KeyCode::F(5), KeyModifiers::NONE), b"\x1b[15~");
    }

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn mouse_reports_follow_the_requested_mode() {
        use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};
        let down = mouse(MouseEventKind::Down(MouseButton::Left));
        assert_eq!(mouse_bytes(down, 2, 3, Mode::None, Encoding::Sgr), None);
        assert_eq!(
            mouse_bytes(down, 2, 3, Mode::PressRelease, Encoding::Sgr),
            Some(b"\x1b[<0;3;4M".to_vec())
        );
        assert_eq!(
            mouse_bytes(
                mouse(MouseEventKind::Up(MouseButton::Left)),
                2,
                3,
                Mode::PressRelease,
                Encoding::Sgr
            ),
            Some(b"\x1b[<0;3;4m".to_vec())
        );
        assert_eq!(
            mouse_bytes(
                mouse(MouseEventKind::ScrollDown),
                0,
                0,
                Mode::PressRelease,
                Encoding::Default
            ),
            Some(vec![0x1b, b'[', b'M', 65 + 32, 33, 33])
        );
        assert_eq!(
            mouse_bytes(
                mouse(MouseEventKind::Down(MouseButton::Right)),
                0,
                0,
                Mode::AnyMotion,
                Encoding::Sgr
            ),
            None
        );
    }

    #[test]
    fn screen_cells_keep_text_and_colors() {
        let mut parser = vt100::Parser::new(2, 6, 0);
        parser.process(b"\x1b[31mhi\x1b[0m \xe4\xbd\xa0");
        let area = Rect::new(1, 1, 6, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        render(parser.screen(), area, &mut buf);
        assert_eq!(buf[(1, 1)].symbol(), "h");
        assert_eq!(buf[(1, 1)].fg, Color::Indexed(1));
        assert_eq!(buf[(3, 1)].symbol(), " ");
        assert_eq!(buf[(4, 1)].symbol(), "你");
        assert_eq!(buf[(1, 2)].symbol(), " ");
    }
}
