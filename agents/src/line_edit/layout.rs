use super::*;
use unicode_width::UnicodeWidthStr;

/// Wrapping width and the first row's reserved label columns.
#[derive(Clone, Copy)]
pub struct Wrap {
    /// Maximum display columns per row.
    pub width: usize,
    /// Columns reserved before text on the first row.
    pub first_row_inset: usize,
}
/// A grapheme boundary expressed in byte and display coordinates.
pub struct CaretStop {
    /// Byte offset in the editor text.
    pub byte: usize,
    /// Display column within the visual row.
    pub column: usize,
    /// Zero-based visual row.
    pub row: usize,
}
/// A visible slice of text and its caret column.
pub struct Window<'a> {
    /// Text visible within the available columns.
    pub text: &'a str,
    /// Caret column relative to the visible text.
    pub caret_column: u16,
}
/// Direction of movement between visual rows.
pub enum Vertical {
    /// Move to the preceding visual row.
    Up,
    /// Move to the following visual row.
    Down,
}

impl<P: Placeholder> Editor<P> {
    /// Caret stops with byte offsets, display columns and visual rows. The first row starts
    /// after `first_row_inset` columns of label; later rows wrap at the full width. A newline never
    /// wraps, so a row filled to the width adds no blank row before the next line.
    pub fn caret_stops(&self, wrap: Wrap) -> Vec<CaretStop> {
        let Wrap {
            width,
            first_row_inset: inset,
        } = wrap;
        let width = width.max(1);
        let mut positions = Vec::new();
        let (mut column, mut row, mut row_start) = (inset.min(width - 1), 0, inset.min(width - 1));
        for (index, grapheme) in self.text.grapheme_indices(true) {
            let columns = grapheme.width();
            let newline = grapheme == "\n" || grapheme == "\r\n";
            if !newline && column > row_start && column + columns.max(1) > width {
                row += 1;
                column = 0;
                row_start = 0;
            }
            positions.push(CaretStop {
                byte: index,
                column,
                row,
            });
            if newline {
                row += 1;
                column = 0;
                row_start = 0;
            } else {
                column += columns.min(width);
            }
        }
        if column >= width {
            row += 1;
            column = 0;
        }
        positions.push(CaretStop {
            byte: self.text.len(),
            column,
            row,
        });
        positions
    }

    /// Places the caret after the displayed grapheme, or at the visible line's end.
    pub fn place_caret(&mut self, wrap: Wrap, column: usize, row: usize) {
        let positions = self.caret_stops(wrap);
        let row = row.min(positions.last().expect("end caret").row);
        let &CaretStop {
            byte, column: x, ..
        } = positions
            .iter()
            .rfind(|p| p.row == row && p.column <= column)
            .or_else(|| positions.iter().find(|p| p.row == row))
            .expect("visual row has caret");
        let mut offset = byte;
        if let Some(grapheme) = self.text[byte..].graphemes(true).next()
            && grapheme != "\n"
            && grapheme != "\r\n"
            && column >= x
        {
            offset += grapheme.len();
        }
        self.set_cursor(self.caret_boundary(offset, true));
    }

    /// Places the caret after the displayed grapheme in a single-line window.
    pub fn place_caret_in_window(&mut self, columns: usize, column: usize) {
        let width = columns;
        if width == 0 {
            self.set_cursor(self.text.len());
            return;
        }
        let visible = self.window(width).text;
        let start = visible.as_ptr() as usize - self.text.as_ptr() as usize;
        let mut end = start;
        let mut x = 0;
        for grapheme in visible.graphemes(true) {
            end += grapheme.len();
            x += grapheme.width();
            if column < x {
                break;
            }
        }
        if column >= visible.width() {
            end = self.text.len();
        }
        self.set_cursor(self.caret_boundary(end, true));
    }

    /// Returns false when the caret is already on the requested boundary row.
    pub fn move_row(&mut self, wrap: Wrap, direction: Vertical) -> bool {
        let Wrap {
            width,
            first_row_inset: inset,
        } = wrap;
        let down = matches!(direction, Vertical::Down);
        // Columns count from the text's start on every row, so the label's inset on the
        // first row does not shift the column the caret keeps.
        let inset = inset.min(width.max(1) - 1);
        let positions: Vec<_> = self
            .caret_stops(Wrap {
                width,
                first_row_inset: inset,
            })
            .into_iter()
            .map(|CaretStop { byte, column, row }| CaretStop {
                byte,
                column: column - if row == 0 { inset } else { 0 },
                row,
            })
            .collect();
        let &CaretStop { column, row, .. } = positions
            .iter()
            .find(|p| p.byte == self.cursor)
            .expect("caret boundary");
        let target_row = if down {
            if row == positions.last().expect("end caret").row {
                return false;
            }
            row + 1
        } else {
            let Some(row) = row.checked_sub(1) else {
                return false;
            };
            row
        };
        let column = *self.preferred_column.get_or_insert(column);
        self.cursor = positions
            .iter()
            .rfind(|p| p.row == target_row && p.column <= column)
            .or_else(|| positions.iter().find(|p| p.row == target_row))
            .expect("visual row has caret")
            .byte;
        self.cursor = self.caret_boundary(self.cursor, down);
        true
    }

    /// Returns a single-line text window that keeps the caret visible.
    pub fn window(&self, columns: usize) -> Window<'_> {
        let cols = columns.min(usize::from(u16::MAX));
        if cols == 0 {
            return Window {
                text: "",
                caret_column: 0,
            };
        }
        let caret_width = self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(1, |g| g.width().max(1));
        let mut start = self.cursor;
        let mut caret = 0;
        for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
            if caret + g.width() > cols.saturating_sub(caret_width) {
                break;
            }
            caret += g.width();
            start = i;
        }
        let mut end = start;
        let mut width = 0;
        for g in self.text[start..].graphemes(true) {
            if width + g.width() > cols {
                break;
            }
            width += g.width();
            end += g.len();
        }
        Window {
            text: &self.text[start..end],
            caret_column: caret as u16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(text: &str) -> Editor {
        let mut editor = Editor::default();
        editor.set_text(text);
        editor
    }
    #[test]
    fn windows_keep_wide_caret_visible() {
        let mut e = edit("a你好b");
        assert_eq!(
            {
                let window = e.window(4);
                (window.text, window.caret_column)
            },
            ("好b", 3)
        );
        e.move_caret(Motion::Left);
        assert_eq!(
            {
                let window = e.window(4);
                (window.text, window.caret_column)
            },
            ("好b", 2)
        );
        e.move_caret(Motion::Left);
        assert_eq!(
            {
                let window = e.window(4);
                (window.text, window.caret_column)
            },
            ("你好", 2)
        );
        e.move_caret(Motion::TextStart);
        assert_eq!(
            {
                let window = e.window(4);
                (window.text, window.caret_column)
            },
            ("a你", 0)
        );
        assert_eq!(
            {
                let window = e.window(0);
                (window.text, window.caret_column)
            },
            ("", 0)
        );
        e.set_text("你好");
        assert_eq!(
            {
                let window = e.window(1);
                (window.text, window.caret_column)
            },
            ("", 0)
        );
        e.set_text("e\u{301}👨‍👩‍👧‍👦");
        assert_eq!(
            {
                let window = e.window(4);
                (window.text, window.caret_column)
            },
            ("e\u{301}👨‍👩‍👧‍👦", 3)
        );
    }
}
