use crate::providers::WeeklyUsage;
use crate::state::UsageView;

pub(super) fn provider_text(name: &str, usage: &WeeklyUsage, view: UsageView) -> String {
    let left = usage.left_percent();
    let mut text = format!("{name} {left}% left");
    let credits = usage.reset_credits.filter(|count| *count > 0);
    if (view == UsageView::Detailed || (left < 5 && credits.is_none()))
        && let Some(reset) = usage.resets_at
    {
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
    fn weekly_usage_text_modes_thresholds_credits_and_reset_times() {
        let reset = "2026-10-23T15:59:59Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        let local = reset
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
