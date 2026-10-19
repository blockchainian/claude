use crate::providers::WeeklyUsage;
use crate::state::UsageView;

pub(super) fn provider_text(name: &str, usage: &WeeklyUsage, view: UsageView) -> String {
    let left = usage.left_percent();
    let mut text = format!("{name} {left}% left");
    let credits = usage.reset_credits.filter(|count| *count > 0);
    if (view == UsageView::Detailed || (left < 5 && credits.is_none()))
        && let Some(reset) = usage.resets_at
    {
        let reset = reset + chrono::Duration::seconds(30);
        text.push_str(&format!(
            " until {}",
            reset.with_timezone(&chrono::Local).format("%a %H:%M")
        ));
    }
    if (view == UsageView::Detailed || left < 5)
        && let Some(count) = credits
    {
        text.push_str(&format!(
            ", {count} reset{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    text
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
                        format!("{name} 3% left until {expected}"),
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
                        let mut expected = format!("Claude {left}% left");
                        let has_credits = matches!(credits, Some(1 | 2));
                        if reset_at.is_some()
                            && (view == UsageView::Detailed || (left < 5 && !has_credits))
                        {
                            expected.push_str(&format!(" until {local}"));
                        }
                        if has_credits && (view == UsageView::Detailed || left < 5) {
                            expected.push_str(if credits == Some(1) {
                                ", 1 reset"
                            } else {
                                ", 2 resets"
                            });
                        }
                        assert_eq!(provider_text("Claude", &usage, view), expected);
                    }
                }
            }
        }
    }
}
