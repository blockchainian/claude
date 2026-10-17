use crate::log::{Section, ToolCall};
use serde_json::Value;

pub(crate) fn json_input(value: &Value) -> Vec<Section> {
    if value.is_null() {
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
        return name.to_owned();
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

pub(super) enum Title {
    Questions,
    Agent,
    Skill,
    Recipient,
    Shell,
    Tool,
}

pub(super) fn title(name: &str, input: &Value, kind: Title) -> String {
    let text = |key: &str| input[key].as_str().filter(|text| !text.trim().is_empty());
    match kind {
        Title::Questions => std::iter::once(name)
            .chain(
                input["questions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|question| {
                        question["header"]
                            .as_str()
                            .filter(|header| !header.trim().is_empty())
                    }),
            )
            .collect::<Vec<_>>()
            .join(" · "),
        Title::Agent => {
            let agent = text("subagent_type")
                .map(|kind| format!("{kind} agent"))
                .unwrap_or_else(|| "Agent".to_owned());
            text("description")
                .map(|description| format!("{agent} {description}"))
                .unwrap_or(agent)
        }
        Title::Skill => text("skill").unwrap_or(name).to_owned(),
        Title::Recipient => text("to")
            .or_else(|| text("target"))
            .map(|recipient| format!("To {recipient}"))
            .unwrap_or_else(|| name.to_owned()),
        _ if name.starts_with("mcp__") => display_tool_name(name),
        _ if text("description").is_some() => text("description").unwrap().to_owned(),
        Title::Shell => {
            let command = text("command").or_else(|| text("cmd")).unwrap_or("");
            let summary = crate::shell_summary::summarize_shell_commands(command);
            if summary.is_empty() {
                name.to_owned()
            } else {
                summary
            }
        }
        Title::Tool => name.to_owned(),
    }
}

/// Retain the input description alongside specialized input sections.
pub(crate) fn include_description(sections: &mut Vec<Section>, input: &Value) {
    if let Some(description) = input["description"].as_str()
        && !sections.iter().any(|section| matches!(section, Section::Fields(fields) if fields.iter().any(|(key, _)| key == "description")))
    {
        sections.push(Section::Fields(vec![("description".into(), description.into())]));
    }
}

pub(crate) fn question_texts(input: &Value) -> Vec<String> {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|question| question["question"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Assign detail metadata and presentation before crossing the UI boundary.
pub(super) fn annotate(call: &mut ToolCall) {
    call.detail_title = ["description", "name"]
        .into_iter()
        .find_map(|label| {
            call.input
                .iter()
                .filter_map(|section| match section {
                    Section::Fields(fields) => Some(fields),
                    _ => None,
                })
                .flatten()
                .find_map(|(key, value)| {
                    (key == label && !value.trim().is_empty()).then(|| value.clone())
                })
        })
        .unwrap_or_else(|| call.title.clone());
    call.input = std::mem::take(&mut call.input)
        .into_iter()
        .flat_map(input_section)
        .collect();
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

fn free_text(key: &str) -> bool {
    matches!(key, "prompt" | "message" | "question")
}

fn input_section(section: Section) -> Vec<Section> {
    match section {
        Section::Fields(fields) => {
            let fields: Vec<_> = fields
                .into_iter()
                .filter(|(key, _)| {
                    !matches!(key.as_str(), "name" | "description" | "subagent_type")
                })
                .collect();
            if fields
                .iter()
                .any(|(key, _)| matches!(key.as_str(), "command" | "cmd"))
            {
                return fields
                    .into_iter()
                    .flat_map(|(key, value)| {
                        if matches!(key.as_str(), "command" | "cmd") {
                            vec![Section::Shell {
                                prefix: key.len() + 2,
                                text: format!("{key}: {value}"),
                            }]
                        } else {
                            input_section(Section::Fields(vec![(key, value)]))
                        }
                    })
                    .collect();
            }
            let ordinary: Vec<_> = fields
                .iter()
                .filter(|(key, _)| !free_text(key))
                .cloned()
                .collect();
            let mut sections = Vec::new();
            if !ordinary.is_empty() {
                sections.push(Section::Fields(ordinary));
            }
            sections.extend(fields.into_iter().filter(|(key, _)| free_text(key)).map(
                |(_, text)| Section::Prose {
                    text,
                    markdown: true,
                },
            ));
            sections
        }
        Section::Code { label, lang, text } if label == "command" && lang == "bash" => {
            vec![Section::Shell { text, prefix: 0 }]
        }
        Section::Text { label, text } if free_text(&label) || label == "questions" => {
            vec![Section::Prose {
                text: text.trim_end_matches(['\n', '\r']).to_owned(),
                markdown: false,
            }]
        }
        section => label_section(section),
    }
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
