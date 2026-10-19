use super::Client;
use crate::providers::WeeklyUsage;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use std::io;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsResponse {
    rate_limits: RateLimits,
}

#[derive(Deserialize)]
struct RateLimits {
    primary: Option<Window>,
    secondary: Option<Window>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    used_percent: f64,
    window_duration_mins: u64,
    resets_at: Option<i64>,
}

impl RateLimitsResponse {
    fn weekly_usage(self) -> io::Result<Option<WeeklyUsage>> {
        let Some(window) = [self.rate_limits.primary, self.rate_limits.secondary]
            .into_iter()
            .flatten()
            .find(|window| window.window_duration_mins == 10080)
        else {
            return Ok(None);
        };
        let resets_at = window
            .resets_at
            .map(|seconds| {
                DateTime::<Utc>::from_timestamp(seconds, 0).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Codex weekly reset time",
                    )
                })
            })
            .transpose()?;
        WeeklyUsage::from_used_percent(window.used_percent, resets_at).map(Some)
    }
}

pub(in crate::providers::codex) fn weekly_usage(
    client: &mut Client,
) -> io::Result<Option<WeeklyUsage>> {
    let response: RateLimitsResponse = client.request("account/rateLimits/read", json!({}))?;
    response.weekly_usage()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use serde_json::{Value, json};

    fn parse(value: Value) -> io::Result<Option<WeeklyUsage>> {
        let response: RateLimitsResponse =
            serde_json::from_value(value).map_err(io::Error::other)?;
        response.weekly_usage()
    }

    fn payload() -> Value {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-weekly-usage.json"
        ))
        .unwrap()
    }

    #[test]
    fn weekly_usage_reads_primary_fixture() {
        let usage = parse(payload()).unwrap().unwrap();
        assert_eq!(usage.left_percent, 2);
        assert_eq!(usage.resets_at, DateTime::from_timestamp(1792874440, 0));
    }

    #[test]
    fn weekly_usage_selects_secondary_by_duration_and_rounds_fractional_percent() {
        let mut response = payload();
        response["rateLimits"]["secondary"] = response["rateLimits"]["primary"].take();
        response["rateLimits"]["primary"] = json!({
            "usedPercent": 7, "windowDurationMins": 300, "resetsAt": 1792874000
        });
        response["rateLimits"]["secondary"]["usedPercent"] = json!(17.6);
        let usage = parse(response).unwrap().unwrap();
        assert_eq!(usage.left_percent, 82);
        assert_eq!(usage.resets_at, DateTime::from_timestamp(1792874440, 0));
    }

    #[test]
    fn weekly_usage_ignores_other_windows_and_allows_absent_reset() {
        let mut response = payload();
        response["rateLimits"]["primary"]["windowDurationMins"] = json!(300);
        assert!(parse(response.clone()).unwrap().is_none());
        response["rateLimits"]["primary"]["windowDurationMins"] = json!(10080);
        response["rateLimits"]["primary"]["resetsAt"] = Value::Null;
        assert!(parse(response).unwrap().unwrap().resets_at.is_none());
    }

    #[test]
    fn weekly_usage_rejects_invalid_percent_and_reset() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(101);
        assert!(parse(response).is_err());
        let mut response = payload();
        response["rateLimits"]["primary"]["resetsAt"] = json!(i64::MAX);
        assert!(parse(response).is_err());
    }

    #[test]
    #[ignore = "requires the running Codex daemon and local account authentication; reads account usage only"]
    fn weekly_usage_live_account() {
        use crate::providers::{Provider, codex::CodexProvider};

        let provider = CodexProvider::from_config().unwrap();
        let usage = provider.weekly_usage().unwrap()().unwrap().unwrap();
        println!(
            "codex: weekly {}% left; resets_at={:?}",
            usage.left_percent, usage.resets_at
        );
    }
}
