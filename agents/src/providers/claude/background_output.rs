use crate::event::Section;
use std::path::Path;

/// Reads the current background output, retaining message-specific formatting.
pub(crate) fn read(path: &Path) -> Section {
    let text = match super::super::tool::read_plain_output_file(path) {
        Section::Text { text, .. } => text,
        section => return section,
    };
    let records = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
        .filter(|records| {
            records.iter().all(|record| record["type"].is_string())
                && records.iter().any(|record| {
                    matches!(record["type"].as_str(), Some("assistant" | "user"))
                        && record["message"]["role"] == record["type"]
                        && record["message"]["content"].is_array()
                })
        });
    let Some(records) = records else {
        return super::codex_manager::await_output::section(text);
    };
    let handback = records
        .iter()
        .filter(|record| record["type"] == "assistant")
        .filter_map(|record| record["message"]["content"].as_array())
        .flatten()
        .rfind(|block| block["type"] == "tool_use" && block["name"] == "SubagentHandback")
        .and_then(|block| block["input"]["message"].as_str())
        .map(str::to_owned);
    let message = handback.or_else(|| {
        let last = records.last()?;
        if last["type"] != "assistant" {
            return None;
        }
        last["message"]["content"]
            .as_array()?
            .iter()
            .map(|block| {
                (block["type"] == "text")
                    .then(|| block["text"].as_str())
                    .flatten()
            })
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join("\n"))
    });
    Section::Markdown {
        label: String::new(),
        text: message.unwrap_or_default(),
    }
}
