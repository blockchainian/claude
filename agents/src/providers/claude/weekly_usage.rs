use crate::providers::WeeklyUsage;
use chrono::DateTime;
use serde::Deserialize;
use std::io;
use std::process::Command;
use std::time::Duration;

#[derive(Deserialize)]
struct UsageResponse {
    limits: Vec<Limit>,
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
    WeeklyUsage::from_used_percent(limit.percent, resets_at).map(Some)
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
    let mut response = ureq::Agent::new_with_config(config)
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(io::Error::other)?;
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
        assert_eq!(usage.left_percent, 82);
        assert_eq!(
            usage.resets_at,
            Some("2026-10-23T15:59:59.800627Z".parse().unwrap())
        );
    }

    #[test]
    fn weekly_usage_rounds_fractional_percentage_and_allows_missing_reset() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["limits"][1]["percent"] = serde_json::json!(18.4);
        payload["limits"][1]["resets_at"] = serde_json::Value::Null;
        let usage = parse(&payload.to_string()).unwrap().unwrap();
        assert_eq!(usage.left_percent, 82);
        assert_eq!(usage.resets_at, None);
    }

    #[test]
    fn weekly_usage_does_not_substitute_scoped_or_session_limits() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["limits"].as_array_mut().unwrap().remove(1);
        assert!(parse(&payload.to_string()).unwrap().is_none());
    }

    #[test]
    fn weekly_usage_rejects_invalid_percentage_and_reset() {
        let mut payload: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        payload["limits"][1]["percent"] = serde_json::json!(101);
        assert!(parse(&payload.to_string()).is_err());
        payload["limits"][1]["percent"] = serde_json::json!(18);
        payload["limits"][1]["resets_at"] = serde_json::json!("invalid");
        assert!(parse(&payload.to_string()).is_err());
    }

    #[test]
    #[ignore = "reads the local keychain and live account usage"]
    fn weekly_usage_live_account() {
        let usage = read().unwrap().expect("Claude weekly usage is available");
        println!("Claude weekly {}% left", usage.left_percent);
        if let Some(reset) = usage.resets_at {
            println!(
                "Claude resets {}",
                reset.with_timezone(&chrono::Local).format("%a %H:%M")
            );
        }
    }
}
