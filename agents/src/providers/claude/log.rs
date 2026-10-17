use crate::log::{Event, Kind, Section, ToolCall, participant_text};
use chrono::{DateTime, Utc};
use std::{
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) fn tool_hidden(name: &str) -> bool {
    matches!(
        name,
        "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" | "ListAgents"
    )
}

fn input_text<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input[key].as_str().filter(|text| !text.trim().is_empty())
}

fn default_detail(input: &serde_json::Value) -> Option<(&str, &str)> {
    ["description", "file_path", "prompt", "query", "message"]
        .into_iter()
        .find_map(|key| input_text(input, key).map(|text| (key, text)))
}

pub(super) fn tool_kind(name: &str) -> Kind {
    match name.rsplit('.').next() {
        Some("AskUserQuestion" | "request_user_input") => Kind::Assistant,
        _ => Kind::Tool,
    }
}

fn base_tool_title(name: &str, input: &serde_json::Value) -> String {
    use crate::providers::tool::Title;
    let kind = match name.rsplit('.').next().unwrap_or(name) {
        "AskUserQuestion" => Title::Questions,
        "Agent" | "Task" | "spawn_agent" => Title::Agent,
        "Skill" => Title::Skill,
        "SendMessage" | "send_message" | "followup_task" => Title::Recipient,
        "Bash" | "exec" | "exec_command" | "shell" => Title::Shell,
        _ => Title::Tool,
    };
    crate::providers::tool::title(name, input, kind)
}

fn tool_title(name: &str, input: &serde_json::Value) -> String {
    if name == "Agent" {
        return name.to_owned();
    }
    let title = base_tool_title(name, input);
    if name == "Skill"
        && let Some(args) = input["args"].as_str().filter(|args| !args.is_empty())
    {
        format!("{title} {args}")
    } else {
        title
    }
}

fn tool_detail(name: &str, input: &serde_json::Value) -> String {
    match name {
        "AskUserQuestion" => crate::providers::tool::question_texts(input).join(" · "),
        "Bash" => input_text(input, "description")
            .map(str::to_owned)
            .unwrap_or_else(|| {
                crate::shell_summary::summarize_shell_commands(
                    input_text(input, "command").unwrap_or(""),
                )
            }),
        "WebFetch" => input_text(input, "url")
            .map(|url| {
                let address = url.split_once("://").map_or(url, |(_, address)| address);
                address.split(['?', '#']).next().unwrap_or("").to_owned()
            })
            .unwrap_or_default(),
        "SendMessage" => {
            let message = input_text(input, "summary")
                .or_else(|| input_text(input, "message").and_then(|text| text.lines().next()));
            participant_text(input_text(input, "to"), message.unwrap_or(""))
        }
        "Agent" => [input_text(input, "name"), input_text(input, "description")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
        "Task" => participant_text(
            input_text(input, "name"),
            default_detail(input).map(|(_, text)| text).unwrap_or(""),
        ),
        "SubagentHandback" => participant_text(
            input_text(input, "name"),
            input_text(input, "message")
                .and_then(|text| text.lines().next())
                .unwrap_or(""),
        ),
        _ => default_detail(input)
            .map(|(_, text)| text)
            .unwrap_or("")
            .to_owned(),
    }
}

fn tool_label(name: &str, detail: &str) -> String {
    let name = if name == "SendMessage" { "To" } else { name };
    if detail.is_empty() {
        crate::providers::tool::display_tool_name(name)
    } else if matches!(name, "Bash" | "AskUserQuestion" | "Agent") {
        detail.to_owned()
    } else {
        let label = crate::providers::tool::display_tool_name(name);
        if label == name {
            format!("{label} {detail}")
        } else {
            [label.as_str(), detail].join(" · ")
        }
    }
}

pub(super) fn tool_activity(block: &serde_json::Value) -> Option<String> {
    let name = block["name"].as_str()?;
    if tool_hidden(name) {
        return None;
    }
    let input = &block["input"];
    if matches!(
        name,
        "Bash" | "WebFetch" | "SendMessage" | "SubagentHandback" | "AskUserQuestion"
    ) || (matches!(name, "Agent" | "Task") && input_text(input, "name").is_some())
    {
        Some(tool_label(name, &tool_detail(name, input)))
    } else {
        Some(input_text(input, "description").map_or_else(
            || crate::providers::tool::display_tool_name(name),
            str::to_owned,
        ))
    }
}

#[cfg(test)]
pub fn tail(path: &Path, count: usize) -> io::Result<Vec<Event>> {
    tail_session(path, count, false)
}

#[cfg(test)]
pub fn tail_session(path: &Path, count: usize, sidechains: bool) -> io::Result<Vec<Event>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut log = Log {
        path: Some(path.to_path_buf()),
        ..Log::default()
    };
    crate::jsonl::read_lines_back(&mut file, len, |offset, line| {
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) else {
            return true;
        };
        if !sidechains && record["isSidechain"] == true {
            return true;
        }
        log.offset = offset;
        log.record(record, count)
    })?;
    if let Some(error) = log.error {
        return Err(error);
    }
    let mut events: Vec<_> = log.entries.into_iter().map(|entry| entry.event).collect();
    events.reverse();
    let mut previous_end = None;
    for event in &mut events {
        if event.kind != Kind::Assistant || event.previous_end.is_none() {
            event.previous_end = previous_end;
        }
        previous_end = match event.kind {
            Kind::Tool | Kind::Error | Kind::Denied => event.ended_at,
            _ => Some(event.started_at),
        };
    }
    let skip = events.len().saturating_sub(count);
    Ok(events.into_iter().skip(skip).collect())
}

/// Retains decoded records and parsed rows between file notifications.
/// Only the suffix affected by a new call, result or image companion is replayed.
pub(crate) struct Reader {
    path: PathBuf,
    sidechains: bool,
    identity: Option<(u64, u64)>,
    modified: Option<std::time::SystemTime>,
    oldest_offset: u64,
    read_offset: u64,
    pending: Vec<u8>,
    records: Vec<(u64, serde_json::Value)>,
    entries: Vec<Entry>,
    shown: usize,
    boundary: crate::jsonl::Boundary,
    #[cfg(test)]
    replayed_records: usize,
}

impl Reader {
    pub fn new(path: PathBuf, sidechains: bool) -> Self {
        Self {
            path,
            sidechains,
            identity: None,
            modified: None,
            oldest_offset: 0,
            read_offset: 0,
            pending: Vec::new(),
            records: Vec::new(),
            entries: Vec::new(),
            shown: 0,
            boundary: crate::jsonl::Boundary::default(),
            #[cfg(test)]
            replayed_records: 0,
        }
    }

    fn decode(&self, bytes: &[u8], start: u64) -> Vec<(u64, serde_json::Value)> {
        let mut offset = start;
        bytes
            .split_inclusive(|byte| *byte == b'\n')
            .filter_map(|line| {
                let position = offset;
                offset += line.len() as u64;
                let record = serde_json::from_slice::<serde_json::Value>(line).ok()?;
                (self.sidechains || record["isSidechain"] != true).then_some((position, record))
            })
            .collect()
    }

    fn replay(&mut self, start: u64) -> io::Result<usize> {
        let retained = self.entries.partition_point(|entry| entry.offset < start);
        let replaced = self.entries.len() - retained;
        self.entries.truncate(retained);
        let mut log = Log {
            path: Some(self.path.clone()),
            ..Log::default()
        };
        for (offset, record) in self
            .records
            .iter()
            .rev()
            .take_while(|(offset, _)| *offset >= start)
        {
            #[cfg(test)]
            {
                self.replayed_records += 1;
            }
            log.offset = *offset;
            log.record(record.clone(), usize::MAX);
        }
        if let Some(error) = log.error {
            return Err(error);
        }
        self.entries.extend(log.entries.into_iter().rev());
        let mut previous_end = None;
        for entry in &mut self.entries {
            let event = &mut entry.event;
            if event.kind != Kind::Assistant || event.previous_end.is_none() {
                event.previous_end = previous_end;
            }
            previous_end = match event.kind {
                Kind::Tool | Kind::Error | Kind::Denied => event.ended_at,
                _ => Some(event.started_at),
            };
        }
        Ok(replaced)
    }

    fn affected_start(&self, appended: &[(u64, serde_json::Value)]) -> u64 {
        let mut group = None;
        let mut key = None;
        let mut requests = std::collections::HashMap::new();
        let mut prompts = std::collections::HashMap::new();
        let mut queued = std::collections::HashMap::new();
        for (offset, record) in &self.records {
            if record["type"] == "queue-operation"
                && record["operation"] == "enqueue"
                && let Some(text) = record["content"].as_str()
            {
                queued.entry(text).or_insert(*offset);
            }
            if record["type"] == "assistant" {
                let next = record["message"]["id"]
                    .as_str()
                    .or_else(|| record["requestId"].as_str())
                    .or_else(|| record["uuid"].as_str());
                if next.is_none() || next != key {
                    group = Some(*offset);
                    key = next;
                }
                for block in record["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if block["type"] == "tool_use"
                        && let Some(id) = block["id"].as_str()
                    {
                        requests.insert(id, group.unwrap_or(*offset));
                    }
                }
            }
            if record["type"] == "user"
                && record["isMeta"] != true
                && let Some(id) = record["promptId"].as_str()
            {
                prompts.insert(id, *offset);
            }
        }
        let mut start = group.unwrap_or(self.read_offset);
        for (_, record) in appended {
            if super::images::queued_prompt(record).is_some()
                && let Some(id) = record["attachment"]["source_uuid"].as_str()
                && let Some(text) =
                    self.records
                        .iter()
                        .chain(appended.iter())
                        .rev()
                        .find_map(|(_, removal)| {
                            (removal["type"] == "queue-operation"
                                && removal["operation"] == "remove"
                                && removal["commandUuid"] == id)
                                .then(|| removal["content"].as_str())
                                .flatten()
                        })
                && let Some(offset) = queued.get(text)
            {
                start = start.min(*offset);
            }
            for block in record["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if block["type"] == "tool_result"
                    && let Some(offset) = block["tool_use_id"]
                        .as_str()
                        .and_then(|id| requests.get(id))
                {
                    start = start.min(*offset);
                }
            }
            if record["turnCompanion"] == true
                && let Some(offset) = record["promptId"].as_str().and_then(|id| prompts.get(id))
            {
                start = start.min(*offset);
            }
        }
        start
    }

