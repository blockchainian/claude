pub(crate) mod groups;
mod shell_summary;

use crate::log::{Section, ToolCall};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn json_input(value: &Value) -> Vec<Section> {
    if value.is_null() || value.as_object().is_some_and(|object| object.is_empty()) {
        Vec::new()
    } else {
        vec![Section::Structured {
            label: "input".into(),
            text: value.to_string(),
        }]
    }
}

/// Decode MCP and Anthropic content blocks without exposing binary payloads.
pub(crate) fn mcp_content(content: &Value) -> Vec<Section> {
    if let Some(text) = content.as_str() {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![Section::Text {
                label: "output".into(),
                text: text.into(),
            }]
        };
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
        .filter(|section| !matches!(section, Section::Text { text, .. } if text.is_empty()))
        .collect()
}

fn tool_name(name: &str) -> String {
    let Some((server, tool)) = name
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
    else {
        return name
            .split_once('.')
            .filter(|(server, tool)| !server.is_empty() && !tool.is_empty())
            .map(|(server, tool)| {
                let mcp = format!("{server} {tool}");
                if MCP_RULES.subjects.iter().any(|(name, _)| *name == mcp) {
                    mcp
                } else {
                    tool.to_owned()
                }
            })
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

/// One display-name rule for both providers; protocol namespaces are not tool names.
pub(crate) fn display_tool_name(name: &str) -> String {
    let name = tool_name(name);
    if name.starts_with("mcp__") {
        return name;
    }
    title_name(&name)
}

/// Underscore identifiers use sentence case; camel-case identifiers retain their spelling.
pub(crate) fn title_name(name: &str) -> String {
    let name = tool_name(name);
    let text = if name.contains('_') || name.chars().all(|c| !c.is_lowercase()) {
        name.split('_')
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    } else {
        name.to_owned()
    };
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Subject {
    Field(&'static str),
    ResultField {
        result: &'static str,
        input: &'static str,
    },
    QuestionHeaders,
    Recipient,
    Shell {
        description: bool,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum Input {
    Field(&'static str),
    QuestionList,
    Arguments,
    FetchPrompt,
    AgentMessage,
    Shell,
}

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Text,
    Markdown,
}

pub(crate) struct Rules {
    pub subjects: &'static [(&'static str, Subject)],
    pub inputs: &'static [(&'static str, Input, Format)],
    pub expanded: &'static [(&'static str, bool)],
}

const MCP_RULES: Rules = Rules {
    subjects: &[
        ("codex-manager start", Subject::Field("name")),
        ("codex-manager send", Subject::Field("threadId")),
        ("codex-manager interrupt", Subject::Field("threadId")),
        (
            "codex-manager detach",
            Subject::ResultField {
                result: "name",
                input: "threadId",
            },
        ),
    ],
    inputs: &[
        (
            "codex-manager start",
            Input::Field("prompt"),
            Format::Markdown,
        ),
        (
            "codex-manager send",
            Input::Field("prompt"),
            Format::Markdown,
        ),
    ],
    expanded: &[],
};

fn find<T: Copy>(name: &str, table: &[(&str, T)]) -> Option<T> {
    table.iter().find_map(|(tool, value)| {
        (*tool == name || (!name.contains(' ') && *tool == name.rsplit('.').next().unwrap_or(name)))
            .then_some(*value)
    })
}

fn text<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input[field].as_str().filter(|text| !text.trim().is_empty())
}

/// A message's named recipient; a peer session's socket address names nobody.
fn recipient(input: &Value) -> Option<&str> {
    text(input, "to").filter(|to| !to.starts_with("uds:"))
}

pub(crate) fn expanded(name: &str, rules: &Rules) -> bool {
    find(name, rules.expanded).unwrap_or(true)
}

