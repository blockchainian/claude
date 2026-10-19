use crate::providers::WeeklyUsage;
use crate::state::UsageView;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::Widget,
};

pub(super) fn provider_text(name: &str, usage: &WeeklyUsage, view: UsageView) -> String {
    if view == UsageView::Compact {
        return format!("{}%", 100 - usage.left_percent());
    }
    let mut text = name.to_owned();
    if let Some(reset) = usage.resets_at {
        let reset = reset + chrono::Duration::seconds(30);
        text.push_str(&format!(
            " resets {}",
            reset.with_timezone(&chrono::Local).format("%a %H:%M")
        ));
    }
    if let Some(count) = usage.reset_credits.filter(|count| *count > 0) {
        text.push_str(&format!(
            ", {count} credit{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    text
}

fn detailed_provider_text(name: &str, usage: &WeeklyUsage, left: bool) -> String {
    let text = provider_text(name, usage, UsageView::Detailed);
    let details = text[name.len()..].trim_start_matches([' ', ',']);
    if details.is_empty() {
        name.to_owned()
    } else if left {
        format!("{details} {name}")
    } else {
        format!("{name} {details}")
    }
}

fn compact_provider(
    name: &str,
    usage: &WeeklyUsage,
    bar_width: u16,
    available: u16,
    left: bool,
    use_colors: bool,
) -> Line<'static> {
    let used = 100 - usage.left_percent();
    let number_width = format!("{used}%").len() as u16;
    let limit = available - name.len() as u16 - number_width - 3;
    let dots = ((f64::from(bar_width * 2) * f64::from(used) / 100.0).round() as usize)
        .min(usize::from(limit * 2));
    // Fill all four dot rows, mirrored at half-cell precision.
    let full = "⣿".repeat(dots / 2);
    let partial = if dots.is_multiple_of(2) {
        ""
    } else if left {
        "⢸"
    } else {
        "⡇"
    };
    let bar = if left {
        format!("{partial}{full}")
    } else {
        format!("{full}{partial}")
    };
    let empty = Span::raw(" ".repeat(usize::from(limit) - dots.div_ceil(2)));
    let number = Span::styled(
        format!("{used}%"),
        if used > 95 {
            super::style::error_style()
        } else {
            ratatui::style::Style::default()
        },
    );
    let bar = Span::styled(bar, super::style::timeline_style(use_colors));
    let label = Span::raw(name.to_owned());
    if left {
        Line::from(vec![
            empty,
            number,
            Span::raw(" "),
            bar,
            Span::raw(" "),
            label,
            Span::raw(" "),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            label,
            Span::raw(" "),
            bar,
            Span::raw(" "),
            number,
            empty,
        ])
    }
}

/// Place each provider against the screen midline without covering either tab.
pub(super) fn render(
    area: Rect,
    tabs: &[(usize, Rect)],
    usage: [Option<&WeeklyUsage>; 2],
    view: UsageView,
    use_colors: bool,
    buf: &mut Buffer,
) -> Option<Rect> {
    if area.is_empty() || usage.iter().all(Option::is_none) {
        return None;
    }
    let center = area.x + area.width / 2;
    let mut available = [center - area.x, area.right().saturating_sub(center + 1)];
    for (_, tab) in tabs {
        if tab.x <= center {
            available[0] = available[0].min(center.saturating_sub(tab.right()));
        }
        if tab.right() > center {
            available[1] = available[1].min(tab.x.saturating_sub(center + 1));
        }
    }
    let names = ["Claude", "Codex"];
    if view == UsageView::Compact {
        let budget = if area.width >= super::render::WIDE_LAYOUT_COLUMNS {
            area.width / 2
        } else {
            (usize::from(area.width) * 3 / 4) as u16
        };
        let half = ((budget.saturating_sub(1)) / 2)
            .min(available[0].saturating_sub(1))
            .min(available[1].saturating_sub(1));
        let text_widths = std::array::from_fn::<_, 2, _>(|i| {
            usage[i].map_or(0, |usage| {
                names[i].len() as u16 + provider_text(names[i], usage, view).len() as u16 + 3
            })
        });
        if text_widths.iter().any(|width| *width > half) {
            return None;
        }
        // Both providers use the same full-scale length, up to the outer edge.
        let bar_width = half.saturating_sub(
            names
                .iter()
                .map(|name| name.len() as u16 + 2)
                .max()
                .unwrap(),
        );
        for i in 0..2 {
            if let Some(usage) = usage[i] {
                let width = half;
                let side = Rect::new(
                    if i == 0 { center - width } else { center + 1 },
                    area.y,
                    width,
                    1,
                );
                compact_provider(names[i], usage, bar_width, half, i == 0, use_colors)
                    .render(side, buf);
            }
        }
        if usage.iter().all(Option::is_some) {
            buf[(center, area.y)].set_symbol("·");
        }
        return Some(Rect::new(center - half, area.y, half * 2 + 1, 1));
    }
    let texts = std::array::from_fn::<_, 2, _>(|i| {
        usage[i].map(|usage| detailed_provider_text(names[i], usage, i == 0))
    });
    let widths = texts.each_ref().map(|text| {
        text.as_ref()
            .map_or(0, |text| Line::from(text.as_str()).width() as u16)
    });
    if (0..2).any(|i| usage[i].is_some() && widths[i] + 2 > available[i]) {
        return None;
    }
    let mut target = Rect::new(center, area.y, 1, 1);
    for i in 0..2 {
        if usage[i].is_none() {
            continue;
        }
        let width = widths[i] + 1;
        let side = Rect::new(
            if i == 0 { center - width } else { center + 1 },
            area.y,
            width,
            1,
        );
        target = if i == 0 {
            Rect::new(side.x, area.y, center + 1 - side.x, 1)
        } else {
            Rect::new(target.x, area.y, side.right() - target.x, 1)
        };
        let text = texts[i].as_ref().unwrap().clone();
        let line = Line::from(text);
        let line = if i == 0 {
            line.right_aligned()
        } else {
            line.left_aligned()
        };
        let text_area = Rect::new(side.x + u16::from(i == 1), side.y, side.width - 1, 1);
        line.render(text_area, buf);
    }
    if usage.iter().all(Option::is_some) {
        Line::from("·").render(Rect::new(center, area.y, 1, 1), buf);
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detailed_labels_stay_next_to_center() {
        let reset: chrono::DateTime<chrono::Utc> = "2026-10-23T16:00:00Z".parse().unwrap();
        let time = reset
            .with_timezone(&chrono::Local)
            .format("%a %H:%M")
            .to_string();
        for resets_at in [None, Some(reset)] {
            for reset_credits in [None, Some(0), Some(1), Some(2)] {
                let usage = WeeklyUsage {
                    used_percent: 21.0,
                    resets_at,
                    reset_credits,
                };
                let mut extras = if resets_at.is_some() {
                    format!("resets {time}")
                } else {
                    String::new()
                };
                if let Some(count) = reset_credits.filter(|n| *n > 0) {
                    if !extras.is_empty() {
                        extras.push_str(", ");
                    }
                    extras.push_str(&format!(
                        "{count} credit{}",
                        if count == 1 { "" } else { "s" }
                    ));
                }
                let expected = if extras.is_empty() {
                    "Claude".to_owned()
                } else {
                    format!("{extras} Claude")
                };
                assert_eq!(detailed_provider_text("Claude", &usage, true), expected);
            }
        }
    }

    #[test]
    fn weekly_usage_rounds_reset_to_nearest_minute() {
        for (actual, rounded) in [
            ("2026-10-23T15:59:59.837Z", "2026-10-23T16:00:00Z"),
            ("2026-10-23T15:59:29.999Z", "2026-10-23T15:59:00Z"),
            ("2026-10-23T15:59:30Z", "2026-10-23T16:00:00Z"),
            ("2026-10-23T23:59:59.837Z", "2026-10-24T00:00:00Z"),
        ] {
            let reset = actual.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
            let expected = rounded
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
                .with_timezone(&chrono::Local)
                .format("%a %H:%M")
                .to_string();
            let usage = WeeklyUsage::from_used_percent(97.0, Some(reset)).unwrap();
            for name in ["Claude", "Codex"] {
                for view in [UsageView::Compact, UsageView::Detailed] {
                    assert_eq!(
                        provider_text(name, &usage, view),
                        match view {
                            UsageView::Compact => "97%".to_owned(),
                            UsageView::Detailed => format!("{name} resets {expected}"),
                        },
                        "{actual}, {view:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn weekly_usage_text_modes_thresholds_credits_and_reset_times() {
        let reset = "2026-10-23T15:59:59Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        let local = "2026-10-23T16:00:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%a %H:%M")
            .to_string();
        for left in [0, 3, 4, 5, 79, 100] {
            for reset_at in [None, Some(reset)] {
                for credits in [None, Some(0), Some(1), Some(2)] {
                    for view in [UsageView::Compact, UsageView::Detailed] {
                        let usage = WeeklyUsage {
                            used_percent: 100.0 - f64::from(left),
                            resets_at: reset_at,
                            reset_credits: credits,
                        };
                        let mut expected = match view {
                            UsageView::Compact => format!("{}%", 100 - left),
                            UsageView::Detailed => "Claude".to_owned(),
                        };
                        let has_credits = matches!(credits, Some(1 | 2));
                        if reset_at.is_some() && view == UsageView::Detailed {
                            expected.push_str(&match view {
                                UsageView::Compact => format!(" until {local}"),
                                UsageView::Detailed => format!(" resets {local}"),
                            });
                        }
                        if has_credits && view == UsageView::Detailed {
                            expected.push_str(if credits == Some(1) {
                                ", 1 credit"
                            } else {
                                ", 2 credits"
                            });
                        }
                        for name in ["Claude", "Codex"] {
                            assert_eq!(
                                provider_text(name, &usage, view),
                                expected.replace("Claude", name)
                            );
                        }
                    }
                }
            }
        }
    }
}
