use super::*;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Whether the terminal background is light, as the terminal reported at startup.
static LIGHT_BACKGROUND: AtomicBool = AtomicBool::new(false);

pub fn set_light_background(light: bool) {
    LIGHT_BACKGROUND.store(light, Ordering::Relaxed);
}

pub(super) fn selection_style() -> Style {
    let term = std::env::var("TERM").unwrap_or_default();
    selection_style_for(
        std::env::var_os("NO_COLOR").is_some(),
        term.contains("256color")
            || term.contains("truecolor")
            || std::env::var_os("COLORTERM").is_some(),
        LIGHT_BACKGROUND.load(Ordering::Relaxed),
    )
}

pub(super) fn selection_style_for(no_color: bool, colors: bool, light: bool) -> Style {
    if no_color || !colors {
        return Style::default().bold().reversed();
    }
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
    crate::theme::color_for_id(id)
}

/// Label and body styles for transcript events, using terminal palette colors.
pub(super) fn log_styles(kind: &crate::sessions::log::Kind) -> (Style, Style) {
    use crate::sessions::log::Kind;
    let body = match kind {
        Kind::User => Style::default(),
        Kind::Model => Style::default().fg(Color::Yellow),
        Kind::Tool | Kind::Queued => Style::default().fg(Color::Blue),
        Kind::Error => Style::default().fg(Color::Red),
    };
    (body.bold(), body)
}
