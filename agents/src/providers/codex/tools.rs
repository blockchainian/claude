use crate::providers::tool::mcp_content;
use crate::providers::tool::*;
use serde_json::Value;

pub(super) const RULES: &[ToolRule] = &[
    ToolRule {
        name: "exec_command",
        title: &[Pick::With(shell_title)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(merged_shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: true,
            trimmed: false,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "shell",
        title: &[Pick::With(shell_title)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(merged_shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: true,
            trimmed: false,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "CommandExecution",
        title: &[Pick::With(shell_title)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(merged_shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: true,
            trimmed: false,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "request_user_input",
        title: &[Pick::With(question_title)],
        input: Some((Pick::With(questions_input), Format::Text)),
        output: Some((Pick::With(questions_output), Format::Text)),
        question: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "spawn_agent",
        output: Some((Pick::With(hidden_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "send_message",
        title: &[Pick::Field(Source::Input, "/target")],
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::With(empty_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "followup_task",
        title: &[Pick::Field(Source::Input, "/target")],
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::With(empty_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "wait_agent",
        input: Some((Pick::Field(Source::Input, "/timeout_ms"), Format::Text)),
        output: Some((Pick::With(wait_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "apply_patch",
        title: &[Pick::Field(Source::Input, "/paths")],
        input: Some((Pick::With(empty_input), Format::Text)),
        wrapper: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "exec",
        wrapper: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "view_image",
        wrapper: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "update_plan",
        task: Some(TaskAction::Plan),
        ..DEFAULT_RULE
    },
];

fn hidden_output(data: &ToolData<'_>) -> Option<Body> {
    (!data.result.is_some_and(|result| result.error)).then_some(Body::Hidden)
}

fn empty_output(data: &ToolData<'_>) -> Option<Body> {
    (!data.result.is_some_and(|result| result.error)).then_some(Body::Sections(Vec::new()))
}

fn wait_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let value = &result.structured;
    let text = if value["timed_out"] == true {
        "(timed out)".to_owned()
    } else {
        value["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| result.text())
    };
    Some(Body::Sections(mcp_content(&Value::String(text), None)))
}

pub(crate) fn merged_shell_output(data: &ToolData<'_>) -> Option<Body> {
    shell_sections(data, rule("exec_command").shell_stream)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_declarations_are_unique_and_keep_captured_agent_fields() {
        let mut names = std::collections::HashSet::new();
        for rule in crate::providers::TOOL_RULES
            .iter()
            .flat_map(|rules| rules.native)
        {
            assert!(
                names.insert(rule.name),
                "duplicate declaration: {}",
                rule.name
            );
        }
        for line in include_str!("../../../tests/fixtures/codex-agent-names.jsonl").lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            let payload = &record["payload"];
            let name = payload["name"].as_str().unwrap();
            let input: serde_json::Value =
                serde_json::from_str(payload["arguments"].as_str().unwrap()).unwrap();
            let subject = input[if name == "spawn_agent" {
                "task_name"
            } else {
                "target"
            }]
            .as_str()
            .unwrap();
            for raw in [
                name.to_owned(),
                format!("collaboration.{name}"),
                format!("functions.collaboration.{name}"),
            ] {
                if name == "spawn_agent" {
                    let message = crate::event::agent::tool_message(&raw, &input).unwrap();
                    assert_eq!(message.recipient.as_deref(), Some(subject));
                    assert_eq!(message.content.text(), input["message"].as_str().unwrap());
                    continue;
                }
                let super::Presentation {
                    title: actual,
                    input: sections,
                    ..
                } = super::presentation(
                    &raw,
                    &super::ToolData {
                        input: &input,
                        result: None,
                    },
                );
                assert_eq!(actual.as_deref(), Some(subject));
                assert_eq!(
                    sections,
                    vec![crate::event::Section::Markdown {
                        label: "message".into(),
                        text: input["message"].as_str().unwrap().to_owned(),
                    }]
                );
            }
        }
    }

    #[test]
    fn captured_wait_name_keeps_a_capitalized_tool_label() {
        let record: serde_json::Value =
            include_str!("../../../tests/fixtures/codex-perspective/beta.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .find(|record| record["payload"]["name"] == "wait")
                .unwrap();
        let name = record["payload"]["name"].as_str().unwrap();
        assert_eq!(super::display_tool_name(name), "Wait");
    }
}