/// A human description stands alone; otherwise the display name heads its subject.
pub(crate) fn list_label(
    name: &str,
    subject: Option<&str>,
    input: &Value,
) -> (String, Option<String>) {
    match input["description"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
    {
        Some(description) => (description.to_owned(), None),
        None => (
            name.to_owned(),
            subject.filter(|text| !text.is_empty()).map(str::to_owned),
        ),
    }
}

pub(crate) fn list_title(name: &str, subject: Option<&str>, input: &Value) -> String {
    let (head, member) = list_label(name, subject, input);
    let mut groups = groups::Groups::default();
    groups.add(&head, member.as_deref());
    groups.finish().unwrap()
}

fn subject_rule(name: &str, rules: &Rules) -> Option<Subject> {
    find(name, rules.subjects).or_else(|| find(&tool_name(name), MCP_RULES.subjects))
}

/// Read a declared subject from JSON output, leaving the input subject as fallback.
pub(crate) fn result_subject(
    name: &str,
    output: Option<&[Section]>,
    rules: &Rules,
) -> Option<String> {
    let Subject::ResultField { result, .. } = subject_rule(name, rules)? else {
        return None;
    };
    output?.iter().find_map(|section| {
        let source = match section {
            Section::Text { text, .. } | Section::Structured { text, .. } => text,
            _ => return None,
        };
        let value: Value = serde_json::from_str(source).ok()?;
        text(&value, result).map(str::to_owned)
    })
}

fn shell_command(input: &Value) -> Option<&str> {
    text(input, "command").or_else(|| text(input, "cmd"))
}

pub(crate) fn presentation(
    name: &str,
    input: &Value,
    rules: &Rules,
) -> (String, Option<String>, Vec<Section>) {
    let raw_name = name;
    let name = display_tool_name(name);
    let mut remaining = input.clone();
    let subject = match subject_rule(raw_name, rules) {
        Some(Subject::Field(field) | Subject::ResultField { input: field, .. }) => remaining
            .as_object_mut()
            .and_then(|object| object.remove(field))
            .and_then(|value| {
                value
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
            }),
        Some(Subject::Shell { description }) => description
            .then(|| text(input, "description"))
            .flatten()
            .map(str::to_owned)
            .or_else(|| {
                let summary = shell_summary::summarize_shell_commands(shell_command(input)?);
                (!summary.is_empty()).then_some(summary)
            }),
        Some(Subject::Recipient) => {
            let to = recipient(input).map(str::to_owned);
            if let Some(object) = remaining.as_object_mut() {
                object.remove("to");
            }
            to
        }
        Some(Subject::QuestionHeaders) => Some(
            input["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|question| text(question, "header"))
                .collect::<Vec<_>>()
                .join(" · "),
        )
        .filter(|headers| !headers.is_empty()),
        None => None,
    };
    let input_rule = |name: &str, table: &[(&str, Input, Format)]| {
        table
            .iter()
            .find_map(|(tool, input, format)| find(name, &[(*tool, (*input, *format))]))
    };
    let rule = input_rule(raw_name, rules.inputs)
        .or_else(|| input_rule(&tool_name(raw_name), MCP_RULES.inputs));
    let sections = match rule {
        Some((Input::Field(""), _)) => Vec::new(),
        Some((method, format)) => {
            let (body, mut sections) = match method {
                Input::Shell => (shell_command(input).unwrap_or("").to_owned(), Vec::new()),
                Input::Field(field) => (
                    remaining
                        .get(field)
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .unwrap_or_default(),
                    Vec::new(),
                ),
                Input::QuestionList => (String::new(), question_sections(input)),
                Input::Arguments => (input["args"].as_str().unwrap_or("").to_owned(), Vec::new()),
                Input::AgentMessage => {
                    (text(input, "message").unwrap_or("").to_owned(), Vec::new())
                }
                Input::FetchPrompt => (
                    text(input, "prompt").unwrap_or("").to_owned(),
                    text(input, "url")
                        .map(|url| {
                            vec![Section::Text {
                                label: String::new(),
                                text: format!("url: {url}"),
                            }]
                        })
                        .unwrap_or_default(),
                ),
            };
            if !body.trim().is_empty() {
                let section = match format {
                    _ if matches!(method, Input::Shell) => Section::Shell {
                        text: body,
                        prefix: 0,
                    },
                    Format::Text if matches!(method, Input::Arguments) => Section::Text {
                        label: "args".into(),
                        text: body,
                    },
                    Format::Text => Section::Prose {
                        text: body,
                        markdown: false,
                    },
                    Format::Markdown => Section::Markdown {
                        label: match method {
                            Input::Field(field) => field,
                            Input::AgentMessage => "message",
                            _ => "",
                        }
                        .into(),
                        text: body,
                    },
                };
                sections.push(section);
            }
            sections
        }
        None => json_input(&remaining),
    };
    (name, subject, sections)
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

/// Lists an ask's answers the way the question view's Submit tab does: a warning while
/// questions are unanswered, then one `● header → answer` line per answered question.
pub(crate) fn answer_list(questions: &Value, answers: &BTreeMap<String, String>) -> String {
    let questions = questions.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut blocks = Vec::new();
    if questions
        .iter()
        .any(|question| !answers.contains_key(question["question"].as_str().unwrap_or("")))
    {
        blocks.push("You have not answered all questions".to_owned());
    }
    let lines: Vec<String> = questions
        .iter()
        .filter_map(|question| {
            let answer = answers.get(question["question"].as_str()?)?;
            let header = text(question, "header").or_else(|| text(question, "question"))?;
            Some(format!("● {header} → {answer}"))
        })
        .collect();
    if !lines.is_empty() {
        blocks.push(lines.join("\n"));
    }
    blocks.join("\n\n")
}

/// Shows a rejected ask as `Rejected` with the user's note, or as `Chat about this` with
/// the answers its "Questions asked:" list reports; other errors keep their text.
pub(crate) fn rejected_answers(questions: &Value, text: &str) -> Option<String> {
    let text = text.strip_prefix("Error: ").unwrap_or(text);
    let reason = text.strip_prefix("The user doesn't want to proceed with this tool use.")?;
    let Some((_, said)) = reason.split_once("the user said:\n") else {
        return Some("Rejected".to_owned());
    };
    if !said.starts_with("The user wants to clarify these questions.") {
        return Some(format!("Rejected\n\n{}", said.trim()));
    }
    let mut answers = BTreeMap::new();
    let (_, asked) = said.split_once("Questions asked:")?;
    let mut lines = asked.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        let Some(question) = line
            .strip_prefix("- \"")
            .and_then(|line| line.strip_suffix('"'))
        else {
            continue;
        };
        if let Some(answer) = lines.peek().and_then(|line| line.strip_prefix("Answer: ")) {
            answers.insert(question.to_owned(), answer.to_owned());
        }
    }
    Some(format!(
        "Chat about this\n\n{}",
        answer_list(questions, &answers)
    ))
}

pub(crate) fn rejected_questions(text: &str) -> Option<String> {
    let (explanation, questions) = text.split_once("Questions asked:")?;
    Some(format!(
        "Questions asked:{}\n\n{}",
        questions.trim_end(),
        explanation.trim_end()
    ))
}

/// One titled section per question: its text, then its numbered options.
fn question_sections(input: &Value) -> Vec<Section> {
    let mut sections = Vec::new();
    for question in input["questions"].as_array().into_iter().flatten() {
        let Some(asked) = text(question, "question") else {
            continue;
        };
        if let Some(header) = text(question, "header") {
            sections.push(Section::Label(header.to_owned()));
        }
        let suffix = if question["multiSelect"] == true {
            " (multi-select)"
        } else {
            ""
        };
        let options = question["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| {
                text(option, "label").map(|label| (label, text(option, "description")))
            });
        let lines: Vec<String> = std::iter::once(format!("{asked}{suffix}"))
            .chain(
                options
                    .enumerate()
                    .map(|(index, (label, description))| match description {
                        Some(description) => format!("{}. {label}: {description}", index + 1),
                        None => format!("{}. {label}", index + 1),
                    }),
            )
            .collect();
        sections.push(Section::Prose {
            text: lines.join("\n"),
            markdown: false,
        });
    }
    sections
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
    fn captured_tool_names_share_one_display_rule() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/codex-agent-names.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let name = record["payload"]["name"].as_str().unwrap();
        assert_eq!(super::display_tool_name(name), "Send message");
        assert_eq!(
            super::display_tool_name(&format!("collaboration.{name}")),
            "Send message"
        );
        for line in include_str!("../../tests/fixtures/claude-tool-calls.jsonl").lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some(name @ ("WebFetch" | "Bash" | "Read" | "Skill")) =
                record["message"]["content"][0]["name"].as_str()
            {
                assert_eq!(super::display_tool_name(name), name);
            }
        }
    }

    #[test]
    fn empty_captured_output_has_no_sections() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let mut content = record["message"]["content"][0]["content"].clone();
        content = serde_json::Value::String(content.as_str().unwrap().chars().take(0).collect());
        assert!(super::mcp_content(&content).is_empty());
    }

    #[test]
    fn result_subject_follows_the_declared_fields() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-manager-detach-name.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let rules = super::Rules {
            subjects: &[(
                "inspect",
                super::Subject::ResultField {
                    result: "name",
                    input: "threadId",
                },
            )],
            inputs: &[],
            expanded: &[],
        };
        let input = &records[0]["message"]["content"][0]["input"];
        let content = &records[1]["message"]["content"][0]["content"];
        let output = super::mcp_content(content);
        assert_eq!(
            super::result_subject("inspect", Some(&output), &rules).as_deref(),
            Some("模型和new挪位")
        );
        assert_eq!(
            super::presentation("inspect", input, &rules).1.as_deref(),
            input["threadId"].as_str()
        );
        let mut value: serde_json::Value =
            serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        for name in [
            serde_json::Value::Null,
            serde_json::json!(" "),
            serde_json::json!(42),
        ] {
            value["name"] = name;
            let output = vec![crate::log::Section::Structured {
                label: "output".into(),
                text: value.to_string(),
            }];
            assert_eq!(
                super::result_subject("inspect", Some(&output), &rules),
                None
            );
        }
    }

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
            expanded: &[],
        };
        for name in [call["name"].as_str().unwrap(), "codex-manager.start"] {
            let (name, subject, input) = super::presentation(name, &call["input"], &rules);
            assert_eq!(name, "Codex-manager start");
            assert_eq!(subject.as_deref(), Some("tool-calls"));
            assert_eq!(
                input,
                vec![crate::log::Section::Markdown {
                    label: "prompt".into(),
                    text: call["input"]["prompt"].as_str().unwrap().into(),
                }]
            );
        }
    }

    #[test]
    fn mcp_titles_show_server_and_tool() {
        for (name, expected) in [
            (
                "mcp__plugin_codex_codex-manager__send",
                "Codex-manager send",
            ),
            (
                "mcp__plugin_codex_codex-manager__start",
                "Codex-manager start",
            ),
            (
                "mcp__claude-in-chrome__navigate",
                "Claude-in-chrome navigate",
            ),
            ("mcp__context7__query-docs", "Context7 query-docs"),
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
