use crate::log::{Section, ToolCall};
use serde_json::Value;

pub(crate) fn json_input(value: &Value) -> Vec<Section> {
    if value.is_null() || value.as_object().is_some_and(|object| object.is_empty()) {
        Vec::new()
    } else {
        vec![Section::Json {
            label: "input".into(),
            text: value.to_string(),
        }]
    }
}

/// Decode MCP and Anthropic content blocks without exposing binary payloads.
pub(crate) fn mcp_content(content: &Value) -> Vec<Section> {
    if let Some(text) = content.as_str() {
        return vec![Section::Text {
            label: "output".into(),
            text: text.into(),
        }];
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .map(|block| {
            if let Some(text) = block["text"].as_str().filter(|_| block["type"] == "text") {
                return Section::Text {
                    label: "output".into(),
                    text: text.into(),
                };
            }
            let kind = block["type"].as_str().unwrap_or("unknown");
            let mime = block["mimeType"]
                .as_str()
                .or_else(|| block["source"]["media_type"].as_str())
                .or_else(|| block["resource"]["mimeType"].as_str());
            Section::Placeholder(match mime {
                Some(mime) => format!("[{kind} {mime}]"),
                None => format!("[{kind}]"),
            })
        })
        .collect()
}

pub(crate) fn display_tool_name(name: &str) -> String {
    let Some((server, tool)) = name
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
    else {
        return name
            .split_once('.')
            .filter(|(server, tool)| !server.is_empty() && !tool.is_empty())
            .map(|(server, tool)| format!("{server} {tool}"))
            .unwrap_or_else(|| name.to_owned());
    };
    let server = if let Some(rest) = server.strip_prefix("plugin_") {
        match rest.split_once('_') {
            Some((plugin, server)) if !plugin.is_empty() => server,
            _ => return name.to_owned(),
        }
    } else {
        server
    };
    if server.is_empty() || tool.is_empty() || tool.contains("__") {
        name.to_owned()
    } else {
        [server, tool].join(" ")
    }
}

pub(crate) struct Rules {
    pub subjects: &'static [(&'static str, &'static str)],
    pub inputs: &'static [(&'static str, &'static str)],
}

const MCP_RULES: Rules = Rules {
    subjects: &[
        ("codex-manager start", "name"),
        ("codex-manager send", "threadId"),
        ("codex-manager interrupt", "threadId"),
        ("codex-manager detach", "threadId"),
    ],
    inputs: &[
        ("codex-manager start", "prompt"),
        ("codex-manager send", "prompt"),
    ],
};

pub(crate) fn presentation(
    name: &str,
    input: &Value,
    rules: &Rules,
) -> (String, Option<String>, Vec<Section>) {
    let name = display_tool_name(name);
    let find = |table: &[(&str, &'static str)]| {
        table
            .iter()
            .find_map(|(tool, field)| (*tool == name).then_some(*field))
    };
    let mut remaining = input.clone();
    let subject = find(rules.subjects)
        .or_else(|| find(MCP_RULES.subjects))
        .and_then(|field| {
            remaining
                .as_object_mut()
                .and_then(|object| object.remove(field))
                .and_then(|value| {
                    value
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(str::to_owned)
                })
        });
    let input = match find(rules.inputs).or_else(|| find(MCP_RULES.inputs)) {
        Some("") => Vec::new(),
        Some(field) => remaining
            .get(field)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                vec![Section::Prose {
                    text: text.to_owned(),
                    markdown: false,
                }]
            })
            .unwrap_or_default(),
        None => json_input(&remaining),
    };
    (name, subject, input)
}

#[cfg(test)]
pub(crate) fn question_texts(input: &Value) -> Vec<String> {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|question| question["question"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Add explicit headings for separately reported stderr output.
pub(super) fn annotate(call: &mut ToolCall) {
    if let Some(output) = &mut call.output {
        *output = std::mem::take(output)
            .into_iter()
            .flat_map(label_section)
            .collect();
    }
}

fn label_section(section: Section) -> Vec<Section> {
    let label = match &section {
        Section::Code { label, .. }
        | Section::Markdown { label, .. }
        | Section::Text { label, .. }
            if label == "stderr" =>
        {
            Some(label.clone())
        }
        _ => None,
    };
    label
        .map(Section::Label)
        .into_iter()
        .chain(std::iter::once(section))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_content_blocks_keep_type_and_mime_without_payload() {
        let record: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/mcp-codex/image.json"))
                .unwrap();
        let image = &record["payload"]["item"]["result"]["content"][1];
        // These types were absent from local transcripts; vary a captured media block.
        for kind in ["image", "audio", "resource_link", "future_content"] {
            let mut block = image.clone();
            block["type"] = kind.into();
            assert_eq!(
                super::mcp_content(&serde_json::Value::Array(vec![block])),
                vec![crate::log::Section::Placeholder(format!(
                    "[{kind} image/png]"
                ))]
            );
        }
    }

    #[test]
    fn mcp_presentation_uses_shared_tables_and_removes_title_fields() {
        let record: serde_json::Value =
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .find(|record: &serde_json::Value| {
                    record["message"]["content"][0]["name"]
                        == "mcp__plugin_codex_codex-manager__start"
                })
                .unwrap();
        let call = &record["message"]["content"][0];
        let rules = super::Rules {
            subjects: &[],
            inputs: &[],
        };
        for name in [call["name"].as_str().unwrap(), "codex-manager.start"] {
            let (name, subject, input) = super::presentation(name, &call["input"], &rules);
            assert_eq!(name, "codex-manager start");
            assert_eq!(subject.as_deref(), Some("tool-calls"));
            assert_eq!(
                input,
                vec![crate::log::Section::Prose {
                    text: call["input"]["prompt"].as_str().unwrap().into(),
                    markdown: false,
                }]
            );
        }
    }

    #[test]
    fn mcp_titles_show_server_and_tool() {
        for (name, expected) in [
            (
                "mcp__plugin_codex_codex-manager__send",
                "codex-manager send",
            ),
            (
                "mcp__plugin_codex_codex-manager__start",
                "codex-manager start",
            ),
            (
                "mcp__claude-in-chrome__navigate",
                "claude-in-chrome navigate",
            ),
            ("mcp__context7__query-docs", "context7 query-docs"),
            ("mcp__tool", "mcp__tool"),
            ("mcp____send", "mcp____send"),
            ("mcp__server__", "mcp__server__"),
            ("mcp__plugin_codex__send", "mcp__plugin_codex__send"),
            ("mcp__plugin__server__send", "mcp__plugin__server__send"),
            ("mcp__plugin_codex___send", "mcp__plugin_codex___send"),
        ] {
            assert_eq!(super::display_tool_name(name), expected);
        }
    }
}
