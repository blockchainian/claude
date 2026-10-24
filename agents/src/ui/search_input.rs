use super::line_edit::LineEdit;
use super::render::{render_input_row, row};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{buffer::Buffer, layout::Rect, style::Stylize, text::Line, widgets::Widget};
use unicode_width::UnicodeWidthStr;

const SEARCH_PROMPT: &str = "Search ❯ ";

#[derive(Default)]
pub(crate) struct SearchInput {
    query: LineEdit,
    focused: bool,
}

impl SearchInput {
    pub fn focus(&mut self) {
        self.focused = true;
        self.query.end();
    }

    pub fn blur(&mut self) {
        self.focused = false;
    }

    pub fn close(&mut self) {
        self.query.clear();
        self.blur();
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn applied(&self) -> bool {
        self.focused || !self.query.is_empty()
    }

    pub fn text(&self) -> &str {
        self.query.text()
    }

    pub fn insert(&mut self, text: &str) {
        self.query.insert(text);
    }

    /// False means the key goes on to the panel: unfocused or not a search key.
    /// Enter blurs, Esc closes, and everything else goes to `LineEdit::handle`.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if !self.focused {
            return false;
        }
        match key.code {
            KeyCode::Enter => self.blur(),
            KeyCode::Esc => self.close(),
            _ => return self.query.handle(key),
        }
        true
    }

    pub fn render(&self, row: Rect, buf: &mut Buffer) {
        if row.is_empty() {
            return;
        }
        if self.focused {
            render_input_row(SEARCH_PROMPT, &self.query, row, buf);
        } else {
            Line::from(self.text().to_owned())
                .dim()
                .right_aligned()
                .render(row, buf);
        }
    }

    pub fn caret(&self, row: Rect) -> Option<(u16, u16)> {
        if !self.focused || row.is_empty() {
            return None;
        }
        let prompt_width = SEARCH_PROMPT.width();
        let (_, caret) = self
            .query
            .window(usize::from(row.width).saturating_sub(prompt_width));
        let column =
            (prompt_width + usize::from(caret)).min(usize::from(row.width.saturating_sub(1)));
        Some((row.x + column as u16, row.y))
    }

    pub fn click(&mut self, row: Rect, column: u16) {
        self.focused = true;
        self.query.click_row_after(row, SEARCH_PROMPT, column);
    }

    #[cfg(test)]
    pub fn set_text(&mut self, text: &str) {
        self.query.set_text(text);
    }
}

/// The bottom search input row; empty when the panel is less than two rows high.
pub(super) fn search_row(area: Rect) -> Rect {
    if area.height < 2 {
        return Rect::default();
    }
    row(area, area.height.saturating_sub(1), 1)
}
