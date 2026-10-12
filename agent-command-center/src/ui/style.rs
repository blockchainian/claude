use super::*;

pub(super) fn selection_style() -> Style {
    let term = std::env::var("TERM").unwrap_or_default();
    selection_style_for(
        std::env::var_os("NO_COLOR").is_some(),
        term.contains("256color")
            || term.contains("truecolor")
            || std::env::var_os("COLORTERM").is_some(),
        std::env::var("COLORFGBG").ok().as_deref(),
    )
}

pub(super) fn selection_style_for(no_color: bool, colors: bool, colorfgbg: Option<&str>) -> Style {
    if no_color || !colors {
        return Style::default().bold().reversed();
    }
    let light = colorfgbg
        .and_then(|s| s.rsplit(';').next())
        .is_some_and(|s| s == "7" || s == "15");
    if light {
        Style::default()
            .bold()
            .fg(Color::Indexed(18))
            .bg(Color::Indexed(111))
    } else {
        Style::default()
            .bold()
            .fg(Color::Indexed(24))
            .bg(Color::Indexed(233))
    }
}
pub(super) fn active_tab_style() -> Style {
    selection_style()
}
pub(super) fn inactive_tab_style() -> Style {
    Style {
        fg: selection_style().fg,
        ..Style::default().bold()
    }
}
pub(super) fn accent_color() -> Color {
    Color::Cyan
}
pub(super) fn thread_color(id: &str) -> Color {
    let hash = id.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let palette = [
        Color::Indexed(81),
        Color::Indexed(114),
        Color::Indexed(180),
        Color::Indexed(176),
        Color::Indexed(110),
        Color::Indexed(215),
    ];
    palette[(hash % palette.len() as u64) as usize]
}
