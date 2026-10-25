use crate::providers::{
    AccountUsage, ResetCreditOffer, ResetCreditOutcome, ResetCreditRedemption, ResetCreditRequest,
    UsageWindow, WeeklyUsage,
};
use chrono::DateTime;
use serde::Deserialize;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Deserialize)]
struct UsageResponse {
    limits: Vec<Limit>,
    cedar_ember: Option<ResetGrants>,
}

#[derive(Deserialize)]
struct ResetGrants {
    eligible: bool,
    #[serde(default)]
    at_limit: bool,
    #[serde(default)]
    exhausted: Vec<String>,
    next_grant_id: Option<String>,
    grants: Vec<ResetGrant>,
}

#[derive(Deserialize)]
struct ResetGrant {
    id: String,
    usable_now: bool,
    paused: bool,
    ends_at: Option<String>,
    clears: Vec<String>,
    resets_left: u64,
}

#[derive(Deserialize)]
struct Limit {
    kind: String,
    percent: f64,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<Oauth>,
}

#[derive(Deserialize)]
struct Oauth {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
}

fn parse_time(value: &str) -> io::Result<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.to_utc())
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid Claude usage reset time",
            )
        })
}

fn parse_account_usage(body: &str) -> io::Result<AccountUsage> {
    let response: UsageResponse = serde_json::from_str(body)?;
    let weekly = response
        .limits
        .iter()
        .find(|limit| limit.kind == "weekly_all")
        .map(|limit| {
            let mut usage = WeeklyUsage::from_used_percent(
                limit.percent,
                limit.resets_at.as_deref().map(parse_time).transpose()?,
            )?;
            usage.reset_credits = response
                .cedar_ember
                .as_ref()
                .filter(|grants| grants.eligible)
                .map(|grants| {
                    grants
                        .grants
                        .iter()
                        .filter(|grant| grant.clears.iter().any(|limit| limit == "seven_day"))
                        .try_fold(0_u64, |total, grant| {
                            total.checked_add(grant.resets_left).ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "Claude reset credits overflow",
                                )
                            })
                        })
                })
                .transpose()?;
            Ok::<_, io::Error>(usage)
        })
        .transpose()?;
    let mut reset_offers = Vec::new();
    if let Some(status) = response
        .cedar_ember
        .as_ref()
        .filter(|status| status.eligible && status.at_limit)
        && let Some(grant) = status.grants.iter().find(|grant| {
            Some(grant.id.as_str()) == status.next_grant_id.as_deref()
                && grant.usable_now
                && !grant.paused
                && grant.resets_left > 0
        })
    {
        let expires_at = grant.ends_at.as_deref().map(parse_time).transpose()?;
        for (kind, window, clear) in [
            ("session", UsageWindow::FiveHour, "five_hour"),
            ("weekly_all", UsageWindow::Weekly, "seven_day"),
        ] {
            let Some(limit) = response.limits.iter().find(|limit| limit.kind == kind) else {
                continue;
            };
            if !grant.clears.iter().any(|value| value == clear)
                || !(status.exhausted.iter().any(|value| value == clear) || limit.percent >= 100.0)
            {
                continue;
            }
            let Some(reset) = limit.resets_at.as_deref() else {
                continue;
            };
            reset_offers.push(ResetCreditOffer {
                window,
                resets_at: parse_time(reset)?,
                credit_id: grant.id.clone(),
                expires_at,
            });
        }
    }
    Ok(AccountUsage {
        weekly,
        reset_offers,
    })
}

#[cfg(test)]
#[cfg(test)]
fn parse(body: &str) -> io::Result<Option<WeeklyUsage>> {
    Ok(parse_account_usage(body)?.weekly)
}

fn user_agent() -> Option<&'static str> {
    static USER_AGENT: OnceLock<Option<String>> = OnceLock::new();
    USER_AGENT
        .get_or_init(|| {
            let output = Command::new("claude").arg("--version").output().ok()?;
            if !output.status.success() {
                return None;
            }
            let version_output = String::from_utf8(output.stdout).ok()?;
            let version = version_output.split_whitespace().next()?;
            Some(format!("claude-cli/{version} (external, cli)"))
        })
        .as_deref()
}

struct Client {
    agent: ureq::Agent,
    token: String,
}

