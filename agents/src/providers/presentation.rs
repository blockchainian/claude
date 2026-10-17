use crate::log::Section;
use serde_json::Value;

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

pub(crate) fn input_text<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input[key].as_str().filter(|text| !text.trim().is_empty())
}

pub(crate) fn default_title(name: &str, input: &Value) -> String {
    if name.starts_with("mcp__") {
        display_tool_name(name)
    } else {
        input_text(input, "description").unwrap_or(name).to_owned()
    }
}

pub(crate) fn agent_title(input: &Value) -> String {
    let agent = input_text(input, "subagent_type")
        .map(|kind| format!("{kind} agent"))
        .unwrap_or_else(|| "Agent".to_owned());
    input_text(input, "description")
        .map(|description| format!("{agent} {description}"))
        .unwrap_or(agent)
}

pub(crate) fn recipient_title(name: &str, input: &Value) -> String {
    input_text(input, "to")
        .or_else(|| input_text(input, "target"))
        .map(|recipient| format!("To {recipient}"))
        .unwrap_or_else(|| name.to_owned())
}

pub(crate) fn shell_title(name: &str, input: &Value) -> String {
    if let Some(description) = input_text(input, "description") {
        return description.to_owned();
    }
    let command = input_text(input, "command")
        .or_else(|| input_text(input, "cmd"))
        .unwrap_or("");
    let summary = crate::shell_summary::summarize_shell_commands(command);
    if summary.is_empty() {
        name.to_owned()
    } else {
        summary
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

/// Prefer the input description, then its name, for the centered title.
pub(crate) fn detail_title(sections: &[Section]) -> Option<String> {
    ["description", "name"].into_iter().find_map(|label_key| {
        sections
            .iter()
            .filter_map(|section| match section {
                Section::Fields(fields) => Some(fields),
                _ => None,
            })
            .flatten()
            .find_map(|(key, value)| {
                (key == label_key && !value.trim().is_empty()).then(|| value.clone())
            })
    })
}

fn free_text(key: &str) -> bool {
    matches!(key, "prompt" | "message" | "question")
}

/// Turn transcript argument fields into presentation semantics.
pub(crate) fn input_sections(sections: Vec<Section>) -> Vec<Section> {
    let mut result = Vec::new();
    for section in sections {
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
                    for (key, value) in fields {
                        if matches!(key.as_str(), "command" | "cmd") {
                            result.push(Section::Shell {
                                text: value,
                                prefix: format!("{key}: "),
                            });
                        } else {
                            result
                                .extend(input_sections(vec![Section::Fields(vec![(key, value)])]));
                        }
                    }
                } else {
                    let ordinary: Vec<_> = fields
                        .iter()
                        .filter(|(key, _)| !free_text(key))
                        .cloned()
                        .collect();
                    if !ordinary.is_empty() {
                        result.push(Section::Fields(ordinary));
                    }
                    for (key, value) in fields {
                        if free_text(&key) {
                            result.push(Section::Prose {
                                text: value,
                                markdown: true,
                            });
                        }
                    }
                }
            }
            Section::Code { label, lang, text } if label == "command" && lang == "bash" => {
                result.push(Section::Shell {
                    text,
                    prefix: String::new(),
                });
            }
            Section::Text { label, text } if free_text(&label) || label == "questions" => {
                result.push(Section::Prose {
                    text,
                    markdown: false,
                });
            }
            section => result.extend(output_sections(vec![section])),
        }
    }
    result
}

/// Make output labels explicit so renderers do not interpret field names.
pub(crate) fn output_sections(sections: Vec<Section>) -> Vec<Section> {
    let mut result = Vec::new();
    for mut section in sections {
        let label = match &mut section {
            Section::Text { label, .. }
            | Section::Markdown { label, .. }
            | Section::Code { label, .. } => Some(label),
            _ => None,
        };
        if let Some(label) = label
            && label == "stderr"
        {
            result.push(Section::Heading(std::mem::take(label)));
        }
        result.push(section);
    }
    result
}

#[cfg(test)]
mod tests {
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
