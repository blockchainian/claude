use super::*;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Whether the terminal background is light, as last reported by the terminal.
static LIGHT_BACKGROUND: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub fn light_background() -> bool {
    LIGHT_BACKGROUND.load(Ordering::Relaxed)
}

pub fn set_light_background(light: bool) {
    LIGHT_BACKGROUND.store(light, Ordering::Relaxed);
}

fn terminal_style(style_for: fn(bool, bool, bool) -> Style) -> Style {
    #[cfg(test)]
    return style_for(false, true, false);

    #[cfg(not(test))]
    {
        let term = std::env::var("TERM").unwrap_or_default();
        style_for(
            std::env::var_os("NO_COLOR").is_some(),
            term.contains("256color")
                || term.contains("truecolor")
                || std::env::var_os("COLORTERM").is_some(),
            LIGHT_BACKGROUND.load(Ordering::Relaxed),
        )
    }
}

#[cfg(not(test))]
pub(super) fn selection_style() -> Style {
    terminal_style(selection_style_for)
}

pub(super) fn attention_title_style() -> Style {
    terminal_style(attention_title_style_for)
}

pub(super) fn attention_title_style_for(no_color: bool, colors: bool, light: bool) -> Style {
    let style = Style::default().bold();
    if no_color || !colors {
        style
    } else {
        style.fg(if light { Color::Red } else { Color::LightRed })
    }
}

#[cfg(test)]
thread_local! {
    static SELECTION_STYLE: std::cell::Cell<Style> =
        std::cell::Cell::new(selection_style_for(false, true, false));
}

#[cfg(test)]
pub(super) fn selection_style() -> Style {
    SELECTION_STYLE.get()
}

/// Supplies a terminal style for one test without changing the process environment.
#[cfg(test)]
pub(super) fn with_selection_style<T>(style: Style, run: impl FnOnce() -> T) -> T {
    struct Restore(Style);
    impl Drop for Restore {
        fn drop(&mut self) {
            SELECTION_STYLE.set(self.0);
        }
    }
    let _restore = Restore(SELECTION_STYLE.replace(style));
    run()
}

pub(super) fn selection_style_for(no_color: bool, colors: bool, light: bool) -> Style {
    if no_color || !colors {
        return Style::default().bold().reversed();
    }
    if light {
        Style::default()
            .bold()
            .fg(crate::theme::text_accent_for(true))
            .bg(Color::Indexed(111))
    } else {
        Style::default()
            .bold()
            .fg(crate::theme::text_accent_for(false))
            .bg(Color::Indexed(233))
    }
}
pub(super) fn active_tab_style() -> Style {
    selection_style()
}

/// Highlights a cursor row without replacing its cells' foreground colors.
pub(super) fn cursor_style() -> Style {
    Style {
        fg: None,
        ..selection_style()
    }
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
pub(super) fn thread_color(slot: usize) -> Color {
    crate::theme::accent(slot)
}

/// Label and body styles for transcript events, with the theme accent on tools.
pub(super) fn log_styles(kind: &crate::log::Kind) -> (Style, Style) {
    use crate::log::Kind;
    let body = match kind {
        Kind::User | Kind::Compaction | Kind::QueuedUser => Style::default(),
        Kind::Assistant | Kind::Agent | Kind::Session => Style::default().fg(Color::Yellow),
        Kind::Tool | Kind::QueuedSystem => Style::default().fg(crate::theme::text_accent_color()),
        Kind::Error | Kind::Denied => Style::default().fg(Color::Red),
    };
    (body.bold(), body)
}

pub(super) fn time_style() -> Style {
    Style::default().dim()
}

/// The braille dots in the usage chart and activity strip.
pub(super) fn timeline_style(use_colors: bool) -> Style {
    if use_colors {
        Style::default().fg(crate::theme::inline_code_color())
    } else {
        Style::default()
    }
}

pub(super) fn log_time_style(kind: &crate::log::Kind, last: bool) -> Style {
    if last {
        log_styles(kind).1.bold()
    } else {
        time_style()
    }
}

/// The arrows that separate a tool call's input from its output.
pub(super) fn log_jump_style() -> Style {
    Style::default().bold().not_dim()
}
