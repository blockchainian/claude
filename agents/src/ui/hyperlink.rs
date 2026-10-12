use ratatui::{buffer::Buffer, layout::Rect};

/// Adds an OSC 8 link to an already rendered, single-row ASCII label.
pub(super) fn hyperlink(buf: &mut Buffer, area: Rect, url: &str) {
    use ratatui::buffer::CellDiffOption;
    use std::num::NonZeroU16;

    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    let first = &mut buf[(area.x, area.y)];
    first.set_symbol(&format!("\x1b]8;;{url}\x1b\\{}", first.symbol()));
    first.set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap()));
    let last = &mut buf[(area.right() - 1, area.y)];
    last.set_symbol(&format!("{}\x1b]8;;\x1b\\", last.symbol()));
    last.set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::CellDiffOption;
    use std::num::NonZeroU16;

    #[test]
    fn hyperlink_preserves_cells_and_width() {
        for text in ["#833", "#"] {
            let area = Rect::new(3, 2, text.len() as u16, 1);
            let mut buf = Buffer::empty(area);
            buf.set_string(3, 2, text, ratatui::style::Style::default());
            hyperlink(&mut buf, area, "https://example.com/pull/833");
            for (i, ch) in text.chars().enumerate() {
                let cell = &buf[(3 + i as u16, 2)];
                let mut expected = ch.to_string();
                if i == 0 {
                    expected = format!("\x1b]8;;https://example.com/pull/833\x1b\\{expected}");
                }
                if i + 1 == text.len() {
                    expected.push_str("\x1b]8;;\x1b\\");
                }
                assert_eq!(cell.symbol(), expected);
                if i == 0 || i + 1 == text.len() {
                    assert_eq!(
                        cell.diff_option,
                        CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap())
                    );
                }
            }
            assert_eq!(Buffer::empty(area).diff(&buf).len(), text.len());
        }
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        hyperlink(&mut buf, Rect::default(), "https://example.com");
    }
}
