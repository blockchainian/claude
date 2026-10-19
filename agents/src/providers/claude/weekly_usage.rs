use crate::providers::WeeklyUsage;
use chrono::DateTime;
use serde::Deserialize;
use std::io;
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
    grants: Vec<ResetGrant>,
}

#[derive(Deserialize)]
struct ResetGrant {
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

fn parse(body: &str) -> io::Result<Option<WeeklyUsage>> {
    let response: UsageResponse = serde_json::from_str(body)?;
    let Some(limit) = response
        .limits
        .into_iter()
        .find(|limit| limit.kind == "weekly_all")
    else {
        return Ok(None);
    };
    let resets_at = limit
        .resets_at
        .map(|reset| {
            DateTime::parse_from_rfc3339(&reset)
                .map(|time| time.to_utc())
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Claude weekly reset time",
                    )
                })
        })
        .transpose()?;
    let mut usage = WeeklyUsage::from_used_percent(limit.percent, resets_at)?;
    usage.reset_credits = response
        .cedar_ember
        .filter(|grants| grants.eligible)
        .map(|grants| {
            grants
                .grants
                .iter()
                .filter(|grant| grant.clears.iter().any(|limit| limit == "seven_day"))
                .try_fold(0_u64, |total, grant| {
                    total.checked_add(grant.resets_left).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "Claude reset credits overflow")
                    })
                })
        })
        .transpose()?;
    Ok(Some(usage))
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

pub(super) fn read() -> io::Result<Option<WeeklyUsage>> {
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
        .timeout_global(Some(Duration::from_secs(5)))
        .https_only(true)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut request = agent
        .get("https://api.anthropic.com/api/oauth/usage?cedar_ember=1&skip_spend=1")
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("x-app", "cli");
    if let Some(user_agent) = user_agent() {
        request = request.header("User-Agent", user_agent);
    }
    let mut response = request.call().map_err(io::Error::other)?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(io::Error::other)?;
    parse(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/claude-weekly-usage.json");

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
