use std::ops::Range;
use unicode_width::UnicodeWidthStr;

pub(super) fn event_detail_rows(capture: &str, width: usize) -> Option<Range<usize>> {
    let mut rules = capture
        .lines()
        .enumerate()
        .filter(|(_, row)| row.width() == width && row.matches('─').count() > width / 2)
        .map(|(index, _)| index)
        .collect::<Vec<_>>()
        .into_iter()
        .rev();
    let bottom = rules.next()?;
    let top = rules.next()?;
    Some(top + 1..bottom)
}
