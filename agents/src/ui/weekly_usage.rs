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

fn provider_spans(
    text: String,
    usage: &WeeklyUsage,
    bar_width: usize,
    left: bool,
) -> Line<'static> {
    let style = if 100 - usage.left_percent() > 95 {
        super::style::error_style()
    } else {
        ratatui::style::Style::default()
    };
    let filled =
        (bar_width as f64 * f64::from(100 - usage.left_percent()) / 100.0).round() as usize;
    let blocks = Span::styled("█".repeat(filled), style);
    let empty = Span::raw(" ".repeat(bar_width - filled));
    let number = Span::styled(text, style);
    if left {
        Line::from(vec![blocks, empty, Span::raw(" "), number])
    } else {
        Line::from(vec![number, Span::raw(" "), empty, blocks])
    }
}

/// Place each provider against the screen midline without covering either tab.
pub(super) fn render(
    area: Rect,
    tabs: &[(usize, Rect)],
    usage: [Option<&WeeklyUsage>; 2],
    view: UsageView,
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
    let texts = std::array::from_fn::<_, 2, _>(|i| {
        usage[i].map(|usage| provider_text(names[i], usage, view))
    });
    let widths = texts.each_ref().map(|text| {
        text.as_ref()
            .map_or(0, |text| Line::from(text.as_str()).width() as u16)
    });
    let half = if view == UsageView::Compact {
        let budget = if area.width >= super::render::WIDE_LAYOUT_COLUMNS {
            usize::from(area.width) / 2
        } else {
            usize::from(area.width) * 3 / 4
        };
        // Reserve the center cell before dividing the budget between providers.
        let half = (budget.saturating_sub(1) / 2) as u16;
        let half = available
            .iter()
            .enumerate()
            .filter(|(i, _)| usage[*i].is_some())
            .fold(half, |half, (_, space)| half.min(*space));
        if half < widths.into_iter().max().unwrap() + 1 {
            return None;
        }
        Some(half)
    } else {
        None
    };
    if (0..2).any(|i| usage[i].is_some() && half.unwrap_or(widths[i] + 1) > available[i]) {
        return None;
    }
    let mut target = Rect::new(center, area.y, 1, 1);
    for i in 0..2 {
        let Some(usage) = usage[i] else {
            continue;
        };
        let width = half.unwrap_or(widths[i] + 1);
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
        let line = if let Some(half) = half {
            let number_width = widths.into_iter().max().unwrap();
            let bar = half.saturating_sub(number_width + 2);
            let text = if i == 0 {
                format!("{text:>width$}", width = usize::from(number_width))
            } else {
                format!("{text:<width$}", width = usize::from(number_width))
            };
            provider_spans(text, usage, usize::from(bar), i == 0)
        } else {
            Line::from(text)
        };
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
