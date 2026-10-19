use super::Client;
use crate::providers::{
    AccountUsage, ResetCreditOffer, ResetCreditOutcome, ResetCreditRedemption, ResetCreditRequest,
    UsageWindow, WeeklyUsage,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use std::io;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsResponse {
    rate_limits: RateLimits,
    rate_limit_reset_credits: Option<ResetCredits>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetCredits {
    available_count: u64,
    credits: Option<Vec<ResetCredit>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetCredit {
    id: String,
    expires_at: Option<i64>,
    status: String,
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
    window_duration_mins: Option<u64>,
    resets_at: Option<i64>,
}

fn timestamp(seconds: i64) -> io::Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid Codex reset time"))
}

impl RateLimitsResponse {
    fn account_usage(self) -> io::Result<AccountUsage> {
        let windows: Vec<_> = [self.rate_limits.primary, self.rate_limits.secondary]
            .into_iter()
            .flatten()
            .collect();
        let weekly = windows
            .iter()
            .find(|window| window.window_duration_mins == Some(10080))
            .map(|window| {
                if !window.used_percent.is_finite() || window.used_percent < 0.0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Codex usage percentage",
                    ));
                }
                let resets_at = window.resets_at.map(timestamp).transpose()?;
                let mut usage =
                    WeeklyUsage::from_used_percent(window.used_percent.min(100.0), resets_at)?;
                usage.reset_credits = self
                    .rate_limit_reset_credits
                    .as_ref()
                    .map(|credits| credits.available_count);
                Ok(usage)
            })
            .transpose()?;
        let mut reset_offers = Vec::new();
        if let Some(credits) = self
            .rate_limit_reset_credits
            .filter(|credits| credits.available_count > 0)
        {
            let credit = credits
                .credits
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|credit| credit.status == "available")
                .min_by_key(|credit| (credit.expires_at.is_none(), credit.expires_at));
            if let Some(credit) = credit {
                if credit.id.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing Codex reset credit id",
                    ));
                }
                let expires_at = credit.expires_at.map(timestamp).transpose()?;
                for window in windows.iter().filter(|window| window.used_percent >= 100.0) {
                    let kind = match window.window_duration_mins {
                        Some(10080) => UsageWindow::Weekly,
                        Some(300) => UsageWindow::FiveHour,
                        _ => continue,
                    };
                    let Some(resets_at) = window.resets_at else {
                        continue;
                    };
                    reset_offers.push(ResetCreditOffer {
                        window: kind,
                        resets_at: timestamp(resets_at)?,
                        credit_id: credit.id.clone(),
                        expires_at,
                    });
                }
            }
        }
        Ok(AccountUsage {
            weekly,
            reset_offers,
        })
    }

    #[cfg(test)]
    fn weekly_usage(self) -> io::Result<Option<WeeklyUsage>> {
        Ok(self.account_usage()?.weekly)
    }
}

pub(in crate::providers::codex) fn account_usage(client: &mut Client) -> io::Result<AccountUsage> {
    let response: RateLimitsResponse = client.request("account/rateLimits/read", json!({}))?;
    response.account_usage()
}

