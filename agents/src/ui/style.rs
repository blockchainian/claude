use super::*;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Whether the terminal background is light, as last reported by the terminal.
static LIGHT_BACKGROUND: AtomicBool = AtomicBool::new(false);

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

pub(super) fn rule_label_style() -> Style {
    Style::new().dim()
}

pub(super) fn selection_style() -> Style {
    #[cfg(test)]
    if let Some(style) = SELECTION_STYLE.get() {
        return style;
    }
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
    static SELECTION_STYLE: std::cell::Cell<Option<Style>> = const { std::cell::Cell::new(None) };
}

/// Supplies a terminal style for one test without changing the process environment.
#[cfg(test)]
pub(super) fn with_selection_style<T>(style: Style, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Style>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SELECTION_STYLE.set(self.0);
        }
    }
    let _restore = Restore(SELECTION_STYLE.replace(Some(style)));
    run()
}

pub(super) fn selection_style_for(no_color: bool, colors: bool, light: bool) -> Style {
    if no_color || !colors {
        return Style::default().bold().reversed();
    }
    Style {
        bg: highlight::selection_background(),
        ..Style::default()
            .bold()
            .fg(crate::theme::text_accent_for(light))
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
pub(super) fn accent_color() -> Option<Color> {
    highlight::accent_color()
}
pub(super) fn thread_color(slot: usize) -> Color {
    crate::theme::accent(slot)
}

/// Label and body styles for transcript events, with the theme accent on tools.
pub(super) fn event_styles(kind: &crate::event::Kind) -> (Style, Style) {
    use crate::event::Kind;
    let body = match kind {
        Kind::User | Kind::Compaction | Kind::QueuedUser => Style::default(),
        Kind::Assistant | Kind::Agent | Kind::Session => assistant_style(),
        Kind::Tool | Kind::QueuedSystem => Style::default().fg(crate::theme::text_accent_color()),
        Kind::Error | Kind::Denied => error_style(),
    };
    (body.bold(), body)
}

/// The assistant event body and token dots for the selected task and its descendants.
pub(super) fn assistant_style() -> Style {
    Style::default().fg(Color::Yellow)
}

/// A search match: bold, in the tool color on assistant text and the assistant color elsewhere.
pub(super) fn search_match_style(style: Style) -> Style {
    let assistant = assistant_style().fg;
    let fg = if style.fg == assistant {
        Some(crate::theme::text_accent_color())
    } else {
        assistant
    };
    Style { fg, ..style }.bold()
}

/// Numeric command status has its own error span; the summary keeps its tool color.
pub(super) fn event_body_style(kind: &crate::event::Kind, exit_code: Option<i64>) -> Style {
    let kind = if *kind == crate::event::Kind::Error && exit_code.is_some_and(|code| code != 0) {
        &crate::event::Kind::Tool
    } else {
        kind
    };
    event_styles(kind).1
}

pub(super) fn error_style() -> Style {
    highlight::scope_style("message.error")
}

pub(super) fn time_style() -> Style {
    Style::default().dim()
}

/// The other active tasks' braille dots in the usage chart.
pub(super) fn timeline_style(use_colors: bool) -> Style {
    if use_colors {
        Style::default().fg(crate::theme::inline_code_color())
    } else {
        Style::default()
    }
}

pub(super) fn event_time_style(kind: &crate::event::Kind, last: bool) -> Style {
    if last {
        event_styles(kind).1.bold()
    } else {
        time_style()
    }
}

/// The arrows that separate a tool call's input from its output.
pub(super) fn event_jump_style() -> Style {
    Style::default().bold().not_dim()
}

/// Keep a command label's style while coloring only its non-zero exit status.
pub(super) fn exit_code_line(text: String, code: Option<i64>, style: Style) -> Line<'static> {
    if let Some(code) = code.filter(|code| *code != 0) {
        let digits = code.to_string();
        if text == digits {
            return Line::from(Span::styled(
                digits,
                event_styles(&crate::event::Kind::Error).1.not_dim(),
            ));
        }
        if let Some(prefix) = text.strip_suffix(&format!(" · {digits}")) {
            return Line::from(vec![
                Span::styled(format!("{prefix} · "), style),
                Span::styled(digits, event_styles(&crate::event::Kind::Error).1.not_dim()),
            ]);
        }
    }
    Line::from(Span::styled(text, style))
}