impl Client {
    fn load(timeout: Duration) -> io::Result<Option<Self>> {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ])
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        let credentials: Credentials = serde_json::from_slice(&output.stdout)?;
        let Some(token) = credentials
            .oauth
            .and_then(|oauth| oauth.access_token)
            .filter(|token| !token.is_empty())
        else {
            return Ok(None);
        };
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .https_only(true)
            .build();
        Ok(Some(Self {
            agent: ureq::Agent::new_with_config(config),
            token,
        }))
    }

    fn headers<B>(&self, mut request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        request = request
            .header("Authorization", format!("Bearer {}", self.token))
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("x-app", "cli");
        if let Some(user_agent) = user_agent() {
            request = request.header("User-Agent", user_agent);
        }
        request
    }

    fn read_usage(&self) -> io::Result<AccountUsage> {
        let mut response = self
            .headers(
                self.agent
                    .get("https://api.anthropic.com/api/oauth/usage?cedar_ember=1&skip_spend=1"),
            )
            .call()
            .map_err(io::Error::other)?;
        parse_account_usage(
            &response
                .body_mut()
                .read_to_string()
                .map_err(io::Error::other)?,
        )
    }
}

pub(super) fn read_account_usage() -> io::Result<AccountUsage> {
    match Client::load(Duration::from_secs(5))? {
        Some(client) => client.read_usage(),
        None => Ok(AccountUsage {
            weekly: None,
            reset_offers: Vec::new(),
        }),
    }
}

pub(super) fn read() -> io::Result<Option<WeeklyUsage>> {
    Ok(read_account_usage()?.weekly)
}

fn redemption_outcome(body: &str) -> io::Result<ResetCreditOutcome> {
    #[derive(Deserialize)]
    struct Response {
        result: String,
    }
    match serde_json::from_str::<Response>(body)?.result.as_str() {
        "reset" => Ok(ResetCreditOutcome::Reset),
        "already_used" => Ok(ResetCreditOutcome::AlreadyRedeemed),
        "not_limited" => Ok(ResetCreditOutcome::NothingToReset),
        "cooldown" => Ok(ResetCreditOutcome::Cooldown),
        "ineligible" => Ok(ResetCreditOutcome::Ineligible),
        "unavailable" => Ok(ResetCreditOutcome::Unavailable),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unknown Claude reset result",
        )),
    }
}

fn redemption_body(request: &ResetCreditRequest) -> serde_json::Value {
    serde_json::json!({"program": "cedar_ember", "grant_id": request.offer.credit_id,
        "request_id": request.idempotency_key})
}