pub(in crate::providers::codex) fn weekly_usage(
    client: &mut Client,
) -> io::Result<Option<WeeklyUsage>> {
    Ok(account_usage(client)?.weekly)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ConsumeOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

impl ConsumeOutcome {
    fn into_outcome(self) -> ResetCreditOutcome {
        match self {
            Self::Reset => ResetCreditOutcome::Reset,
            Self::NothingToReset => ResetCreditOutcome::NothingToReset,
            Self::NoCredit => ResetCreditOutcome::NoCredit,
            Self::AlreadyRedeemed => ResetCreditOutcome::AlreadyRedeemed,
        }
    }
}

#[derive(Deserialize)]
struct ConsumeResponse {
    outcome: ConsumeOutcome,
}

fn consume_params(request: &ResetCreditRequest) -> serde_json::Value {
    json!({
        "idempotencyKey": request.idempotency_key,
        "creditId": request.offer.credit_id,
    })
}

pub(in crate::providers::codex) fn redeem_reset_credit(
    client: &mut Client,
    request: ResetCreditRequest,
) -> io::Result<ResetCreditRedemption> {
    let response: ConsumeResponse = client.request(
        "account/rateLimitResetCredit/consume",
        consume_params(&request),
    )?;
    Ok(ResetCreditRedemption {
        outcome: response.outcome.into_outcome(),
        reason: None,
        usage: account_usage(client)?,
    })
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
            "../../../../tests/fixtures/codex-reset-credits.json"
        ))
        .unwrap()
    }

    #[test]
    fn weekly_usage_reads_primary_fixture() {
        let usage = parse(payload()).unwrap().unwrap();
        assert_eq!(usage.left_percent(), 84);
        assert_eq!(usage.used_percent, 16.0);
        assert_eq!(usage.reset_credits, Some(2));
        assert_eq!(usage.resets_at, DateTime::from_timestamp(1792979570, 0));
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
        assert_eq!(usage.left_percent(), 82);
        assert_eq!(usage.resets_at, DateTime::from_timestamp(1792979570, 0));
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
    fn weekly_usage_allows_absent_reset_credits_and_preserves_zero() {
        let mut response = payload();
        response
            .as_object_mut()
            .unwrap()
            .remove("rateLimitResetCredits");
        assert_eq!(
            parse(response.clone()).unwrap().unwrap().reset_credits,
            None
        );
        response["rateLimitResetCredits"] = Value::Null;
        assert_eq!(
            parse(response.clone()).unwrap().unwrap().reset_credits,
            None
        );
        response["rateLimitResetCredits"] = json!({ "availableCount": 0, "credits": [] });
        assert_eq!(parse(response).unwrap().unwrap().reset_credits, Some(0));
    }

    #[test]
    fn weekly_usage_rejects_invalid_percent_and_reset() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(-1);
        assert!(parse(response).is_err());
        let mut response = payload();
        response["rateLimits"]["primary"]["resetsAt"] = json!(i64::MAX);
        assert!(parse(response).is_err());
    }

    fn parse_account(value: Value) -> io::Result<AccountUsage> {
        serde_json::from_value::<RateLimitsResponse>(value)
            .map_err(io::Error::other)?
            .account_usage()
    }

    fn check_unknown_duration(missing: bool) {
        for known_duration in [10080, 300] {
            let mut response = payload();
            response["rateLimits"]["primary"]["usedPercent"] = json!(100);
            response["rateLimits"]["secondary"] = response["rateLimits"]["primary"].clone();
            response["rateLimits"]["secondary"]["windowDurationMins"] = json!(known_duration);
            if missing {
                response["rateLimits"]["primary"]
                    .as_object_mut()
                    .unwrap()
                    .remove("windowDurationMins");
            } else {
                response["rateLimits"]["primary"]["windowDurationMins"] = Value::Null;
            }
            let usage = parse_account(response.clone()).unwrap();
            assert_eq!(usage.weekly.is_some(), known_duration == 10080);
            if let Some(weekly) = usage.weekly {
                assert_eq!(weekly.reset_credits, Some(2));
                assert_eq!(weekly.used_percent, 100.0);
            }
            assert_eq!(usage.reset_offers.len(), 1);
            assert_eq!(
                usage.reset_offers[0].window,
                if known_duration == 10080 {
                    UsageWindow::Weekly
                } else {
                    UsageWindow::FiveHour
                }
            );
            assert_eq!(usage.reset_offers[0].credit_id, "fixture-credit");
            response["rateLimits"]["secondary"] = Value::Null;
            let usage = parse_account(response).unwrap();
            assert!(usage.weekly.is_none());
            assert!(usage.reset_offers.is_empty());
        }
    }

    #[test]
    fn account_usage_ignores_null_duration() {
        check_unknown_duration(false);
    }

    #[test]
    fn account_usage_ignores_missing_duration() {
        check_unknown_duration(true);
    }

    #[test]
    fn account_usage_allows_unknown_reset_time() {
        for missing in [false, true] {
            let mut response = payload();
            response["rateLimits"]["primary"]["usedPercent"] = json!(100);
            response["rateLimits"]["secondary"] = response["rateLimits"]["primary"].clone();
            response["rateLimits"]["secondary"]["windowDurationMins"] = json!(300);
            if missing {
                response["rateLimits"]["primary"]
                    .as_object_mut()
                    .unwrap()
                    .remove("resetsAt");
            } else {
                response["rateLimits"]["primary"]["resetsAt"] = Value::Null;
            }
            let usage = parse_account(response).unwrap();
            let weekly = usage.weekly.unwrap();
            assert!(weekly.resets_at.is_none());
            assert_eq!(weekly.reset_credits, Some(2));
            assert_eq!(usage.reset_offers.len(), 1);
            assert_eq!(usage.reset_offers[0].window, UsageWindow::FiveHour);
        }
    }

    #[test]
    fn reset_offers_require_a_limited_window_and_available_credit() {
        assert!(parse_account(payload()).unwrap().reset_offers.is_empty());
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(100);
        let usage = parse_account(response.clone()).unwrap();
        assert_eq!(usage.reset_offers.len(), 1);
        assert_eq!(usage.reset_offers[0].window, UsageWindow::Weekly);
        assert_eq!(usage.reset_offers[0].credit_id, "fixture-credit");
        response["rateLimitResetCredits"]["availableCount"] = json!(0);
        assert!(parse_account(response).unwrap().reset_offers.is_empty());
    }

    #[test]
    fn reset_offers_never_choose_unavailable_or_unspecified_credits() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(100);
        for status in ["redeeming", "redeemed", "unknown"] {
            response["rateLimitResetCredits"]["credits"][0]["status"] = json!(status);
            assert!(
                parse_account(response.clone())
                    .unwrap()
                    .reset_offers
                    .is_empty()
            );
        }
        for details in [Value::Null, json!([])] {
            response["rateLimitResetCredits"]["credits"] = details;
            assert!(
                parse_account(response.clone())
                    .unwrap()
                    .reset_offers
                    .is_empty()
            );
        }
    }

    #[test]
    fn reset_offers_choose_earliest_available_expiry_and_null_last() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(100);
        let mut credit = response["rateLimitResetCredits"]["credits"][0].clone();
        credit["id"] = json!("later");
        credit["expiresAt"] = json!(1793301813);
        let mut no_expiry = credit.clone();
        no_expiry["id"] = json!("forever");
        no_expiry["expiresAt"] = Value::Null;
        let mut unavailable = credit.clone();
        unavailable["id"] = json!("redeemed");
        unavailable["status"] = json!("redeemed");
        unavailable["expiresAt"] = json!(1);
        let first = response["rateLimitResetCredits"]["credits"][0].clone();
        response["rateLimitResetCredits"]["credits"] =
            json!([no_expiry.clone(), credit, unavailable, first]);
        assert_eq!(
            parse_account(response.clone()).unwrap().reset_offers[0].credit_id,
            "fixture-credit"
        );
        response["rateLimitResetCredits"]["credits"] = json!([no_expiry]);
        assert!(
            parse_account(response).unwrap().reset_offers[0]
                .expires_at
                .is_none()
        );
    }

    #[test]
    fn five_hour_limit_offers_credit_without_a_weekly_window() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(101);
        response["rateLimits"]["primary"]["windowDurationMins"] = json!(300);
        let usage = parse_account(response).unwrap();
        assert!(usage.weekly.is_none());
        assert_eq!(usage.reset_offers[0].window, UsageWindow::FiveHour);
    }

    #[test]
    fn reset_offers_keep_both_windows_and_skip_unknown_reset_time() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(100);
        response["rateLimits"]["secondary"] = json!({
            "usedPercent": 100, "windowDurationMins": 300, "resetsAt": 1792874000
        });
        assert_eq!(
            parse_account(response.clone()).unwrap().reset_offers.len(),
            2
        );
        response["rateLimits"]["primary"]["resetsAt"] = Value::Null;
        let usage = parse_account(response).unwrap();
        assert_eq!(usage.reset_offers.len(), 1);
        assert_eq!(usage.reset_offers[0].window, UsageWindow::FiveHour);
    }

    #[test]
    fn consume_parameters_reuse_request_key_and_explicit_credit() {
        let request = ResetCreditRequest {
            offer: ResetCreditOffer {
                window: UsageWindow::Weekly,
                resets_at: DateTime::from_timestamp(1792979570, 0).unwrap(),
                credit_id: "fixture-credit".into(),
                expires_at: None,
            },
            idempotency_key: "00000000-0000-4000-8000-000000000001".into(),
        };
        let expected =
            json!({"creditId": "fixture-credit", "idempotencyKey": request.idempotency_key});
        assert_eq!(consume_params(&request), expected);
        assert_eq!(consume_params(&request), expected);
        for (raw, expected) in [
            ("reset", ResetCreditOutcome::Reset),
            ("nothingToReset", ResetCreditOutcome::NothingToReset),
            ("noCredit", ResetCreditOutcome::NoCredit),
            ("alreadyRedeemed", ResetCreditOutcome::AlreadyRedeemed),
        ] {
            let response: ConsumeResponse =
                serde_json::from_value(json!({"outcome": raw})).unwrap();
            assert_eq!(response.outcome.into_outcome(), expected);
        }
        assert!(serde_json::from_value::<ConsumeResponse>(json!({"outcome":"unknown"})).is_err());
    }

    #[test]
    fn limited_usage_above_one_hundred_is_displayed_as_full() {
        let mut response = payload();
        response["rateLimits"]["primary"]["usedPercent"] = json!(101);
        assert_eq!(parse(response).unwrap().unwrap().left_percent(), 0);
    }

    #[test]
    #[ignore = "requires the running Codex daemon and local account authentication; reads account usage only"]
    fn weekly_usage_live_account() {
        use crate::providers::{Provider, codex::CodexProvider};

        let provider = CodexProvider::from_config().unwrap();
        let usage = provider.weekly_usage().unwrap()().unwrap().unwrap();
        println!(
            "codex: weekly {}% left; resets_at={:?}",
            usage.left_percent(),
            usage.resets_at
        );
    }
}