    fn older_page(&mut self, file: &mut std::fs::File) -> io::Result<u64> {
        let end = self.oldest_offset;
        let mut start = end.saturating_sub(64 * 1024);
        let (bytes, skip) = loop {
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = vec![0; (end - start) as usize];
            file.read_exact(&mut bytes)?;
            let skip = if start == 0 {
                Some(0)
            } else {
                bytes
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|index| index + 1)
            };
            if let Some(skip) = skip.filter(|skip| *skip < bytes.len() || start == 0) {
                break (bytes, skip);
            }
            start = start.saturating_sub(64 * 1024);
        };
        self.oldest_offset = start + skip as u64;
        let complete = if end == self.read_offset && self.records.is_empty() {
            let complete = bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(skip, |index| index + 1);
            self.pending = bytes[complete..].to_vec();
            complete
        } else {
            bytes.len()
        };
        let mut records = self.decode(&bytes[skip..complete], self.oldest_offset);
        if complete < bytes.len() {
            records.extend(self.decode(&bytes[complete..], start + complete as u64));
        }
        records.append(&mut self.records);
        self.records = records;
        Ok((bytes.len() - skip) as u64)
    }

    pub fn refresh(&mut self, requested: usize) -> io::Result<crate::log::Update> {
        let mut file = std::fs::File::open(&self.path)?;
        let metadata = file.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let modified = metadata.modified().ok();
        let reset = self.identity != Some(identity)
            || metadata.len() < self.read_offset
            || (metadata.len() == self.read_offset && modified != self.modified)
            || (metadata.len() > self.read_offset && !self.boundary.matches(&mut file)?);
        self.modified = modified;
        if reset {
            self.identity = Some(identity);
            self.records.clear();
            self.entries.clear();
            self.shown = 0;
            self.pending.clear();
            self.oldest_offset = metadata.len();
            self.read_offset = metadata.len();
        }
        let before = self.entries.len();
        let shown_before = self.shown;
        let mut parsed_bytes = 0;
        let mut replaced = 0;
        if !reset && metadata.len() > self.read_offset {
            file.seek(SeekFrom::Start(self.read_offset))?;
            let start = self.read_offset - self.pending.len() as u64;
            let mut bytes = std::mem::take(&mut self.pending);
            (&mut file)
                .take(metadata.len() - self.read_offset)
                .read_to_end(&mut bytes)?;
            self.read_offset = metadata.len();
            let complete = bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1);
            self.pending = bytes[complete..].to_vec();
            let mut appended = self.decode(&bytes[..complete], start);
            if complete < bytes.len() {
                appended.extend(self.decode(&bytes[complete..], start + complete as u64));
            }
            parsed_bytes += bytes.len() as u64;
            let previous_eof = self
                .records
                .last()
                .is_some_and(|(offset, _)| *offset == start);
            if !appended.is_empty() || previous_eof {
                let affected = self.affected_start(&appended).min(start);
                if previous_eof {
                    self.records.pop();
                }
                self.records.extend(appended);
                replaced = self.replay(affected)?;
            }
        }
        let after_append = self.entries.len();
        let parsed_before_older = parsed_bytes;
        while self.oldest_offset > 0 && self.entries.len() < requested {
            parsed_bytes += self.older_page(&mut file)?;
            // Full history has no intermediate row-count boundary to inspect.
            if requested != usize::MAX || self.oldest_offset == 0 {
                self.replay(self.oldest_offset)?;
            }
        }
        if parsed_bytes > parsed_before_older {
            replaced = replaced.max(shown_before);
        }
        let appended = after_append.saturating_sub(before);
        self.shown = self
            .shown
            .saturating_add(appended)
            .max(requested)
            .min(self.entries.len());
        let prepended = self
            .shown
            .saturating_sub(shown_before)
            .saturating_sub(appended);
        self.boundary.capture(&mut file, metadata.len())?;
        Ok(crate::log::Update {
            events: self
                .entries
                .iter()
                .skip(self.entries.len() - self.shown)
                .map(|entry| entry.event.clone())
                .collect(),
            prepended: if reset { 0 } else { prepended },
            appended: if reset { self.shown } else { appended },
            replaced: replaced.min(shown_before),
            reset,
            parsed_bytes,
            oldest_offset: self.oldest_offset,
            read_offset: self.read_offset,
        })
    }
}

impl crate::log::Reader for Reader {
    fn refresh(&mut self, requested: usize) -> io::Result<crate::log::Update> {
        Reader::refresh(self, requested)
    }
}

#[derive(Default)]
struct Log {
    path: Option<PathBuf>,
    offset: u64,
    companions: std::collections::HashMap<String, Vec<crate::log::SourceRecord>>,
    images: super::images::Companions,
    error: Option<io::Error>,
    entries: Vec<Entry>,
    results: std::collections::HashMap<String, ResultAt>,
    assistant: Option<String>,
    batch: Option<usize>,
    sequence: usize,
}
struct Entry {
    offset: u64,
    event: Event,
    missing: bool,
    background: bool,
    last_result: Option<usize>,
}
struct ResultAt {
    source_records: Vec<crate::log::SourceRecord>,
    at: DateTime<Utc>,
    sequence: usize,
    error: bool,
    denial: Option<String>,
    content: String,
    raw_content: serde_json::Value,
    value: serde_json::Value,
}

