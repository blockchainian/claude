use super::{Event, Section, ToolCall};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Encrypted,
}

impl Content {
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Encrypted => "(encrypted)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    Incoming,
    Outgoing,
    Between,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub author: Option<String>,
    pub recipient: Option<String>,
    pub direction: Direction,
    pub kind: String,
    pub content: Content,
}

struct Route {
    tool: &'static str,
    recipient: &'static str,
    body: &'static str,
    kind: &'static str,
}

const AGENT_ROUTES: &[Route] = &[
    Route {
        tool: "Agent",
        recipient: "name",
        body: "prompt",
        kind: "SPAWN_AGENT",
    },
    Route {
        tool: "Task",
        recipient: "name",
        body: "prompt",
        kind: "SPAWN_AGENT",
    },
    Route {
        tool: "spawn_agent",
        recipient: "task_name",
        body: "message",
        kind: "SPAWN_AGENT",
    },
    Route {
        tool: "SendMessage",
        recipient: "to",
        body: "message",
        kind: "MESSAGE",
    },
    Route {
        tool: "send_message",
        recipient: "target",
        body: "message",
        kind: "MESSAGE",
    },
    Route {
        tool: "followup_task",
        recipient: "target",
        body: "message",
        kind: "FOLLOWUP_TASK",
    },
];

pub fn tool_message(name: &str, input: &Value) -> Option<Message> {
    let route = AGENT_ROUTES
        .iter()
        .find(|route| Some(route.tool) == name.rsplit('.').next())?;
    let recipient = input[route.recipient]
        .as_str()
        .filter(|s| !s.trim().is_empty() && !s.starts_with("uds:"));
    Some(Message {
        author: None,
        recipient: recipient.map(str::to_owned),
        direction: Direction::Outgoing,
        kind: route.kind.to_owned(),
        content: Content::Text(input[route.body].as_str().unwrap_or("").to_owned()),
    })
}

pub fn name(path: &str) -> &str {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

impl Message {
    pub fn label(&self) -> String {
        const LABELS: &[(&str, &str)] = &[
            ("MESSAGE", "Message"), ("NEW_TASK", "New task"),
            ("FINAL_ANSWER", "Final answer"), ("SPAWN_AGENT", "Spawn agent"),
            ("FOLLOWUP_TASK", "Followup task"),
        ];
        LABELS.iter().find_map(|(kind, label)| (*kind == self.kind).then(|| (*label).to_owned()))
            .unwrap_or_else(|| crate::providers::tool::title_name(&self.kind))
    }

    pub fn route(&self) -> String {
        match (&self.author, &self.recipient) {
            (Some(author), Some(recipient)) => format!("{} → {}", name(author), name(recipient)),
            (Some(author), None) => name(author).to_owned(),
            (None, Some(recipient)) => name(recipient).to_owned(),
            _ => String::new(),
        }
    }

    fn perspective(&self, content: &str) -> String {
        match self.direction {
            Direction::Outgoing => self
                .recipient
                .as_deref()
                .map(|p| format!("{} ← {content}", name(p))),
            Direction::Incoming => self
                .author
                .as_deref()
                .map(|p| format!("{} → {content}", name(p))),
            Direction::Between => Some(format!("{} · {content}", self.route())),
        }
        .unwrap_or_else(|| content.to_owned())
    }

    pub fn summary(&self, count: usize) -> String {
        if count > 1 {
            return self.perspective(&format!("{count} messages"));
        }
        if self.kind == "SPAWN_AGENT" {
            return self
                .recipient
                .as_deref()
                .map(|p| format!("Spawn agent {}", name(p)))
                .unwrap_or_else(|| "Spawn agent".to_owned());
        }
        self.perspective(match self.kind.as_str() {
            "NEW_TASK" => "new task",
            "FINAL_ANSWER" => match &self.content {
                Content::Text(text) => text,
                Content::Encrypted => "message",
            },
            "FOLLOWUP_TASK" => "Followup task",
            _ => "message",
        })
    }

    pub fn mergeable(&self, other: &Self) -> bool {
        self.kind == "MESSAGE"
            && other.kind == "MESSAGE"
            && self.author == other.author
            && self.recipient == other.recipient
            && self.direction == other.direction
            && (self.author.is_some() || self.recipient.is_some())
    }
}

/// The single presentation of an inter-agent message in tool and message details.
pub fn present(call: &mut ToolCall) {
    let Some(message) = &call.agent_message else {
        return;
    };
    call.title = message.summary(1);
    call.detail_left = message.label();
    call.detail_title = if message.kind == "SPAWN_AGENT" {
        message
            .recipient
            .as_deref()
            .map(name)
            .unwrap_or("")
            .to_owned()
    } else {
        message.route()
    };
    call.show_details = true;
    call.input = vec![Section::Markdown {
        label: String::new(),
        text: message.content.text().to_owned(),
    }];
}

pub fn summary(event: &Event) -> Option<String> {
    let message = event.calls.first()?.agent_message.as_ref()?;
    if event.calls.iter().all(|call| {
        call.agent_message
            .as_ref()
            .is_some_and(|other| message.mergeable(other))
    }) {
        Some(message.summary(event.calls.len()))
    } else if event.calls.len() == 1 {
        Some(message.summary(1))
    } else {
        Some(
            event
                .calls
                .iter()
                .map(|call| call.title.as_str())
                .collect::<Vec<_>>()
                .join(" · "),
        )
    }
}

pub fn can_merge(previous: &Event, event: &Event) -> bool {
    let Some(message) = previous
        .calls
        .first()
        .and_then(|call| call.agent_message.as_ref())
    else {
        return false;
    };
    previous.kind == event.kind
        && !event.calls.is_empty()
        && previous.calls.iter().chain(&event.calls).all(|call| {
            call.agent_message
                .as_ref()
                .is_some_and(|other| message.mergeable(other))
        })
}

pub fn merge(events: Vec<Event>) -> Vec<Event> {
    let mut merged: Vec<Event> = Vec::new();
    for event in events {
        if let Some(previous) = merged.last_mut()
            && can_merge(previous, &event)
        {
            previous.calls.extend(event.calls);
            previous.source_records.extend(event.source_records);
            previous.started_at = event.started_at;
            previous.ended_at = event.ended_at;
            previous.duration = previous.ended_at.map(|end| end - previous.started_at);
        } else {
            merged.push(event);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_agent_calls_share_the_message_model() {
        let claude: Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-agent-names.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let block = &claude["message"]["content"][0];
        let message = tool_message(block["name"].as_str().unwrap(), &block["input"]).unwrap();
        assert_eq!(message.summary(1), "Spawn agent reviewer");
        assert_eq!(message.label(), "Spawn agent");
        assert_eq!(message.author, None);
        assert_eq!(
            message.content.text(),
            block["input"]["prompt"].as_str().unwrap()
        );
        let records = include_str!("../../tests/fixtures/codex-agent-names.jsonl");
        for (line, expected) in records.lines().zip([
            "Spawn agent real_scan",
            "root ← message",
            "ux_probe ← Followup task",
        ]) {
            let record: Value = serde_json::from_str(line).unwrap();
            let payload = &record["payload"];
            let input: Value =
                serde_json::from_str(payload["arguments"].as_str().unwrap()).unwrap();
            let message = tool_message(payload["name"].as_str().unwrap(), &input).unwrap();
            assert_eq!(message.author, None);
            assert_eq!(message.direction, Direction::Outgoing);
            assert_eq!(message.summary(1), expected);
        }
    }
}
