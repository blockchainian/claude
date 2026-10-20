use crate::event::Kind;
use serde_json::Value;

type Message<'a> = (Option<&'a str>, &'a str);

fn report_body(body: &str) -> &str {
    let body = body.trim();
    if body.starts_with("[Subagent hand-back]") {
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            offset += line.len();
            if line.trim_end().ends_with("The report follows:") {
                return body[offset..].trim();
            }
        }
    }
    body
}

pub(super) fn origin_message(record: &Value) -> Option<Message<'_>> {
    let origin = &record["origin"];
    if origin["kind"] != "peer" {
        return None;
    }
    Some((
        origin["name"].as_str(),
        report_body(origin["body"].as_str()?),
    ))
}

/// Extracts peer text without changing the body's internal whitespace.
pub(super) fn wrapped_message(text: &str) -> Option<(Kind, Message<'_>)> {
    let text = text.trim();
    let text = text
        .strip_prefix("Another Claude session sent a message:")
        .unwrap_or(text)
        .trim_start();
    let (opening, rest) = text.strip_prefix('<')?.split_once('>')?;
    let tag = opening.split_whitespace().next()?;
    let kind = match tag {
        "agent-message" | "teammate-message" => Kind::Agent,
        "cross-session-message" => Kind::Session,
        _ => return None,
    };
    let end = rest.rfind(&format!("</{tag}>"))?;
    let attribute = |key: &str| {
        opening
            .split_once(&format!(" {key}=\""))?
            .1
            .split_once('"')
            .map(|(value, _)| value)
    };
    let name = match tag {
        "cross-session-message" => Some(attribute("from-name")?),
        "teammate-message" => attribute("teammate_id"),
        _ => attribute("from-name").or_else(|| attribute("from")),
    };
    Some((kind, (name, report_body(&rest[..end]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_wrappers_share_bodies_with_origin_and_ignore_the_trailer() {
        for line in include_str!("../../../tests/fixtures/claude-peer-prompts.jsonl").lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            let text = record["message"]["content"].as_str().unwrap();
            let (kind, (name, body)) = wrapped_message(text).unwrap();
            assert!(name.is_some());
            assert!(!body.contains("permission laundering"));
            if let Some((_, origin_body)) = origin_message(&record) {
                assert_eq!(body, origin_body);
            } else {
                assert_eq!(kind, Kind::Agent);
                assert_eq!(name, Some("codex-support-research"));
                assert!(body.ends_with("\"done\"}"));
            }
            let opening = text.find('<').unwrap();
            assert_eq!(wrapped_message(&text[opening..]), wrapped_message(text));
        }
    }
}