fn result_text(value: &serde_json::Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| match block["type"].as_str() {
                Some("text") => block["text"].as_str(),
                Some("tool_reference") => block["tool_name"].as_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn fields(input: &serde_json::Value, keys: &[&str]) -> Section {
    Section::Fields(
        keys.iter()
            .filter_map(|key| {
                input.get(*key).map(|value| {
                    (
                        (*key).to_owned(),
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                    )
                })
            })
            .collect(),
    )
}

fn all_fields(input: &serde_json::Value) -> Section {
    fields(
        input,
        &input
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.as_str() != "description")
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
    )
}

fn text_section(label: &str, text: &str) -> Section {
    Section::Text {
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

fn code_section(label: &str, lang: &str, text: &str) -> Section {
    Section::Code {
        label: label.to_owned(),
        lang: lang.to_owned(),
        text: text.to_owned(),
    }
}

fn line_range(start: u64, count: Option<u64>) -> String {
    match count {
        Some(count) => format!(
            "lines {start}–{}",
            start.saturating_add(count.saturating_sub(1))
        ),
        None => format!("lines {start}–"),
    }
}

fn tool_call(name: &str, input: &serde_json::Value, result: Option<&ResultAt>) -> ToolCall {
    let path = input["file_path"].as_str().unwrap_or("");
    let lang = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let value = result.map(|result| &result.value);
    let mut sections = match name {
        "AskUserQuestion" => {
            let mut lines = Vec::new();
            for question in input["questions"].as_array().into_iter().flatten() {
                let Some(text) = question["question"].as_str() else {
                    continue;
                };
                let suffix = if question["multiSelect"] == true {
                    " (multi-select)"
                } else {
                    ""
                };
                lines.push(format!("• {text}{suffix}"));
                for option in question["options"].as_array().into_iter().flatten() {
                    if let (Some(label), Some(description)) =
                        (option["label"].as_str(), option["description"].as_str())
                    {
                        lines.push(format!("  • {label}: {description}"));
                    }
                }
            }
            vec![text_section("questions", &lines.join("\n"))]
        }
        "Bash" => vec![code_section(
            "command",
            "bash",
            input["command"].as_str().unwrap_or(""),
        )],
        "Read" if input.get("offset").is_some() || input.get("limit").is_some() => {
            let start = input["offset"].as_u64().unwrap_or(1);
            let end = input["limit"].as_u64().map_or_else(String::new, |count| {
                start.saturating_add(count.saturating_sub(1)).to_string()
            });
            vec![text_section("path", &format!("{path}:{start}-{end}"))]
        }
        "Read" | "Write" => vec![text_section("path", path)],
        // The diff's label shows the path.
        "Edit" => Vec::new(),
        "WebFetch" => vec![fields(input, &["url", "prompt"])],
        "Agent" | "Task" => vec![
            Section::Markdown {
                label: "prompt".to_owned(),
                text: input["prompt"].as_str().unwrap_or("").to_owned(),
            },
            fields(input, &["name"]),
        ],
        "SendMessage" => vec![Section::Markdown {
            label: "message".to_owned(),
            text: input["message"].as_str().unwrap_or("").to_owned(),
        }],
        "Skill" => vec![text_section("args", input["args"].as_str().unwrap_or(""))],
        _ if name.starts_with("mcp__") => crate::providers::tool::json_input(input),
        _ => vec![all_fields(input)],
    };
    match name {
        "Edit" => {
            let patch = value.and_then(|value| value["structuredPatch"].as_array());
            let diff = if let Some(hunks) = patch {
                hunks
                    .iter()
                    .map(|hunk| {
                        let header = format!(
                            "@@ -{},{} +{},{} @@",
                            hunk["oldStart"], hunk["oldLines"], hunk["newStart"], hunk["newLines"]
                        );
                        std::iter::once(header)
                            .chain(
                                hunk["lines"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|line| line.as_str().map(str::to_owned)),
                            )
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                [("old_string", "-"), ("new_string", "+")]
                    .into_iter()
                    .flat_map(|(key, prefix)| {
                        input[key]
                            .as_str()
                            .unwrap_or("")
                            .lines()
                            .map(move |line| format!("{prefix}{line}"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            sections.push(Section::Diff {
                label: path.to_owned(),
                text: diff,
            });
        }
        "Write" => sections.push(code_section(
            "content",
            lang,
            input["content"].as_str().unwrap_or(""),
        )),
        _ => {}
    }
    let output = result.map(|result| {
        let fallback = || vec![text_section("output", &result.content)];
        let value = &result.value;
        if name.starts_with("mcp__") {
            return crate::providers::tool::mcp_content(&result.raw_content);
        }
        if name == "AskUserQuestion"
            && result.error
            && let Some((explanation, questions)) = result.content.split_once("Questions asked:")
        {
            return vec![text_section(
                "output",
                &format!(
                    "Questions asked:{}\n\n{}",
                    questions.trim_end(),
                    explanation.trim_end()
                ),
            )];
        }
        if result.error {
            return fallback();
        }
        if matches!(name, "Agent" | "SendMessage") {
            return vec![Section::Markdown {
                label: "output".to_owned(),
                text: result.content.clone(),
            }];
        }
        if !(value.is_object() || value.is_array()) {
            return fallback();
        }
        match name {
            "Bash" => {
                let mut output: Vec<_> = ["stdout", "stderr"]
                    .into_iter()
                    .filter_map(|key| {
                        value[key]
                            .as_str()
                            .filter(|text| !text.is_empty())
                            .map(|text| text_section(key, text))
                    })
                    .collect();
                if output.is_empty() {
                    output.push(text_section("stdout", "(no output)"));
                }
                output
            }
            "Read" if value["file"].is_object() => {
                let file = &value["file"];
                let label = file["startLine"]
                    .as_u64()
                    .map(|start| line_range(start, file["numLines"].as_u64()))
                    .unwrap_or_else(|| "content".to_owned());
                vec![code_section(
                    &label,
                    lang,
                    file["content"].as_str().unwrap_or(""),
                )]
            }
            "WebFetch" if value["result"].is_string() => vec![Section::Markdown {
                label: "output".to_owned(),
                text: value["result"].as_str().unwrap().to_owned(),
            }],
            _ => fallback(),
        }
    });
    if !name.starts_with("mcp__") {
        crate::providers::tool::include_description(&mut sections, input);
    }
    let mut call = ToolCall {
        detail_left: crate::providers::tool::display_tool_name(name),
        detail_title: String::new(),
        show_details: name == "AskUserQuestion",
        id: String::new(),
        title: tool_title(name, input),
        name: name.to_owned(),
        input: sections,
        output,
        exit_code: None,
        is_error: result.is_some_and(|result| result.error),
        duration: None,
    };
    crate::providers::tool::annotate(&mut call);
    call
}

impl Log {
    fn source_records(&self) -> Vec<crate::log::SourceRecord> {
        self.path
            .as_ref()
            .map(|path| crate::log::SourceRecord {
                path: path.clone(),
                offset: self.offset,
            })
            .into_iter()
            .collect()
    }

    fn ready(&self, count: usize) -> bool {
        self.entries.len() > count && self.batch.is_none_or(|index| index > count)
    }

    fn record(&mut self, record: serde_json::Value, count: usize) -> bool {
        match self.record_result(record, count) {
            Ok(keep_reading) => keep_reading,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn record_result(&mut self, record: serde_json::Value, count: usize) -> io::Result<bool> {
        self.images.record(&record);
        if record["turnCompanion"] == true
            && let Some(id) = record["promptId"].as_str()
        {
            let source_records = self.source_records();
            self.companions
                .entry(id.to_owned())
                .or_default()
                .extend(source_records);
        }
        self.sequence += 1;
        if record["type"] == "assistant" {
            let key = record["message"]["id"]
                .as_str()
                .or_else(|| record["requestId"].as_str())
                .or_else(|| record["uuid"].as_str())
                .map(str::to_owned);
            if key.is_none() || key != self.assistant {
                self.batch = None;
                self.assistant = key;
                if self.ready(count) {
                    return Ok(false);
                }
            }
        }
        let Some(at) = record["timestamp"]
            .as_str()
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|at| at.with_timezone(&Utc))
        else {
            return Ok(!self.ready(count));
        };
        let content = &record["message"]["content"];
        match record["type"].as_str() {
            Some("user") => {
                let entry_count = self.entries.len();
                if let Some(blocks) = content.as_array() {
                    for block in blocks {
                        if block["type"] == "tool_result"
                            && let Some(id) = block["tool_use_id"].as_str()
                        {
                            let source_records = self.source_records();
                            let result = self.results.entry(id.to_owned()).or_insert(ResultAt {
                                source_records: Vec::new(),
                                at,
                                sequence: self.sequence,
                                error: false,
                                denial: None,
                                content: result_text(&block["content"]),
                                raw_content: block["content"].clone(),
                                value: record["toolUseResult"].clone(),
                            });
                            result.source_records.extend(source_records);
                            result.error |= block["is_error"] == true;
                            if block["is_error"] == true {
                                if record["toolDenialKind"] == "automode-blocked" {
                                    result.denial = Some(
                                        block["content"]
                                            .as_str()
                                            .and_then(|text| text.split_once("Reason: ["))
                                            .and_then(|(_, text)| text.split_once(']'))
                                            .map(|(reason, _)| reason)
                                            .unwrap_or("Dangerous")
                                            .to_owned(),
                                    );
                                } else if record["permissionDecision"]["reasonType"]
                                    == "safetyCheck"
                                {
                                    result.denial = Some("Safety Check".to_owned());
                                }
                            }
                        }
                    }
                }
                let text = content.as_str().map(str::to_owned).or_else(|| {
                    Some(
                        content
                            .as_array()?
                            .iter()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                });
                if record["isMeta"] != true
                    && let Some(text) = text.filter(|text| !text.trim().is_empty())
                    && !text.trim_start().starts_with("<task-notification>")
                {
                    if let Some((kind, sender, body)) = wrapped_message(&text) {
                        self.push(kind, body, at);
                        self.entries.last_mut().unwrap().event.source =
                            record["uuid"].as_str().map(str::to_owned);
                        self.entries.last_mut().unwrap().event.sender = sender;
                    } else if !text.trim_start().starts_with('<') {
                        let images = self.images.take(&record, &text)?;
                        let kind = if record["isCompactSummary"] == true {
                            Kind::Compaction
                        } else {
                            Kind::User
                        };
                        self.push(kind, text, at);
                        self.entries.last_mut().unwrap().event.source =
                            record["uuid"].as_str().map(str::to_owned);
                        self.entries.last_mut().unwrap().event.images = images;
                        if let Some(records) = record["promptId"]
                            .as_str()
                            .and_then(|id| self.companions.remove(id))
                        {
                            self.entries
                                .last_mut()
                                .unwrap()
                                .event
                                .source_records
                                .extend(records);
                        }
                    }
                }
                // A hidden turn starter is still the immediately preceding event.
                if self.entries.len() == entry_count
                    && !content.as_array().is_some_and(|blocks| {
                        blocks.iter().any(|block| block["type"] == "tool_result")
                    })
                    && let Some(entry) = self.entries.last_mut()
                    && entry.event.kind == Kind::Assistant
                    && entry.event.previous_end.is_none()
                {
                    entry.event.previous_end = Some(at);
                }
            }
            Some("assistant") => {
                if let Some(blocks) = content.as_array() {
                    for (index, block) in blocks.iter().enumerate().rev() {
                        match block["type"].as_str() {
                            Some("text") => {
                                if let Some(text) = block["text"].as_str() {
                                    self.push(Kind::Assistant, text.to_owned(), at);
                                    self.entries.last_mut().unwrap().event.source = record["uuid"]
                                        .as_str()
                                        .map(|uuid| format!("{uuid}:text:{index}"));
                                }
                            }
                            Some("tool_use") => self.tool(block, at),
                            _ => {}
                        }
                    }
                }
            }
            Some("queue-operation") if record["operation"] == "enqueue" => {
                if let Some(text) = record["content"].as_str() {
                    if self.notification(text, at, record["uuid"].as_str()) {
                        return Ok(!self.ready(count));
                    }
                    let (kind, body) = if text.starts_with('<') {
                        (Kind::QueuedSystem, text)
                    } else {
                        (Kind::QueuedUser, text)
                    };
                    let (sender, body) = wrapped_message(body)
                        .map(|(_, sender, body)| (sender, body))
                        .unwrap_or_else(|| (None, body.to_owned()));
                    let images = self.images.take(&record, &body)?;
                    self.push(kind, body, at);
                    self.entries.last_mut().unwrap().event.images = images;
                    self.entries.last_mut().unwrap().event.source =
                        record["uuid"].as_str().map(str::to_owned);
                    self.entries.last_mut().unwrap().event.sender = sender;
                }
            }
            _ => {}
        }
        Ok(!self.ready(count))
    }

    fn notification(&mut self, text: &str, at: DateTime<Utc>, source: Option<&str>) -> bool {
        let Some(notification) = text
            .trim()
            .strip_prefix("<task-notification>")
            .and_then(|text| {
                text.split_once("</task-notification>")
                    .map(|(body, _)| body)
            })
        else {
            return false;
        };
        let field = |key: &str| {
            notification
                .split_once(&format!("<{key}>"))
                .and_then(|(_, text)| text.split_once(&format!("</{key}>")))
                .map(|(value, _)| value.trim())
        };
        let summary = field("summary").unwrap_or_else(|| {
            notification
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("")
        });
        // The output file's content, or its path once the file is gone.
        let output = field("output-file").map(|path| match std::fs::read_to_string(path) {
            Ok(text) => Section::Text {
                label: String::new(),
                text,
            },
            Err(_) => Section::Fields(vec![("output-file".to_owned(), path.to_owned())]),
        });
        self.push(Kind::Tool, summary.to_owned(), at);
        let event = &mut self.entries.last_mut().unwrap().event;
        event.source = source.map(str::to_owned);
        if field("task-id").is_some() || output.is_some() {
            event.calls.push(ToolCall {
                detail_left: format!("Task {}", field("task-id").unwrap_or("")),
                detail_title: summary.to_owned(),
                show_details: false,
                id: field("task-id").unwrap_or("").to_owned(),
                title: summary.to_owned(),
                name: "task-notification".to_owned(),
                input: Vec::new(),
                output: Some(output.into_iter().collect()),
                exit_code: None,
                is_error: false,
                duration: None,
            });
        }
        true
    }

    fn push(&mut self, kind: Kind, body: String, started_at: DateTime<Utc>) {
        let ended_at = matches!(kind, Kind::Tool).then_some(started_at);
        self.entries.push(Entry {
            offset: self.offset,
            event: Event {
                completed_time: false,
                background: body.ends_with(" *"),
                source: None,
                source_records: self.source_records(),
                sender: None,
                kind,
                body,
                paths: Vec::new(),
                started_at,
                ended_at,
                duration: None,
                previous_end: None,
                images: Vec::new(),
                calls: Vec::new(),
            },
            missing: false,
            background: false,
            last_result: None,
        });
    }

    fn tool(&mut self, block: &serde_json::Value, at: DateTime<Utc>) {
        let Some(name) = block["name"].as_str() else {
            return;
        };
        if tool_hidden(name) {
            return;
        }
        let input = &block["input"];
        let result = block["id"].as_str().and_then(|id| self.results.get(id));
        let detail = default_detail(input);
        let path = detail
            .filter(|(key, _)| *key == "file_path")
            .filter(|_| name != "Bash")
            .filter(|_| result.is_none_or(|result| result.denial.is_none()))
            .map(|(_, path)| path);
        let detail = tool_detail(name, input);
        let display_name = crate::providers::tool::display_tool_name(name);
        let call = if let Some(reason) = result.and_then(|result| result.denial.as_deref()) {
            if display_name == name {
                format!("{display_name}: {reason}")
            } else {
                tool_label(name, reason)
            }
        } else if matches!(name, "AskUserQuestion" | "Skill") {
            tool_title(name, input)
        } else if let Some(path) = path {
            tool_label(name, path)
        } else {
            tool_label(name, &detail)
        };
        let kind = tool_kind(name);
        if self.batch.is_some_and(|index| {
            (self.entries[index].event.kind == Kind::Assistant) != (kind == Kind::Assistant)
        }) {
            self.batch = None;
        }
        let mut entry = if let Some(index) = self.batch.take() {
            self.entries.remove(index)
        } else {
            Entry {
                offset: self.offset,
                event: Event {
                    completed_time: false,
                    background: false,
                    source: None,
                    source_records: self.source_records(),
                    sender: None,
                    kind,
                    body: String::new(),
                    paths: Vec::new(),
                    started_at: at,
                    ended_at: None,
                    duration: None,
                    previous_end: None,
                    images: Vec::new(),
                    calls: Vec::new(),
                },
                missing: false,
                background: false,
                last_result: None,
            }
        };
        entry.offset = self.offset;
        entry.event.source_records.extend(self.source_records());
        if let Some(result) = result {
            entry
                .event
                .source_records
                .extend(result.source_records.clone());
        }
        entry
            .event
            .source_records
            .sort_by_key(|record| record.offset);
        entry.event.source_records.dedup();
        let body = entry
            .event
            .body
            .strip_suffix(" *")
            .filter(|_| entry.background)
            .unwrap_or(&entry.event.body);
        let path = path.map(|path| call.len() - path.len()..call.len());
        // A path call followed by a path call of the same tool shares its verb: "Edit a b".
        let verb = path
            .as_ref()
            .map(|range| call[..range.start].to_owned())
            .unwrap_or_default();
        let shared = path.is_some()
            && entry
                .event
                .calls
                .first()
                .is_some_and(|call| call.name == name)
            && body.starts_with(&verb)
            && entry.event.paths.first().map(|range| range.start) == Some(verb.len());
        let (body, call) = if shared {
            for range in &mut entry.event.paths {
                *range = range.start - verb.len()..range.end - verb.len();
            }
            let separator = if display_name == name { " " } else { " · " };
            (&body[verb.len()..], format!("{call}{separator}"))
        } else if body.is_empty() {
            (body, call)
        } else {
            (body, format!("{call}，"))
        };
        for range in &mut entry.event.paths {
            *range = range.start + call.len()..range.end + call.len();
        }
        entry.event.paths.splice(0..0, path);
        entry.event.body = format!("{call}{body}");
        let mut detail = tool_call(name, input, result);
        detail.id = block["id"].as_str().unwrap_or("").to_owned();
        detail.duration = result.map(|result| result.at - at);
        if name == "Agent"
            && let Some(kind) = input_text(input, "subagent_type")
        {
            detail.detail_left = kind.to_owned();
        }
        entry.event.calls.insert(0, detail);
        entry.background |= input["run_in_background"] == true;
        if entry.background {
            entry.event.body.push_str(" *");
        }
        entry.event.background = entry.event.body.ends_with(" *");
        entry.event.started_at = at;
        if let Some(result) = result {
            if result.denial.is_some() {
                entry.event.kind = Kind::Denied;
            } else if result.error && entry.event.kind != Kind::Denied {
                entry.event.kind = Kind::Error;
            }
            if !entry.missing
                && entry
                    .last_result
                    .is_none_or(|sequence| result.sequence < sequence)
            {
                entry.event.ended_at = Some(result.at);
                entry.last_result = Some(result.sequence);
            }
        } else {
            entry.missing = true;
            entry.event.ended_at = None;
        }
        self.batch = Some(self.entries.len());
        self.entries.push(entry);
    }
}

/// Extracts the visible text of supported harness messages.
fn wrapped_message(text: &str) -> Option<(Kind, Option<String>, String)> {
    if let Some(body) = pasted_content(text) {
        return Some((Kind::User, None, user_text(&body)));
    }
    let (kind, (name, body)) = super::peer::wrapped_message(text)?;
    let body = body.trim().to_owned();
    Some((
        kind,
        name.map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
        body,
    ))
}

/// Replaces each `<pasted_content>` block with its content, or `None` when
/// the text has none.
pub(super) fn pasted_content(mut text: &str) -> Option<String> {
    let mut body = String::new();
    let mut found = false;
    while let Some(start) = text.find("<pasted_content") {
        body.push_str(&text[..start]);
        let Some((opening, rest)) = text[start + 1..].split_once('>') else {
            body.push_str(&text[start..]);
            text = "";
            break;
        };
        let closing = format!("</{opening}>");
        let end = (opening.split_whitespace().next() == Some("pasted_content"))
            .then(|| {
                [closing.as_str(), "</pasted_content>"]
                    .into_iter()
                    .filter_map(|tag| rest.find(tag).map(|index| (index, tag.len())))
                    .min_by_key(|(index, _)| *index)
            })
            .flatten();
        if let Some((end, closing_len)) = end {
            let inner = rest[..end]
                .strip_prefix('\n')
                .unwrap_or(&rest[..end])
                .split_inclusive('\n')
                .map(|line| line.strip_prefix("  ").unwrap_or(line))
                .collect::<String>();
            // A paste nested in a paste has its tags escaped with a backslash.
            let inner = inner
                .replace("<\\pasted_content", "<pasted_content")
                .replace("<\\/pasted_content", "</pasted_content");
            body.push_str(&pasted_content(&inner).unwrap_or(inner));
            text = &rest[end + closing_len..];
            found = true;
        } else {
            body.push_str(&text[start..start + opening.len() + 2]);
            text = rest;
        }
    }
    body.push_str(text);
    found.then_some(body)
}

fn user_text(text: &str) -> String {
    text.lines()
        .filter(|line| {
            !line.trim().strip_prefix("```").is_some_and(|suffix| {
                suffix.is_empty()
                    || suffix
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '+' | '#' | '.'))
            })
        })
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn mcp_real_nested_arguments_are_a_single_json_section() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/mcp-claude/nested.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let input = records[0]["message"]["content"][0]["input"].clone();
        let events = events_from(records);
        assert_eq!(
            events[0].calls[0].input,
            vec![Section::Json {
                label: "input".into(),
                text: input.to_string(),
            }]
        );
    }

    #[test]
    fn mcp_arguments_keep_all_json_shapes_and_description() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/mcp-claude/nested.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let query = &records[0]["message"]["content"][0]["input"]["query"];
        for input in [
            Value::Null,
            query["parameters"]["filters"].clone(),
            query["dry"].clone(),
            query["limit"].clone(),
            query["view"].clone(),
            json!({"description": query["view"]}),
        ] {
            let mut records = records.clone();
            records[0]["message"]["content"][0]["input"] = input.clone();
            let events = events_from(records);
            let expected = if input.is_null() {
                Vec::new()
            } else {
                vec![Section::Json {
                    label: "input".into(),
                    text: input.to_string(),
                }]
            };
            assert_eq!(events[0].calls[0].input, expected);
        }
    }

    #[test]
    fn mcp_real_image_result_keeps_placeholder_and_text() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/mcp-claude/image.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let content = records[1]["message"]["content"][0]["content"].clone();
        let expected: Vec<_> = content
            .as_array()
            .unwrap()
            .iter()
            .map(|block| {
                if block["type"] == "image" {
                    Section::Placeholder(format!(
                        "[image {}]",
                        block["source"]["media_type"].as_str().unwrap()
                    ))
                } else {
                    text_section("output", block["text"].as_str().unwrap())
                }
            })
            .collect();
        for copy in [records[1]["toolUseResult"].clone(), Value::Null] {
            let mut records = records.clone();
            records[1]["toolUseResult"] = copy;
            let events = events_from(records);
            assert_eq!(events[0].calls[0].output, Some(expected.clone()));
        }
    }

    #[test]
    fn mcp_real_results_use_content_without_metadata_duplication() {
        for source in [
            include_str!("../../../tests/fixtures/mcp-claude/structured.jsonl"),
            include_str!("../../../tests/fixtures/mcp-claude/error.jsonl"),
        ] {
            let records: Vec<Value> = source
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let content = &records[1]["message"]["content"][0]["content"];
            let expected = if let Some(text) = content.as_str() {
                vec![text_section("output", text)]
            } else {
                content
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|block| text_section("output", block["text"].as_str().unwrap()))
                    .collect()
            };
            let is_error = records[1]["message"]["content"][0]["is_error"] == true;
            let events = events_from(records);
            assert_eq!(events[0].calls[0].output, Some(expected));
            assert_eq!(events[0].calls[0].is_error, is_error);
        }
    }

    #[test]
    fn raw_source_records_include_each_consumed_result_line() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let lines: Vec<_> = include_str!("../../../tests/fixtures/claude-raw-detail.jsonl")
            .lines()
            .collect();
        for line in lines.iter().chain(std::iter::once(&lines[2])) {
            writeln!(file, "{line}").unwrap();
        }
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let update = reader.refresh(usize::MAX).unwrap();
        let expected = [lines[1], lines[2], lines[2]]
            .into_iter()
            .map(|line| {
                serde_json::to_string_pretty(&serde_json::from_str::<Value>(line).unwrap()).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(update.events.last().unwrap().raw_text().unwrap(), expected);
    }

    #[test]
    fn raw_source_records_preserve_real_call_and_result_order() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut file,
            include_bytes!("../../../tests/fixtures/claude-raw-detail.jsonl"),
        )
        .unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let update = reader.refresh(usize::MAX).unwrap();
        let rows: Vec<serde_json::Value> =
            include_str!("../../../tests/fixtures/claude-raw-detail.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let expected = rows[1..]
            .iter()
            .map(|row| serde_json::to_string_pretty(row).unwrap())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(update.events.last().unwrap().raw_text().unwrap(), expected);
        assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        assert_eq!(
            update.events[0].raw_text().unwrap(),
            serde_json::to_string_pretty(&rows[0]).unwrap()
        );
    }

    #[test]
    fn task_notification_delivery_keeps_only_enqueue_rows() {
        use std::io::Write;
        let fixture =
            include_str!("../../../tests/fixtures/claude-task-notification-delivery.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let mut enqueued = Vec::new();
        for line in fixture.lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            writeln!(file, "{line}").unwrap();
            if record["type"] == "queue-operation" {
                enqueued.push(record.clone());
            }
            let update = reader.refresh(usize::MAX).unwrap();
            if record["type"] != "queue-operation" {
                assert_eq!(update.appended, 0, "delivery must not append a row");
            }
            let events = super::tail(file.path(), usize::MAX).unwrap();
            assert_eq!(update.events, events);
            assert_eq!(events.len(), enqueued.len(), "delivery must not add a row");
            for enqueue in &enqueued {
                let text = enqueue["content"].as_str().unwrap();
                let id = text
                    .split_once("<task-id>")
                    .unwrap()
                    .1
                    .split_once("</task-id>")
                    .unwrap()
                    .0;
                let rows: Vec<_> = events
                    .iter()
                    .filter(|event| event.calls[0].id == id)
                    .collect();
                assert_eq!(rows.len(), 1, "task {id}");
                let at =
                    chrono::DateTime::parse_from_rfc3339(enqueue["timestamp"].as_str().unwrap())
                        .unwrap();
                assert_eq!(rows[0].started_at, at);
            }
        }
    }

    #[test]
    #[ignore = "requires AGENTS_MEASURE_TRANSCRIPT pointing to a real large transcript"]
    fn measure_incremental_transcript_bytes() {
        use std::io::Write;
        let source = std::env::var_os("AGENTS_MEASURE_TRANSCRIPT").unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::copy(&mut std::fs::File::open(source).unwrap(), &mut file).unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let initial = reader.refresh(50).unwrap();
        let notification = include_str!("../../../tests/fixtures/claude-task-notifications.jsonl")
            .split_inclusive('\n')
            .find(|line| line.contains("<task-notification>"))
            .unwrap();
        file.write_all(notification.as_bytes()).unwrap();
        super::super::transcript::take_parsed_bytes();
        let baseline = super::tail(file.path(), 50).unwrap();
        let before = super::super::transcript::take_parsed_bytes();
        let after = reader.refresh(50).unwrap();
        assert_eq!(
            &after.events[after.events.len() - baseline.len()..],
            baseline
        );
        assert_eq!(after.parsed_bytes, notification.len() as u64);
        println!(
            "transcript_bytes={} initial_parsed_bytes={} before_refresh_bytes={} after_refresh_bytes={} appended_record_bytes={} events={}",
            file.as_file().metadata().unwrap().len(),
            initial.parsed_bytes,
            before,
            after.parsed_bytes,
            notification.len(),
            after.events.len()
        );
    }

    #[test]
    fn incremental_older_pages_and_resets_match_real_transcript() {
        use std::io::Write;
        let fixture = include_bytes!("../../../tests/fixtures/claude-bg-metadata.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(fixture).unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let tail = reader.refresh(1).unwrap();
        assert!(tail.oldest_offset > 0);
        let complete = reader.refresh(usize::MAX).unwrap();
        assert_eq!(complete.oldest_offset, 0);
        assert!(complete.replaced >= tail.events.len());
        assert_eq!(
            complete.events,
            super::tail(file.path(), usize::MAX).unwrap()
        );
        assert_eq!(
            complete.prepended,
            complete.events.len() - tail.events.len()
        );
        assert_eq!(
            tail.parsed_bytes + complete.parsed_bytes,
            fixture.len() as u64
        );
        file.as_file().set_len(0).unwrap();
        let reset = reader.refresh(usize::MAX).unwrap();
        assert!(reset.reset);
        assert!(reset.events.is_empty());
    }

    #[test]
    fn full_history_replays_each_retained_record_once() {
        use std::io::Write;
        let fixture = include_bytes!("../../../tests/fixtures/claude-bg-metadata.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(fixture).unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let tail = reader.refresh(1).unwrap();
        assert!(tail.oldest_offset > 0);
        let replayed = reader.replayed_records;
        let complete = reader.refresh(usize::MAX).unwrap();
        assert_eq!(complete.oldest_offset, 0);
        assert_eq!(
            reader.replayed_records - replayed,
            reader.records.len(),
            "expanding full history must replay retained records only once"
        );
        assert_eq!(
            complete.events,
            super::tail(file.path(), usize::MAX).unwrap()
        );
        assert_eq!(
            tail.parsed_bytes + complete.parsed_bytes,
            fixture.len() as u64
        );

        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let complete = reader.refresh(usize::MAX).unwrap();
        assert_eq!(reader.replayed_records, reader.records.len());
        assert_eq!(
            complete.events,
            super::tail(file.path(), usize::MAX).unwrap()
        );
    }

    #[test]
    fn incremental_valid_unterminated_record_is_not_duplicated_by_newline() {
        use std::io::Write;
        let lines: Vec<_> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .take(2)
            .collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(lines[0].as_bytes()).unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let first = reader.refresh(usize::MAX).unwrap();
        assert!(!first.events.is_empty());
        assert_eq!(first.events, super::tail(file.path(), usize::MAX).unwrap());
        writeln!(file).unwrap();
        assert_eq!(reader.refresh(usize::MAX).unwrap().events, first.events);
        file.write_all(lines[1].as_bytes()).unwrap();
        assert_eq!(
            reader.refresh(usize::MAX).unwrap().events,
            super::tail(file.path(), usize::MAX).unwrap()
        );
        writeln!(file).unwrap();
        assert_eq!(
            reader.refresh(usize::MAX).unwrap().events,
            super::tail(file.path(), usize::MAX).unwrap()
        );
    }

    #[test]
    fn incremental_partial_record_waits_for_newline() {
        use std::io::Write;
        let line = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .split_inclusive('\n')
            .next()
            .unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let split = line.len() / 2;
        file.write_all(&line.as_bytes()[..split]).unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        assert!(reader.refresh(usize::MAX).unwrap().events.is_empty());
        file.write_all(&line.as_bytes()[split..]).unwrap();
        let update = reader.refresh(usize::MAX).unwrap();
        assert_eq!(update.events, super::tail(file.path(), usize::MAX).unwrap());
        assert_eq!(update.parsed_bytes, line.len() as u64);
    }

    #[test]
    fn incremental_real_transcripts_match_full_parse() {
        use std::io::Write;
        for fixture in [
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl"),
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl"),
            include_str!("../../../tests/fixtures/claude-prompt-images.jsonl"),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let mut reader = super::Reader::new(file.path().to_owned(), false);
            for line in fixture.split_inclusive('\n') {
                file.write_all(line.as_bytes()).unwrap();
                let update = reader.refresh(usize::MAX).unwrap();
                assert_eq!(update.events, super::tail(file.path(), usize::MAX).unwrap());
                assert_eq!(update.parsed_bytes, line.len() as u64);
            }
            assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        }
    }

    #[test]
    fn real_mcp_calls_share_display_names_and_keep_raw_names() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-tool-calls.jsonl");
        let events = super::tail(&path, 100).unwrap();
        let event = events
            .iter()
            .find(|event| {
                event.calls.len() == 2
                    && event
                        .calls
                        .iter()
                        .all(|call| call.name == "mcp__plugin_codex_codex-manager__start")
            })
            .unwrap();
        assert!(event.body.contains("codex-manager start"));
        assert!(!event.body.contains("mcp__"));
        assert!(
            event
                .calls
                .iter()
                .all(|call| call.title == "codex-manager start")
        );
        let record: serde_json::Value =
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .find(|record| record["message"]["content"][0]["id"] == event.calls[0].id)
                .unwrap();
        assert_eq!(
            super::tool_activity(&record["message"]["content"][0]).unwrap(),
            "codex-manager start"
        );
        let input = &record["message"]["content"][0]["input"];
        let prompt = input["prompt"].as_str().unwrap();
        assert!(
            event
                .body
                .contains(&format!("codex-manager start · {prompt}"))
        );
        assert_eq!(
            super::tool_label("mcp__plugin_codex_codex-manager__start", ""),
            "codex-manager start"
        );
        assert_eq!(
            super::tool_label("mcp__tool", prompt),
            format!("mcp__tool {prompt}")
        );
        assert_eq!(
            super::tool_label("mcp__plugin_codex_codex-manager__send", prompt),
            format!("codex-manager send · {prompt}")
        );
        let url = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["message"]["content"][0]["name"] == "WebFetch")
            .unwrap()["message"]["content"][0]["input"]["url"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            super::tool_label("mcp__claude-in-chrome__navigate", &url),
            format!("claude-in-chrome navigate · {url}")
        );
    }

    #[test]
    fn skill_rows_and_titles_use_name_and_optional_args() {
        use std::io::Write;
        let fixture = include_str!("../../../tests/fixtures/claude-skill-call.jsonl");
        for args in [Some("Visual Studio Code"), Some(""), None] {
            let mut records: Vec<serde_json::Value> = fixture
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let input = records[0]["message"]["content"][0]["input"]
                .as_object_mut()
                .unwrap();
            match args {
                Some(args) => {
                    input.insert("args".to_owned(), args.into());
                }
                None => {
                    input.remove("args");
                }
            }
            let mut file = tempfile::NamedTempFile::new().unwrap();
            for record in records {
                writeln!(file, "{record}").unwrap();
            }
            let events = super::tail(file.path(), 100).unwrap();
            assert_eq!(events.len(), 1);
            let expected = if args == Some("Visual Studio Code") {
                "computer-use:capture-window Visual Studio Code"
            } else {
                "computer-use:capture-window"
            };
            assert_eq!(events[0].body, expected);
            assert_eq!(events[0].calls[0].title, expected);
            assert_eq!(events[0].calls[0].name, "Skill");
        }
    }

    #[test]
    fn agent_subagent_type_preserves_input_including_general_purpose() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-agent-display.jsonl");
        let events = tail(&fixture, 10).unwrap();
        let types: Vec<_> = events
            .iter()
            .flat_map(|event| &event.calls)
            .map(|call| Some(call.detail_left.as_str()))
            .collect();
        assert_eq!(
            types,
            [
                Some("general-purpose"),
                Some("fork"),
                Some("feature:ux-implementer")
            ]
        );
    }

    #[test]
    fn real_call_titles_and_ids_match_inputs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-tool-calls.jsonl");
        let events = super::tail(&path, 100).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let bash = calls.iter().find(|call| call.name == "Bash").unwrap();
        assert_eq!(bash.title, "改到持久目录");
        assert_eq!(bash.id, "toolu_01XU5dCe5eBxX8ccqC2DwN2G");
        let agent = calls.iter().find(|call| call.name == "Agent").unwrap();
        assert_eq!(agent.title, "Agent");
        let skill = calls.iter().find(|call| call.name == "Skill").unwrap();
        assert_eq!(skill.title, "secrets:secrets-manager [scrubbed]");
        assert_eq!(skill.input, vec![super::text_section("args", "[scrubbed]")]);
        let records: Vec<serde_json::Value> =
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let send = records
            .iter()
            .flat_map(|record| {
                record["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .find(|block| block["name"] == "SendMessage")
            .unwrap();
        assert_eq!(
            super::tool_call("SendMessage", &send["input"], None).title,
            format!("To {}", send["input"]["to"].as_str().unwrap())
        );
    }

    #[test]
    fn calls_time_from_their_request_to_their_result() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-tool-calls.jsonl");
        let events = super::tail(&path, 100).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let call = |id: &str| calls.iter().find(|call| call.id == id).unwrap();
        assert_eq!(
            call("toolu_01XU5dCe5eBxX8ccqC2DwN2G").duration,
            Some(chrono::Duration::milliseconds(46_490))
        );
        assert_eq!(call("toolu_01Jj8h2kKJBp4jXGRdtZu6Ys").duration, None);
    }

    #[test]
    fn tool_inputs_retain_descriptions_for_detail_headers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-tool-calls.jsonl");
        let events = super::tail(&path, 100).unwrap();
        for name in ["Bash", "Agent"] {
            let call = events
                .iter()
                .flat_map(|event| &event.calls)
                .find(|call| call.name == name)
                .unwrap();

            assert!(!call.detail_title.is_empty());
            assert!(!call.input.iter().any(|section| matches!(section,
                Section::Fields(fields) if fields.iter().any(|(key, _)| key == "description"))));
        }
    }

    #[test]
    fn queued_prompt_images_follow_delivery_in_incremental_and_tail_reads() {
        // Transcript e9f4ef31, record 4800; only the PNG and local cwd are reduced.
        let delivery: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-queued-image.jsonl"
        ))
        .unwrap();
        let text = delivery["attachment"]["prompt"][0]["text"]
            .as_str()
            .unwrap();
        let enqueue = json!({
            "type": "queue-operation", "operation": "enqueue", "content": text,
            "timestamp": "2026-10-17T02:58:05.000Z"
        });
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{enqueue}").unwrap();
        let mut reader = Reader::new(file.path().to_owned(), false);
        assert!(reader.refresh(20).unwrap().events[0].images.is_empty());
        // Queue records identify absorbed commands on removal, not enqueue.
        let remove = json!({
            "type": "queue-operation", "operation": "remove", "content": text,
            "commandUuid": delivery["attachment"]["source_uuid"],
            "reason": "absorbed_mid_turn", "timestamp": delivery["timestamp"]
        });
        writeln!(file, "{remove}").unwrap();
        reader.refresh(20).unwrap();
        writeln!(file, "{delivery}").unwrap();
        let events = reader.refresh(20).unwrap().events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::QueuedUser);
        assert_eq!(events[0].body, text);
        assert_eq!(events[0].images.len(), 1);
        assert_eq!(events[0].images[0].label, "[Image #1]");
        let bytes = std::fs::read(&events[0].images[0].path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(super::tail(file.path(), 20).unwrap(), events);
        let summary =
            super::super::transcript::read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap();
        assert_eq!(summary.prompt, text);
        assert_eq!(summary.prompt_images, events[0].images);
    }

    #[test]
    fn prompt_images_follow_their_own_user_events() {
        let (dir, records) = super::super::transcript::tests::image_fixture();
        let path = dir.path().join("transcript.jsonl");
        std::fs::write(
            &path,
            records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>(),
        )
        .unwrap();
        for missing in [false, true] {
            if missing {
                std::fs::remove_file(dir.path().join("images/8.png")).unwrap();
            }
            let events = super::tail(&path, 20).unwrap();
            assert_eq!(events.len(), 2);
            for (event, numbers) in events
                .iter()
                .zip([vec![6], if missing { vec![9] } else { vec![8, 9] }])
            {
                assert_eq!(event.kind, crate::log::Kind::User);
                assert_eq!(
                    event.images,
                    numbers
                        .into_iter()
                        .map(|number| crate::log::Image {
                            label: format!("[Image #{number}]"),
                            path: dir.path().join(format!("images/{number}.png")),
                        })
                        .collect::<Vec<_>>()
                );
            }
            assert_eq!(super::tail(&path, 1).unwrap()[0].images, events[1].images);
        }
    }

    use super::*;
    use serde_json::{Value, json};
    use std::io::Write;

    #[test]
    fn tool_calls_keep_input_and_result_sections() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records.clone());
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        assert!(calls.len() >= 10);
        let bash = calls.iter().find(|call| call.name == "Bash").unwrap();
        assert!(matches!(&bash.input[0], Section::Shell { prefix: 0, .. }));
        let output = bash.output.as_ref().unwrap();
        assert!(matches!(&output[0], Section::Text { label, .. } if label == "stdout"));
        assert!(matches!(&output[1], Section::Label(label) if label == "stderr"));
        let read = calls.iter().find(|call| call.name == "Read").unwrap();
        assert!(
            matches!(&read.output.as_ref().unwrap()[0], Section::Code { lang, text, .. }
            if lang == "conf" && !text.contains("1→"))
        );
        let raw_file = &records[3]["toolUseResult"]["file"];
        assert_eq!(
            read.output.as_ref().unwrap()[0],
            code_section(
                &line_range(
                    raw_file["startLine"].as_u64().unwrap(),
                    raw_file["numLines"].as_u64()
                ),
                "conf",
                raw_file["content"].as_str().unwrap()
            )
        );
        let read_input = &records
            .iter()
            .map(|record| &record["message"]["content"][0])
            .find(|block| block["name"] == "Read")
            .unwrap()["input"];
        let offset = read_input["offset"].as_u64().unwrap();
        assert_eq!(
            read.input,
            [text_section(
                "path",
                &format!(
                    "{}:{offset}-{}",
                    read_input["file_path"].as_str().unwrap(),
                    offset + read_input["limit"].as_u64().unwrap() - 1
                )
            )]
        );
        let edit = calls.iter().find(|call| call.name == "Edit").unwrap();
        assert_eq!(edit.input.len(), 1);
        assert!(matches!(&edit.input[0], Section::Diff { text, .. } if text.starts_with("@@ -")));
        let patch = &records[5]["toolUseResult"]["structuredPatch"][0];
        if let Section::Diff { text, .. } = &edit.input[0] {
            assert!(text.starts_with(&format!(
                "@@ -{},{} +{},{} @@\n",
                patch["oldStart"], patch["oldLines"], patch["newStart"], patch["newLines"]
            )));
            assert!(
                text.ends_with(
                    patch["lines"]
                        .as_array()
                        .unwrap()
                        .last()
                        .unwrap()
                        .as_str()
                        .unwrap()
                )
            );
        }
        let mcp = calls
            .iter()
            .find(|call| call.name.starts_with("mcp__"))
            .unwrap();
        assert!(
            matches!(&mcp.output.as_ref().unwrap()[0], Section::Text { text, .. }
            if serde_json::from_str::<Value>(text).unwrap().is_object())
        );
        let parallel = events
            .iter()
            .find(|event| {
                event.calls.len() == 2
                    && event
                        .calls
                        .iter()
                        .all(|call| call.name.starts_with("mcp__"))
            })
            .unwrap();
        assert_eq!(
            parallel
                .calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>(),
            [
                "mcp__plugin_codex_codex-manager__start",
                "mcp__plugin_codex_codex-manager__start"
            ]
        );
        for (call, record) in parallel.calls.iter().zip(&records[16..18]) {
            let input = &record["message"]["content"][0]["input"];
            assert_eq!(call.detail_title, "codex-manager start");
            assert_eq!(call.input, crate::providers::tool::json_input(input));
        }
        assert!(parallel.calls[1].output.is_none());
        for (name, input_index, output_index) in
            [("Write", 6, 7), ("WebFetch", 8, 9), ("Agent", 10, 11)]
        {
            let call = calls.iter().find(|call| call.name == name).unwrap();
            let raw_input = &records[input_index]["message"]["content"][0]["input"];
            let raw_result = &records[output_index]["toolUseResult"];
            match name {
                "Write" => assert!(
                    matches!(&call.input[1], Section::Code { text, lang, .. } if text == raw_input["content"].as_str().unwrap() && lang == "md")
                ),
                "WebFetch" => assert!(
                    matches!(&call.output.as_ref().unwrap()[0], Section::Markdown { text, .. } if text == raw_result["result"].as_str().unwrap())
                ),
                "Agent" => assert!(
                    matches!(&call.input[0], Section::Markdown { text, .. } if text == raw_input["prompt"].as_str().unwrap())
                ),
                _ => unreachable!(),
            }
        }
        assert!(calls.iter().any(|call| call.is_error));
    }

    #[test]
    fn tool_reference_results_keep_tool_names_on_separate_lines() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-search.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let name = "mcp__plugin_codex_codex-manager__send";
        let reference = records[1]["message"]["content"][0]["content"][0].clone();
        for (content, expected) in [
            (json!([reference]), name.to_owned()),
            (json!([reference, reference]), format!("{name}\n{name}")),
            (
                json!([{"type":"text","text":"before"}, reference, {"type":"text","text":"after"}]),
                format!("before\n{name}\nafter"),
            ),
        ] {
            let mut records = records.clone();
            records[1]["message"]["content"][0]["content"] = content;
            let events = events_from(records);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].calls.len(), 1);
            assert_eq!(
                events[0].calls[0].output,
                Some(vec![text_section("output", &expected)])
            );
        }
    }

    #[test]
    fn tool_call_fallbacks_keep_reasons() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let bash = &records[0]["message"]["content"][0];
        let mut result = ResultAt {
            source_records: Vec::new(),
            at: at(1),
            sequence: 1,
            error: false,
            denial: None,
            content: "reason".to_owned(),
            raw_content: Value::Null,
            value: Value::Null,
        };
        for value in [Value::Null, json!("metadata"), json!(3)] {
            result.value = value;
            assert_eq!(
                tool_call("Bash", &bash["input"], Some(&result)).output,
                Some(vec![text_section("output", "reason")])
            );
        }
        result.value = json!({"stdout":"", "stderr":""});
        assert_eq!(
            tool_call("Bash", &bash["input"], Some(&result)).output,
            Some(vec![text_section("stdout", "(no output)")])
        );
        result.error = true;
        assert_eq!(
            tool_call("Bash", &bash["input"], Some(&result)).output,
            Some(vec![text_section("output", "reason")])
        );
        result.error = false;
        result.value = Value::Null;
        assert!(
            matches!(&tool_call("Agent", &records[10]["message"]["content"][0]["input"], Some(&result)).output.unwrap()[0], Section::Markdown { text, .. } if text == "reason")
        );
        result.value = Value::Null;
        let edit = tool_call(
            "Edit",
            &records[4]["message"]["content"][0]["input"],
            Some(&result),
        );
        assert!(
            matches!(&edit.input[0], Section::Diff { text, .. } if text.contains("\n+") && text.starts_with('-'))
        );
        assert_eq!(
            tool_call(
                "Read",
                &records[2]["message"]["content"][0]["input"],
                Some(&result)
            )
            .output,
            Some(vec![text_section("output", "reason")])
        );
        assert_eq!(
            result_text(
                &json!([{"type":"image"},{"type":"text","text":"one"},{"type":"text","text":"two"}])
            ),
            "one\ntwo"
        );
        assert!(
            tool_call("Read", &records[2]["message"]["content"][0]["input"], None)
                .output
                .is_none()
        );
        let mut record = records[0].clone();
        record["wireToolInputs"] = json!({"command":"wrong"});
        let events = events_from(vec![record]);
        assert_eq!(
            events[0].calls[0].input[0],
            Section::Shell {
                prefix: 0,
                text: bash["input"]["command"].as_str().unwrap().to_owned()
            }
        );
        assert_eq!(
            all_fields(&json!({"flag":true,"nested":{"n":1}})),
            Section::Fields(vec![
                ("flag".to_owned(), "true".to_owned()),
                ("nested".to_owned(), "{\"n\":1}".to_owned())
            ])
        );
    }

    #[test]
    fn ask_user_question_texts_in_log_and_latest_activity() {
        for fixture in [
            include_str!("../../../tests/fixtures/claude-questions-pending.jsonl"),
            include_str!("../../../tests/fixtures/claude-questions-answered.jsonl"),
        ] {
            let record: serde_json::Value =
                serde_json::from_str(fixture.lines().next().unwrap()).unwrap();
            let expected = record["message"]["content"][0]["input"]["questions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|question| question["question"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" · ");
            let mut file = tempfile::NamedTempFile::new().unwrap();
            write!(file, "{fixture}").unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].body, "AskUserQuestion · 子命令 · cd");
            assert_eq!(events[0].kind, Kind::Assistant);
            let len = file.as_file().metadata().unwrap().len();
            let summary = super::super::transcript::read(file.path(), len).unwrap();
            let activity = summary.activity.unwrap();
            assert_eq!(activity.label, expected);
            assert_eq!(activity.kind, Kind::Assistant);
        }
    }

    #[test]
    fn real_tool_details_match_log_and_latest_activity() {
        let expected = [
            "mkdir, cd, ls, wc, head",
            "找那个会话",
            "To (hide-idle-teammates) 问 line_truncation.rs 是不是它在改",
            "To (team-lead) Report: /tmp/research",
            "SubagentHandback Report: /tmp/research",
            "WebFetch code.claude.com/docs/en/hooks",
            "cd, ls, sed, wc",
            "cd, cat, wc, sed",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
            .lines()
            .zip(expected)
        {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{line}").unwrap();
            let events = tail_session(file.path(), 8, true).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].body, expected);
            let len = file.as_file().metadata().unwrap().len();
            let summary = super::super::transcript::read_subagent(file.path(), len).unwrap();
            assert_eq!(summary.activity.unwrap().label, expected);
        }
    }

    #[test]
    fn real_agent_names_in_log_and_activity() {
        let expected = [
            "reviewer · Review files",
            "SubagentHandback Checks passed.",
            "(worker-id) Checks passed.",
            "(team-lead) Review files.",
            "(peer-session) Checks passed.",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
            .lines()
            .zip(expected)
        {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), line).unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(
                participant_text(events[0].sender.as_deref(), &events[0].body),
                expected
            );
            let record: Value = serde_json::from_str(line).unwrap();
            if record["type"] == "assistant" {
                let len = file.as_file().metadata().unwrap().len();
                let summary = super::super::transcript::read_subagent(file.path(), len).unwrap();
                assert_eq!(summary.activity.unwrap().label, expected);
            }
        }
    }

    #[test]
    fn task_names_and_named_handbacks_share_log_and_activity_format() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for (mut record, tool_name, expected) in [
            (records[0].clone(), "Task", "Task (reviewer) Review files"),
            (
                records[1].clone(),
                "SubagentHandback",
                "SubagentHandback (reviewer) Checks passed.",
            ),
        ] {
            let block = &mut record["message"]["content"][0];
            block["name"] = tool_name.into();
            block["input"]["name"] = "reviewer".into();
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), record.to_string()).unwrap();
            assert_eq!(tail(file.path(), 1).unwrap()[0].body, expected);
            let summary = super::super::transcript::read_subagent(
                file.path(),
                file.as_file().metadata().unwrap().len(),
            )
            .unwrap();
            assert_eq!(summary.activity.unwrap().label, expected);
        }
    }

    #[test]
    fn agent_names_without_details_or_names_keep_clean_spacing() {
        let record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        for (name, label) in [
            ("Agent", "Agent"),
            ("Task", "Task"),
            ("SendMessage", "To"),
            ("SubagentHandback", "SubagentHandback"),
        ] {
            for (participant, expected) in [
                (
                    "reviewer",
                    if name == "Agent" {
                        "reviewer".to_owned()
                    } else {
                        format!("{label} (reviewer)")
                    },
                ),
                (" ", label.to_owned()),
            ] {
                let mut record = record.clone();
                let block = &mut record["message"]["content"][0];
                block["name"] = name.into();
                block["input"] =
                    json!({if name == "SendMessage" { "to" } else { "name" }: participant});
                let file = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(file.path(), record.to_string()).unwrap();
                assert_eq!(tail(file.path(), 1).unwrap()[0].body, expected);
                let summary = super::super::transcript::read_subagent(
                    file.path(),
                    file.as_file().metadata().unwrap().len(),
                )
                .unwrap();
                assert_eq!(summary.activity.unwrap().label, expected);
            }
        }
    }

    #[test]
    fn bash_blank_description_uses_command_in_both_surfaces() {
        let mut record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
                .lines()
                .nth(6)
                .unwrap(),
        )
        .unwrap();
        record["message"]["content"][0]["input"]["description"] = "  ".into();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        let events = tail_session(file.path(), 8, true).unwrap();
        assert_eq!(events[0].body, "cd, ls, sed, wc");
        let summary = super::super::transcript::read_subagent(
            file.path(),
            file.as_file().metadata().unwrap().len(),
        )
        .unwrap();
        assert_eq!(summary.activity.unwrap().label, "cd, ls, sed, wc");
    }

    #[test]
    fn subagent_transcript_reads_sidechain_records_only_in_subagent_mode() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!("../../../tests/fixtures/subagent.jsonl"))
            .unwrap();
        assert!(tail(file.path(), 8).unwrap().is_empty());
        let events = tail_session(file.path(), 8, true).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Tool);
        assert!(events[0].body.starts_with("SubagentHandback"));
        assert!(events[0].ended_at.is_some());
    }

    fn at(second: u32) -> DateTime<Utc> {
        format!("2026-10-06T16:39:{second:02}Z").parse().unwrap()
    }
    fn record(kind: &str, second: u32, content: Value) -> Value {
        json!({"type":kind, "timestamp":at(second).to_rfc3339(),
            "uuid":format!("record-{second}"), "message":{"content":content}})
    }
    fn tool(second: u32, message: &str, id: &str, name: &str, input: Value) -> Value {
        let mut r = record(
            "assistant",
            second,
            json!([{"type":"tool_use", "id":id, "name":name, "input":input}]),
        );
        r["message"]["id"] = message.into();
        r
    }
    fn result(second: u32, id: &str, error: bool) -> Value {
        record(
            "user",
            second,
            json!([{"type":"tool_result", "tool_use_id":id, "is_error":error, "content":"done"}]),
        )
    }
    fn model_turn() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (time, mut r) in [
            ("10:00:00", record("user", 0, json!("inspect"))),
            ("10:00:02", tool(2, "first", "a", "Read", json!({}))),
            ("10:00:05", result(5, "a", false)),
            (
                "10:00:11",
                record("assistant", 11, json!([{"type":"text", "text":"done"}])),
            ),
            (
                "10:01:00",
                record(
                    "user",
                    0,
                    json!("<task-notification>finished</task-notification>"),
                ),
            ),
            (
                "10:01:57",
                record("assistant", 57, json!([{"type":"text", "text":"checked"}])),
            ),
        ] {
            r["timestamp"] = format!("2026-10-06T{time}Z").into();
            writeln!(file, "{r}").unwrap();
        }
        file
    }

    fn denial_records(fixture: &str) -> Vec<Value> {
        fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn events_from(records: Vec<Value>) -> Vec<Event> {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        tail(file.path(), 20).unwrap()
    }

    #[test]
    fn automode_blocked_is_denied_with_classifier_reason() {
        let records = denial_records(include_str!(
            "../../../tests/fixtures/automode_blocked.jsonl"
        ));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let events = events_from(records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Bash: Create Unsafe Agents");
        assert_eq!(events[0].ended_at, Some(end));
    }

    #[test]
    fn automode_blocked_without_brackets_is_denied_as_dangerous() {
        let records = denial_records(include_str!(
            "../../../tests/fixtures/automode_blocked_dangerous.jsonl"
        ));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let events = events_from(records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Edit: Dangerous");
        assert_eq!(events[0].ended_at, Some(end));
    }

    #[test]
    fn safety_check_is_denied_and_ends_before_the_next_reply() {
        let mut records =
            denial_records(include_str!("../../../tests/fixtures/safety_check.jsonl"));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let mut reply = record("assistant", 1, json!([{"type":"text", "text":"checked"}]));
        reply["timestamp"] = "2026-10-12T15:13:30Z".into();
        records.push(reply);
        let events = events_from(records);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Bash: Safety Check");
        assert_eq!(events[0].ended_at, Some(end));
        assert_eq!(events[1].previous_end, Some(end));
    }

    #[test]
    fn automode_unavailable_and_other_errors_stay_errors() {
        for (denial, reason) in [
            ("automode-unavailable", "classifier"),
            ("permission-rule", "user"),
            ("hook", "hook"),
        ] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[1]["toolDenialKind"] = denial.into();
            records[1]["permissionDecision"]["reasonType"] = reason.into();
            let events = events_from(records);
            assert_eq!(events[0].kind, Kind::Error);
            assert_eq!(events[0].body, "echo");
        }
    }

    #[test]
    fn automode_reason_without_complete_brackets_uses_dangerous() {
        for text in ["denied", "Reason: [unfinished"] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[1]["message"]["content"][0]["content"] = text.into();
            let events = events_from(records);
            assert_eq!(events[0].kind, Kind::Denied);
            assert_eq!(events[0].body, "Bash: Dangerous");
        }
    }

    #[test]
    fn denied_parallel_batch_takes_precedence_over_errors_in_either_order() {
        for first in [true, false] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            let mut other = records[0].clone();
            other["message"]["content"][0]["id"] = "other".into();
            other["message"]["content"][0]["name"] = "Read".into();
            let mut result = records[1].clone();
            result.as_object_mut().unwrap().remove("toolDenialKind");
            result.as_object_mut().unwrap().remove("permissionDecision");
            result["message"]["content"][0]["tool_use_id"] = "other".into();
            records.insert(if first { 1 } else { 0 }, other);
            records.push(result);
            let events = events_from(records);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, Kind::Denied);
            assert!(events[0].body.contains("Bash: Create Unsafe Agents"));
            assert!(events[0].body.contains("Read"));
        }
    }

    #[test]
    fn model_reply_duration_starts_after_tools_and_hidden_turn_starters() {
        let file = model_turn();
        let events = tail(file.path(), 20).unwrap();
        let models: Vec<_> = events
            .iter()
            .filter(|event| event.kind == Kind::Assistant)
            .collect();
        let tool = events
            .iter()
            .find(|event| event.kind == Kind::Tool)
            .unwrap();
        assert_eq!(
            (tool.ended_at.unwrap() - tool.previous_end.unwrap()).num_seconds(),
            5
        );
        assert_eq!(models.len(), 2);
        for (event, seconds) in models.into_iter().zip([6, 57]) {
            assert_eq!(
                (event.started_at - event.previous_end.unwrap()).num_seconds(),
                seconds
            );
        }
    }

    #[test]
    fn model_reply_tail_finds_tool_end_across_chunks() {
        let mut file = model_turn();
        let before = tail(file.path(), 1).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].kind, Kind::Assistant);
        assert_eq!(
            (before[0].started_at - before[0].previous_end.unwrap()).num_seconds(),
            57
        );
        // The preceding tool batch crosses a transcript chunk boundary.
        writeln!(
            file,
            "{}",
            tool(
                58,
                "large",
                "b",
                "Read",
                json!({"prompt":"x".repeat(140_000)})
            )
        )
        .unwrap();
        writeln!(file, "{}", result(59, "b", false)).unwrap();
        writeln!(
            file,
            "{}",
            record("assistant", 59, json!([{"type":"text", "text":"again"}]))
        )
        .unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].previous_end, Some(at(59)));
    }

    #[test]
    fn consecutive_model_replies_reference_the_previous_text() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for r in [
            record("user", 0, json!("inspect")),
            record("assistant", 11, json!([{"type":"text", "text":"first"}])),
            record("assistant", 20, json!([{"type":"text", "text":"second"}])),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events[0].previous_end, Some(at(11)));
        assert_eq!(
            (events[0].started_at - events[0].previous_end.unwrap()).num_seconds(),
            9
        );
    }

    #[test]
    fn model_reply_without_reference_keeps_none() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            record("assistant", 11, json!([{"type":"text", "text":"first"}]))
        )
        .unwrap();
        assert_eq!(tail(file.path(), 1).unwrap()[0].previous_end, None);
    }

    #[test]
    fn model_reply_directly_after_user_takes_the_gap() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (time, mut r) in [
            ("10:01:00", record("user", 0, json!("inspect"))),
            (
                "10:01:57",
                record("assistant", 57, json!([{"type":"text", "text":"done"}])),
            ),
        ] {
            r["timestamp"] = format!("2026-10-06T{time}Z").into();
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(
            (events[0].started_at - events[0].previous_end.unwrap()).num_seconds(),
            57
        );
    }

    #[test]
    fn model_reply_uses_tool_end_after_any_user_content() {
        for content in [
            json!("typed prompt"),
            json!("<task-notification>finished</task-notification>"),
            json!("<agent-message>finished</agent-message>"),
            json!("<cross-session-message from-name=\"peer\">finished</cross-session-message>"),
            json!("<pasted_content>finished</pasted_content>"),
            json!([{"type":"image", "source":{"type":"base64", "data":""}}]),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            for r in [
                record("user", 0, content),
                tool(2, "between", "a", "Read", json!({})),
                result(5, "a", false),
                record("assistant", 11, json!([{"type":"text", "text":"done"}])),
            ] {
                writeln!(file, "{r}").unwrap();
            }
            assert_eq!(tail(file.path(), 1).unwrap()[0].previous_end, Some(at(5)));
        }
    }
    #[test]
    fn detail_sources_identify_real_transcript_events() {
        let file = fixture();
        let summary =
            super::super::transcript::read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap();
        let events = tail(file.path(), usize::MAX).unwrap();
        for (index, kind) in [(1, Kind::Assistant), (2, Kind::User)] {
            let source = summary.detail_sources[index].as_ref().unwrap();
            assert!(
                events
                    .iter()
                    .any(|event| event.source.as_ref() == Some(source) && event.kind == kind)
            );
        }
        let source = summary.detail_sources[0].as_ref().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.calls.iter().any(|call| &call.id == source))
        );
    }

    fn fixture() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Reuse the real prompt and assistant response before the tool records.
        for line in include_str!("../../../tests/fixtures/transcript.jsonl")
            .lines()
            .take(2)
        {
            writeln!(file, "{line}").unwrap();
        }
        let mut sidechain = record("user", 43, json!("sidechain"));
        sidechain["isSidechain"] = true.into();
        let mut meta = record("user", 43, json!("meta"));
        meta["isMeta"] = true.into();
        for r in [
            tool(
                30,
                "batch",
                "a",
                "Read",
                json!({"description":"", "file_path":"src/main.rs"}),
            ),
            result(31, "a", false),
            tool(32, "batch", "b", "Grep", json!({"query":"needle"})),
            result(35, "b", true),
            tool(
                36,
                "background",
                "c",
                "Bash",
                json!({"description":"build", "run_in_background":true}),
            ),
            result(38, "c", false),
            json!({"type":"queue-operation", "timestamp":at(40).to_rfc3339(), "operation":"enqueue", "content":"next task"}),
            json!({"type":"queue-operation", "timestamp":at(42).to_rfc3339(), "operation":"enqueue", "content":"<agent-message>finished</agent-message>"}),
            json!({"type":"queue-operation", "timestamp":at(43).to_rfc3339(), "operation":"enqueue", "content":"<task-notification>\n<task-id>bcfsuvg3w</task-id>\n<status>completed</status>\n<summary>  Background command \"等 Codex 做日志面板\" completed (exit code 0) \n</summary>\n</task-notification>"}),
            record(
                "user",
                41,
                json!("<task-notification>finished</task-notification>"),
            ),
            sidechain,
            meta,
            json!({"type":"attachment", "attachment":{"type":"noise"}}),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        file
    }
    pub(crate) fn wrapped_fixture() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (second, content) in [
            (
                1,
                "<pasted_content id=\"paste-1\">\n  first line\n  second line\n</pasted_content id=\"paste-1\">",
            ),
            (
                3,
                "<agent-message from=\"a113c3cb9c1482c7a\">\n[Subagent hand-back] The text below is the final report of a subagent. The report follows:\n  Report: /private/tmp/claude-501/mit-dl/agent6.md\n  - Source: css.csail.mit.edu/6.5660/2026/\n</agent-message>",
            ),
            (
                5,
                "<cross-session-message from=\"uds:/tmp/cc-socks/71065.sock\" from-name=\"translate-pdf\" from-mode=\"prompting\">\ntranslate-pdf pilots ready for review.\n</cross-session-message>",
            ),
        ] {
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(second).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            writeln!(file, "{}", record("user", second + 1, json!(content))).unwrap();
        }
        // Real cross-session body, also exercised through the other peer envelopes.
        for (second, opening, closing) in [
            (8, "agent-message from=\"peer\"", "agent-message"),
            (
                10,
                "teammate-message teammate_id=\"peer\"",
                "teammate-message",
            ),
            (
                12,
                "cross-session-message from-name=\"peer\"",
                "cross-session-message",
            ),
        ] {
            let body = include_str!("../../../tests/fixtures/claude-peer-message.txt");
            let content = format!("<{opening}>\n  {body}  \n</{closing}>");
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(second).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            writeln!(file, "{}", record("user", second + 1, json!(content))).unwrap();
        }
        for content in [
            "<task-notification>finished</task-notification>",
            "  <system-reminder>hidden</system-reminder>",
            "<unknown>hidden</unknown>",
        ] {
            writeln!(file, "{}", record("user", 7, json!(content))).unwrap();
        }
        file
    }

    #[test]
    fn wrapped_messages_extract_bodies_and_keep_queue_origin() {
        let file = wrapped_fixture();
        let events = tail(file.path(), 20).unwrap();
        assert_eq!(events.len(), 12);
        let peer_body = include_str!("../../../tests/fixtures/claude-peer-message.txt").trim();
        assert_eq!(peer_body.matches('\n').count(), 11);
        for (pair, kind, sender, body) in [
            (&events[0..2], Kind::User, None, "first line second line"),
            (
                &events[2..4],
                Kind::Agent,
                Some("a113c3cb9c1482c7a"),
                "Report: /private/tmp/claude-501/mit-dl/agent6.md\n  - Source: css.csail.mit.edu/6.5660/2026/",
            ),
            (
                &events[4..6],
                Kind::Session,
                Some("translate-pdf"),
                "translate-pdf pilots ready for review.",
            ),
            (&events[6..8], Kind::Agent, Some("peer"), peer_body),
            (&events[8..10], Kind::Agent, Some("peer"), peer_body),
            (&events[10..12], Kind::Session, Some("peer"), peer_body),
        ] {
            assert_eq!(pair[0].kind, Kind::QueuedSystem);
            assert_eq!(pair[1].kind, kind);
            for event in pair {
                assert_eq!(event.sender.as_deref(), sender);
                assert_eq!(event.body, body);
                assert_eq!(event.ended_at, None);
            }
        }
    }

    #[test]
    fn fenced_pasted_user_text_keeps_the_following_comment() {
        let content = "```\n\n\n<pasted_content id=\"7771\">\n  17:58:53 User    实话实说吧啊，…\n        6s Tool    mcp__plugin_codex_codex-manager__send …\n</pasted_content id=\"7771\">\n\n```\n\n那个五点五十八五十三秒和五点五十九零四秒。 哎，这个 model 的这个回复是…";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!(content))).unwrap();
        writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
        let events = tail(file.path(), 2).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, Kind::User);
        assert_eq!(events[1].kind, Kind::QueuedUser);
        for event in events {
            assert!(event.body.starts_with("17:58:53 User 实话实说吧啊"));
            assert!(event.body.contains("那个五点五十八五十三秒"));
            assert!(!event.body.contains("```"));
            assert!(!event.body.contains("<pasted_content"));
        }
    }

    #[test]
    fn pasted_blocks_anywhere_preserve_surrounding_user_and_queue_text() {
        for content in [
            "before\n```text\n<pasted_content id=\"a\">\n  first\n  second\n</pasted_content id=\"a\">\n```\nbetween <pasted_content id=\"b\">  third</pasted_content id=\"b\"> after",
            "<pasted_content id=\"a\">\n  before first second\n</pasted_content id=\"a\"> between <pasted_content id=\"b\">  third</pasted_content id=\"b\"> after",
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", record("user", 1, json!(content))).unwrap();
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 2).unwrap();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].kind, Kind::User);
            for event in events {
                assert_eq!(event.body, "before first second between third after");
            }
        }
    }

    #[test]
    fn agent_report_without_harness_keeps_the_whole_inner_text() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!([{"type":"text", "text":"<agent-message from=\"agent\">\n  Report without harness\n  The report follows:\n  details\n</agent-message>"}]))).unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Agent);
        assert_eq!(
            events[0].body,
            "Report without harness\n  The report follows:\n  details"
        );
    }

    #[test]
    fn task_list_tools_produce_no_events() {
        for name in [
            "TaskCreate",
            "TaskUpdate",
            "TaskList",
            "TaskGet",
            "ListAgents",
        ] {
            assert!(events_from(vec![tool(1, "call", "task", name, json!({}))]).is_empty());
        }
    }

    #[test]
    fn task_list_tools_preserve_the_bash_batch() {
        let bash = tool(
            1,
            "batch",
            "bash",
            "Bash",
            json!({"description":"inspect files"}),
        );
        let mut expected = events_from(vec![bash.clone(), result(3, "bash", false)]);
        for event in &mut expected {
            event.source_records.clear();
        }
        assert_eq!(expected.len(), 1);
        for task_first in [false, true] {
            let task = tool(1, "batch", "task", "TaskCreate", json!({}));
            let mut mixed = bash.clone();
            let blocks = mixed["message"]["content"].as_array_mut().unwrap();
            blocks.insert(
                usize::from(!task_first),
                task["message"]["content"][0].clone(),
            );
            let mut actual = events_from(vec![mixed, result(3, "bash", false)]);
            for event in &mut actual {
                event.source_records.clear();
            }
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn task_list_tool_results_cannot_make_error_or_denied_rows() {
        for name in [
            "TaskCreate",
            "TaskUpdate",
            "TaskList",
            "TaskGet",
            "ListAgents",
        ] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[0]["message"]["content"][0]["name"] = name.into();
            assert!(events_from(records).is_empty());
            assert!(
                events_from(vec![
                    tool(1, "call", "task", name, json!({})),
                    result(2, "task", true)
                ])
                .is_empty()
            );
        }
    }

    #[test]
    fn task_stop_produces_a_tool_row() {
        let events = events_from(vec![tool(1, "call", "task", "TaskStop", json!({}))]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Tool);
        assert_eq!(events[0].body, "TaskStop");
    }

    #[test]
    fn file_path_is_stored_separately_from_the_tool_name() {
        for name in ["Read", "Edit", "Write"] {
            let events = events_from(vec![tool(
                1,
                "call",
                "file",
                name,
                json!({"file_path":"~/Users/project/src/main.rs"}),
            )]);
            assert_eq!(
                events[0].body,
                format!("{name} ~/Users/project/src/main.rs")
            );
            assert_eq!(
                &events[0].body[events[0].paths[0].clone()],
                "~/Users/project/src/main.rs"
            );
        }
    }

    #[test]
    fn bash_description_replaces_only_the_exact_tool_name() {
        for (name, input, expected) in [
            (
                "Bash",
                json!({"description":"看 dot 仓库和引用位置"}),
                "看 dot 仓库和引用位置",
            ),
            ("Bash", json!({}), "Bash"),
            ("Bash", json!({"description":" \n "}), "Bash"),
            (
                "Read",
                json!({"description":"inspect files"}),
                "Read inspect files",
            ),
            (
                "bash",
                json!({"description":"inspect files"}),
                "bash inspect files",
            ),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", tool(1, "call", "a", name, input)).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events[0].kind, Kind::Tool);
            assert_eq!(events[0].body, expected);
        }
    }

    #[test]
    fn parallel_calls_apply_bash_description_per_call() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for r in [
            tool(
                1,
                "batch",
                "a",
                "Bash",
                json!({"description":"inspect files"}),
            ),
            tool(2, "batch", "b", "Bash", json!({})),
            tool(
                3,
                "batch",
                "c",
                "Read",
                json!({"description":"read config"}),
            ),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, "inspect files，Bash，Read read config");
    }

    #[test]
    fn parallel_calls_without_details_have_no_separator_after_names() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for r in [
            tool(
                1,
                "batch",
                "a",
                "mcp__plugin_codex_codex-manager__detach",
                json!({}),
            ),
            tool(2, "batch", "b", "TaskStop", json!({})),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, "codex-manager detach，TaskStop");
    }

    #[test]
    fn groups_interleaved_parallel_calls_and_tracks_predecessors() {
        let file = fixture();
        let events = tail(file.path(), 8).unwrap();
        assert_eq!(events.len(), 7);
        assert_eq!(
            events.iter().map(|e| &e.kind).collect::<Vec<_>>(),
            [
                &Kind::User,
                &Kind::Assistant,
                &Kind::Error,
                &Kind::Tool,
                &Kind::QueuedUser,
                &Kind::QueuedSystem,
                &Kind::Tool
            ]
        );
        assert!(events[0].body.contains("Stop hook error"));
        assert!(events[1].body.starts_with("对，必须"));
        assert_eq!(
            events[0].started_at,
            "2026-10-06T16:39:12.595Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            events[1].started_at,
            "2026-10-06T16:39:29.554Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(events[2].body, "Read src/main.rs，Grep needle");
        assert_eq!(events[2].started_at, at(30));
        assert_eq!(events[2].ended_at, Some(at(35)));
        assert_eq!(events[3].body, "build *");
        assert_eq!(events[3].started_at, at(36));
        assert_eq!(events[3].ended_at, Some(at(38)));
        assert_eq!(events[4].body, "next task");
        assert_eq!(events[4].started_at, at(40));
        assert_eq!(events[5].body, "finished");
        assert_eq!(events[5].started_at, at(42));
        assert_eq!(
            events[6].body,
            "Background command \"等 Codex 做日志面板\" completed (exit code 0)"
        );
        assert_eq!(events[6].started_at, at(43));
        assert_eq!(events[6].ended_at, Some(at(43)));
        assert_eq!(events[0].previous_end, None);
        for pair in events.windows(2) {
            let end = if matches!(pair[0].kind, Kind::Tool | Kind::Error) {
                pair[0].ended_at
            } else {
                Some(pair[0].started_at)
            };
            assert_eq!(pair[1].previous_end, end);
        }
        assert_eq!(tail(file.path(), 2).unwrap(), events[5..]);
        assert!(tail(file.path(), 0).unwrap().is_empty());
    }
    #[test]
    fn missing_result_keeps_the_entire_batch_running_and_request_id_groups() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for mut r in [
            tool(1, "unused", "a", "Read", json!({"file_path":"a"})),
            result(2, "a", true),
            tool(3, "unused", "b", "Read", json!({"file_path":"b"})),
        ] {
            r["message"].as_object_mut().unwrap().remove("id");
            r["requestId"] = "request".into();
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Error);
        assert_eq!(events[0].body, "Read a b");
        assert_eq!(events[0].paths, [5..6, 7..8]);
        assert_eq!(events[0].ended_at, None);
        assert_eq!(events[0].started_at, at(1));
    }
    #[test]
    fn chunk_boundary_keeps_the_whole_record_and_parallel_group() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!("before"))).unwrap();
        writeln!(
            file,
            "{}",
            tool(
                2,
                "large",
                "a",
                "Read",
                json!({"prompt":"x".repeat(70_000)})
            )
        )
        .unwrap();
        writeln!(file, "{}", result(3, "a", false)).unwrap();
        writeln!(
            file,
            "{}",
            tool(4, "large", "b", "Read", json!({"file_path":"b"}))
        )
        .unwrap();
        writeln!(file, "{}", result(5, "b", false)).unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].started_at, at(2));
        assert_eq!(events[0].previous_end, Some(at(1)));
        assert_eq!(events[0].ended_at, Some(at(5)));
        assert_eq!(
            events[0].body,
            format!("Read {}，Read b", "x".repeat(70_000))
        );
    }
    #[test]
    fn stops_in_the_tail_of_a_large_file() {
        let base = fixture();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&vec![b'x'; 200_000]).unwrap();
        writeln!(file).unwrap();
        file.write_all(&std::fs::read(base.path()).unwrap())
            .unwrap();
        let mut actual = tail(file.path(), 2).unwrap();
        let mut expected = tail(base.path(), 2).unwrap();
        for (actual, expected) in actual.iter_mut().zip(&mut expected) {
            assert_eq!(actual.raw_text().unwrap(), expected.raw_text().unwrap());
            actual.source_records.clear();
            expected.source_records.clear();
        }
        assert_eq!(actual, expected);
    }
    #[test]
    fn shared_walk_reads_only_the_last_chunk_when_events_are_complete() {
        use std::io::{Read, Seek, SeekFrom};
        struct MeasuredFile {
            file: std::fs::File,
            bytes_read: usize,
            earliest: u64,
        }
        impl Read for MeasuredFile {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.earliest = self.earliest.min(self.file.stream_position()?);
                let read = self.file.read(buffer)?;
                self.bytes_read += read;
                Ok(read)
            }
        }
        impl Seek for MeasuredFile {
            fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
                self.file.seek(from)
            }
        }
        let base = fixture();
        let mut fixture = tempfile::NamedTempFile::new().unwrap();
        fixture.write_all(&vec![b'x'; 200_000]).unwrap();
        writeln!(fixture).unwrap();
        fixture
            .write_all(&std::fs::read(base.path()).unwrap())
            .unwrap();
        let file = std::fs::File::open(fixture.path()).unwrap();
        let len = file.metadata().unwrap().len();
        let mut measured = MeasuredFile {
            file,
            bytes_read: 0,
            earliest: len,
        };
        let mut log = Log::default();
        super::super::transcript::walk_records(&mut measured, len, Vec::new(), false, |record| {
            log.record(record, 2)
        })
        .unwrap();
        assert_eq!(measured.bytes_read, 64 * 1024);
        assert_eq!(measured.earliest, len - 64 * 1024);
        assert_eq!(
            log.entries[0].event.body,
            "Background command \"等 Codex 做日志面板\" completed (exit code 0)"
        );
    }

    #[test]
    fn queued_task_notifications_end_at_the_record_time_for_every_status() {
        for status in ["completed", "failed", "killed", "stopped"] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let content = format!(
                "<task-notification>\n<status>{status}</status>\n<summary> \njob {status}\n </summary>\n</task-notification>"
            );
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, Kind::Tool);
            assert_eq!(events[0].body, format!("job {status}"));
            assert_eq!(events[0].started_at, at(2));
            assert_eq!(events[0].ended_at, Some(at(2)));
        }
    }

    #[test]
    fn queued_task_notifications_without_summary_use_the_first_nonempty_line() {
        for (content, body) in [
            (
                "<task-notification>\n \n  finished  \nmore\n</task-notification>",
                "finished",
            ),
            (
                "<task-notification>finished</task-notification>",
                "finished",
            ),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events[0].kind, Kind::Tool);
            assert_eq!(events[0].body, body);
            assert_eq!(events[0].started_at, at(2));
            assert_eq!(events[0].ended_at, Some(at(2)));
        }
    }

    #[test]
    fn other_queued_tags_keep_their_content_and_kind() {
        for content in [
            "<system-reminder>finished</system-reminder>",
            "<unknown>finished</unknown>",
            "<agent-message-extra>finished</agent-message-extra>",
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events[0].kind, Kind::QueuedSystem);
            assert_eq!(events[0].body, content);
            assert_eq!(events[0].ended_at, None);
        }
    }

    #[test]
    fn text_blocks_keep_order_skip_thinking_and_keep_unterminated_last_record() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            record(
                "user",
                1,
                json!([{"type":"text","text":"first"},{"type":"text","text":"second"}])
            )
        )
        .unwrap();
        write!(file,"{}",record("assistant",2,json!([{"type":"thinking","thinking":"private"},{"type":"text","text":"one"},{"type":"text","text":"two"}]))).unwrap();
        let events = tail(file.path(), 8).unwrap();
        assert_eq!(
            events.iter().map(|e| e.body.as_str()).collect::<Vec<_>>(),
            ["first\nsecond", "one", "two"]
        );
        assert_eq!(events[1].previous_end, Some(at(1)));
        assert_eq!(events[2].previous_end, Some(at(2)));
    }
    #[test]
    fn a_valid_json_suffix_at_the_chunk_edge_is_not_a_complete_record() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!("before"))).unwrap();
        let suffix = format!(
            "{}\n{}\n",
            record("user", 2, json!("ghost")),
            record("user", 3, json!("after"))
        );
        // The initial window starts on valid JSON inside an invalid, longer line.
        file.write_all(&vec![b'x'; 70_000]).unwrap();
        file.write_all(suffix.as_bytes()).unwrap();
        file.write_all(&vec![b'\n'; 64 * 1024 - suffix.len()])
            .unwrap();
        let events = tail(file.path(), 2).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.body.as_str())
                .collect::<Vec<_>>(),
            ["before", "after"]
        );
        assert_eq!(events[1].previous_end, Some(at(1)));
    }
}