pub(super) fn redeem(
    config_dir: &Path,
    request: ResetCreditRequest,
) -> io::Result<ResetCreditRedemption> {
    let client = Client::load(Duration::from_secs(25))?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Claude OAuth token unavailable",
        )
    })?;
    let org = super::account::read(config_dir)?.organization_uuid;
    if org.is_empty()
        || !org
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid Claude organization UUID",
        ));
    }
    let body = serde_json::to_string(&redemption_body(&request))?;
    let mut response = client
        .headers(client.agent.post(format!(
            "https://api.anthropic.com/api/organizations/{org}/reset_rate_limits"
        )))
        .header("Content-Type", "application/json")
        .send(body.as_str())
        .map_err(io::Error::other)?;
    let result = response
        .body_mut()
        .read_to_string()
        .map_err(io::Error::other)
        .and_then(|body| {
            let value: serde_json::Value = serde_json::from_str(&body)?;
            let reason = value
                .get("reason")
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_str().map(str::to_owned).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "Invalid Claude reset reason")
                    })
                })
                .transpose()?;
            Ok((redemption_outcome(&body)?, reason))
        });
    let usage = client.read_usage();
    let (outcome, reason) = result?;
    Ok(ResetCreditRedemption {
        outcome,
        reason,
        usage: usage?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/claude-weekly-usage.json");

    fn limited_payload() -> serde_json::Value {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["cedar_ember"]["at_limit"] = serde_json::json!(true);
        payload["cedar_ember"]["exhausted"] = serde_json::json!(["seven_day"]);
        payload["limits"][0]["percent"] = serde_json::json!(100);
        payload
    }

    #[test]
    fn reset_offer_uses_only_the_next_usable_grant() {
        let mut payload = limited_payload();
        let usage = parse_account_usage(&payload.to_string()).unwrap();
        assert_eq!(usage.reset_offers.len(), 1);
        let offer = &usage.reset_offers[0];
        assert_eq!(offer.window, UsageWindow::Weekly);
        assert_eq!(offer.credit_id, "opus55-launch-promax-20260921");
        assert_eq!(
            offer.expires_at,
            Some("2026-10-22T16:00:00Z".parse().unwrap())
        );
        for (field, value) in [
            ("usable_now", serde_json::json!(false)),
            ("paused", serde_json::json!(true)),
            ("resets_left", serde_json::json!(0)),
            ("clears", serde_json::json!(["five_hour"])),
        ] {
            let mut changed = payload.clone();
            changed["cedar_ember"]["grants"][0][field] = value;
            assert!(
                parse_account_usage(&changed.to_string())
                    .unwrap()
                    .reset_offers
                    .is_empty(),
                "{field}"
            );
        }
        payload["cedar_ember"]["next_grant_id"] = serde_json::json!("different");
        assert!(
            parse_account_usage(&payload.to_string())
                .unwrap()
                .reset_offers
                .is_empty()
        );
    }

    #[test]
    fn reset_offer_requires_at_limit_and_identifies_five_hour_window() {
        let mut payload = limited_payload();
        payload["limits"][0]["percent"] = serde_json::json!(21);
        payload["limits"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "kind": "session", "group": "session", "percent": 100,
                "resets_at": "2026-10-19T20:00:00Z", "scope": null, "is_active": true
            }));
        payload["cedar_ember"]["exhausted"] = serde_json::json!(["five_hour"]);
        let usage = parse_account_usage(&payload.to_string()).unwrap();
        assert_eq!(usage.reset_offers.len(), 1);
        assert_eq!(usage.reset_offers[0].window, UsageWindow::FiveHour);
        assert_eq!(
            usage.reset_offers[0].resets_at,
            "2026-10-19T20:00:00Z"
                .parse::<DateTime<chrono::Utc>>()
                .unwrap()
        );
        payload["cedar_ember"]["at_limit"] = serde_json::json!(false);
        assert!(
            parse_account_usage(&payload.to_string())
                .unwrap()
                .reset_offers
                .is_empty()
        );
    }

    #[test]
    fn reset_offer_uses_limit_percentage_when_exhausted_is_empty() {
        let mut payload = limited_payload();
        payload["cedar_ember"]["exhausted"] = serde_json::json!([]);
        assert_eq!(
            parse_account_usage(&payload.to_string())
                .unwrap()
                .reset_offers
                .len(),
            1
        );
        payload["limits"][0]["percent"] = serde_json::json!(99);
        assert!(
            parse_account_usage(&payload.to_string())
                .unwrap()
                .reset_offers
                .is_empty()
        );
    }

    #[test]
    fn reset_offer_does_not_require_weekly_display() {
        let mut payload = limited_payload();
        payload["limits"][0]["kind"] = serde_json::json!("session");
        payload["cedar_ember"]["exhausted"] = serde_json::json!(["five_hour"]);
        let usage = parse_account_usage(&payload.to_string()).unwrap();
        assert!(usage.weekly.is_none());
        assert_eq!(usage.reset_offers.len(), 1);
        payload["limits"][0]["resets_at"] = serde_json::Value::Null;
        assert!(
            parse_account_usage(&payload.to_string())
                .unwrap()
                .reset_offers
                .is_empty()
        );
    }

    #[test]
    fn reset_request_preserves_grant_and_idempotency_key() {
        let offer = parse_account_usage(&limited_payload().to_string())
            .unwrap()
            .reset_offers
            .remove(0);
        let request = ResetCreditRequest {
            offer,
            idempotency_key: "00000000-0000-4000-8000-000000000001".into(),
        };
        let expected = serde_json::json!({"program":"cedar_ember", "grant_id":"opus55-launch-promax-20260921",
            "request_id":"00000000-0000-4000-8000-000000000001"});
        assert_eq!(redemption_body(&request), expected);
        assert_eq!(redemption_body(&request), expected);
    }

    #[test]
    fn reset_response_handles_every_documented_outcome() {
        for (result, expected) in [
            ("reset", ResetCreditOutcome::Reset),
            ("already_used", ResetCreditOutcome::AlreadyRedeemed),
            ("not_limited", ResetCreditOutcome::NothingToReset),
            ("cooldown", ResetCreditOutcome::Cooldown),
            ("ineligible", ResetCreditOutcome::Ineligible),
            ("unavailable", ResetCreditOutcome::Unavailable),
        ] {
            let body = serde_json::json!({"result":result}).to_string();
            assert_eq!(redemption_outcome(&body).unwrap(), expected);
        }
        assert!(redemption_outcome(r#"{"result":"unknown"}"#).is_err());
    }

    #[test]
    fn weekly_usage_selects_account_limit_and_parses_reset() {
        let usage = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(usage.left_percent(), 79);
        assert_eq!(
            usage.resets_at,
            Some("2026-10-23T15:59:59.837616Z".parse().unwrap())
        );
    }

    #[test]
    fn weekly_usage_reads_weekly_reset_credits() {
        let usage = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(usage.reset_credits, Some(1));
    }

    #[test]
    fn weekly_usage_sums_only_grants_that_clear_seven_day() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let mut grant = payload["cedar_ember"]["grants"][0].clone();
        grant["resets_left"] = serde_json::json!(2);
        payload["cedar_ember"]["grants"]
            .as_array_mut()
            .unwrap()
            .push(grant.clone());
        grant["clears"] = serde_json::json!(["five_hour"]);
        payload["cedar_ember"]["grants"]
            .as_array_mut()
            .unwrap()
            .push(grant);
        let usage = parse(&payload.to_string()).unwrap().unwrap();
        assert_eq!(usage.reset_credits, Some(3));
    }

    #[test]
    fn weekly_usage_preserves_zero_reset_credits() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["cedar_ember"]["grants"][0]["resets_left"] = serde_json::json!(0);
        assert_eq!(
            parse(&payload.to_string()).unwrap().unwrap().reset_credits,
            Some(0)
        );
        payload["cedar_ember"]["grants"]
            .as_array_mut()
            .unwrap()
            .clear();
        assert_eq!(
            parse(&payload.to_string()).unwrap().unwrap().reset_credits,
            Some(0)
        );
    }

    #[test]
    fn weekly_usage_omits_ineligible_and_missing_reset_credits() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["cedar_ember"]["eligible"] = serde_json::json!(false);
        assert_eq!(
            parse(&payload.to_string()).unwrap().unwrap().reset_credits,
            None
        );
        payload["cedar_ember"] = serde_json::Value::Null;
        assert_eq!(
            parse(&payload.to_string()).unwrap().unwrap().reset_credits,
            None
        );
        payload.as_object_mut().unwrap().remove("cedar_ember");
        assert_eq!(
            parse(&payload.to_string()).unwrap().unwrap().reset_credits,
            None
        );
    }

    #[test]
    fn weekly_usage_rounds_fractional_percentage_and_allows_missing_reset() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["limits"][0]["percent"] = serde_json::json!(18.4);
        payload["limits"][0]["resets_at"] = serde_json::Value::Null;
        let usage = parse(&payload.to_string()).unwrap().unwrap();
        assert_eq!(usage.left_percent(), 82);
        assert_eq!(usage.resets_at, None);
    }

    #[test]
    fn weekly_usage_does_not_substitute_scoped_or_session_limits() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        for kind in ["weekly_scoped", "session"] {
            payload["limits"][0]["kind"] = serde_json::json!(kind);
            assert!(parse(&payload.to_string()).unwrap().is_none());
        }
        payload["limits"].as_array_mut().unwrap().clear();
        assert!(parse(&payload.to_string()).unwrap().is_none());
    }

    #[test]
    fn weekly_usage_rejects_invalid_percentage_and_reset() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["limits"][0]["percent"] = serde_json::json!(101);
        assert!(parse(&payload.to_string()).is_err());
        payload["limits"][0]["percent"] = serde_json::json!(18);
        payload["limits"][0]["resets_at"] = serde_json::json!("invalid");
        assert!(parse(&payload.to_string()).is_err());
    }

    #[test]
    #[ignore = "reads the local keychain and live account usage"]
    fn weekly_usage_live_account() {
        let usage = read().unwrap().expect("Claude weekly usage is available");
        println!("Claude weekly {}% left", usage.left_percent());
        if let Some(reset) = usage.resets_at {
            println!(
                "Claude resets {}",
                reset.with_timezone(&chrono::Local).format("%a %H:%M")
            );
        }
    }
}
