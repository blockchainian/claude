use crate::event::{Event, Kind, Section, ToolCall};
use chrono::{DateTime, Utc};
use std::{
    io::{self, Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[cfg(test)]
use crate::providers::tool::code_section;
use crate::providers::tool::{self, Output, Subject};
pub(super) use crate::providers::tool::{tool_hidden, tool_kind};

/// Reuses the backward event parser, including result lookup and merged tool rows.
#[derive(Default)]
pub(super) struct ActivityEvent {
    event_panel: EventReader,
    ready: bool,
}

impl ActivityEvent {
    pub fn record(&mut self, record: serde_json::Value) {
        if self.ready {
            return;
        }
        self.event_panel.record(record, usize::MAX);
        if let Some(index) = self
            .event_panel
            .entries
            .iter()
            .position(|entry| !entry.event.calls.is_empty())
        {
            let event = &self.event_panel.entries[index].event;
            let message = event
                .calls
                .first()
                .and_then(|call| call.agent_message.as_ref());
            if let Some(message) = message.filter(|message| message.mergeable(message)) {
                let end = self.event_panel.entries[index..]
                    .iter()
                    .take_while(|entry| {
                        entry.event.kind == event.kind
                            && !entry.event.calls.is_empty()
                            && entry.event.calls.iter().all(|call| {
                                call.agent_message
                                    .as_ref()
                                    .is_some_and(|other| message.mergeable(other))
                            })
                    })
                    .count()
                    + index;
                self.ready = end < self.event_panel.entries.len()
                    && self
                        .event_panel
                        .batch
                        .is_none_or(|batch| batch < index || batch >= end);
            } else {
                self.ready = self.event_panel.batch != Some(index);
            }
        }
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn finish(self) -> io::Result<Option<Event>> {
        if let Some(error) = self.event_panel.error {
            return Err(error);
        }
        Ok(crate::event::agent::merge(
            self.event_panel
                .entries
                .into_iter()
                .rev()
                .map(|entry| entry.event)
                .collect(),
        )
        .into_iter()
        .rev()
        .find(|event| !event.calls.is_empty()))
    }
}

/// When an event stops occupying the session: a tool's or answered question's result, otherwise its start.
fn end(event: &Event) -> Option<DateTime<Utc>> {
    match event.kind {
        Kind::Tool | Kind::Error | Kind::Denied => event.ended_at,
        _ => event.ended_at.or(Some(event.started_at)),
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
    let mut records = Vec::new();
    crate::jsonl::read_lines_back(&mut file, len, |offset, line| {
        if let Ok(record) = serde_json::from_slice::<serde_json::Value>(line)
            && (sidechains || record["isSidechain"] != true)
        {
            records.push((offset, record));
        }
        true
    })?;
    let turns = crate::activity::EventTurns::new(
        crate::providers::ProviderId::Claude,
        records
            .iter()
            .rev()
            .map(|(offset, record)| (*offset, record)),
    );
    let mut event_panel = EventReader {
        path: Some(path.to_path_buf()),
        turns: Some(turns),
        delivered: prompt_deliveries(records.iter().rev())
            .into_values()
            .collect(),
        ..EventReader::default()
    };
    for (offset, record) in records {
        event_panel.offset = offset;
        if !event_panel.record(record, count) {
            break;
        }
    }
    if let Some(error) = event_panel.error {
        return Err(error);
    }
    let mut events: Vec<_> = event_panel
        .entries
        .into_iter()
        .map(|entry| entry.event)
        .collect();
    events.reverse();
    let mut previous_end = None;
    for event in &mut events {
        if event.kind != Kind::Assistant || event.previous_end.is_none() {
            event.previous_end = previous_end;
        }
        previous_end = end(event);
    }
    let events = crate::event::agent::merge(events);
    let skip = events.len().saturating_sub(count);
    let preceding = skip.checked_sub(1).and_then(|index| events[index].turn);
    Ok(clip_turn_start(
        events.into_iter().skip(skip).collect(),
        preceding,
    ))
}

/// A returned page cannot draw a turn's start when the preceding event belongs to it.
fn clip_turn_start(mut events: Vec<Event>, preceding: Option<crate::event::Turn>) -> Vec<Event> {
    if let Some(previous) = preceding {
        for event in &mut events {
            if let Some(turn) = &mut event.turn
                && turn.id == previous.id
            {
                turn.started = false;
            }
        }
    }
    events
}

/// Pairs each delivered human prompt with the oldest pending enqueue of identical text.
fn prompt_deliveries<'a>(
    records: impl IntoIterator<Item = &'a (u64, serde_json::Value)>,
) -> std::collections::HashMap<u64, u64> {
    let mut pending: std::collections::HashMap<&str, std::collections::VecDeque<u64>> =
        std::collections::HashMap::new();
    let mut absorbed = std::collections::HashMap::new();
    let mut deliveries = std::collections::HashMap::new();
    for (offset, record) in records {
        if record["type"] == "queue-operation" {
            if let Some(text) = record["content"].as_str() {
                if record["operation"] == "enqueue" {
                    pending.entry(text).or_default().push_back(*offset);
                } else if record["operation"] == "remove"
                    && record["reason"] == "absorbed_mid_turn"
                    && let Some(id) = record["commandUuid"].as_str()
                    && let Some(enqueue) = pending.get_mut(text).and_then(|queue| queue.pop_front())
                {
                    absorbed.insert(id, enqueue);
                }
            }
        } else if super::images::queued_prompt(record).is_some() {
            if let Some(enqueue) = record["attachment"]["source_uuid"]
                .as_str()
                .and_then(|id| absorbed.remove(id))
            {
                deliveries.insert(*offset, enqueue);
            }
        } else if record["type"] == "user"
            && record["isMeta"] != true
            && record["isCompactSummary"] != true
            && let Some(text) = prompt_text(&record["message"]["content"])
            && is_prompt_text(record, &text)
            && !wrapped_message(&text)
                .is_some_and(|(kind, _, _)| matches!(kind, Kind::Agent | Kind::Session))
            && let Some(enqueue) = pending
                .get_mut(text.as_str())
                .and_then(|queue| queue.pop_front())
        {
            deliveries.insert(*offset, enqueue);
        }
    }
    deliveries
}

fn prompt_text(content: &serde_json::Value) -> Option<String> {
    content.as_str().map(str::to_owned).or_else(|| {
        Some(
            content
                .as_array()?
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })
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
        let start = self
            .entries
            .last()
            .filter(|entry| {
                entry
                    .event
                    .calls
                    .first()
                    .and_then(|call| call.agent_message.as_ref())
                    .is_some_and(|message| message.kind == "MESSAGE")
            })
            .map_or(start, |entry| start.min(entry.offset));
        let retained = self.entries.partition_point(|entry| entry.offset < start);
        let turns = crate::activity::EventTurns::new(
            crate::providers::ProviderId::Claude,
            self.records
                .iter()
                .map(|(offset, record)| (*offset, record)),
        );
        let changed = self
            .entries
            .iter()
            .position(|entry| entry.event.turn != turns.at(entry.offset));
        let replaced = self.entries.len() - changed.map_or(retained, |index| index.min(retained));
        self.entries.truncate(retained);
        for entry in &mut self.entries {
            entry.event.turn = turns.at(entry.offset);
        }
        let mut event_panel = EventReader {
            path: Some(self.path.clone()),
            turns: Some(turns),
            delivered: prompt_deliveries(self.records.iter())
                .into_values()
                .collect(),
            ..EventReader::default()
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
            event_panel.offset = *offset;
            event_panel.record(record.clone(), usize::MAX);
        }
        if let Some(error) = event_panel.error {
            return Err(error);
        }
        self.entries.extend(event_panel.entries.into_iter().rev());
        let mut previous_end = None;
        for entry in &mut self.entries {
            let event = &mut entry.event;
            if event.kind != Kind::Assistant || event.previous_end.is_none() {
                event.previous_end = previous_end;
            }
            previous_end = end(event);
        }
        Ok(replaced)
    }

    fn affected_start(&self, appended: &[(u64, serde_json::Value)]) -> u64 {
        let mut group = None;
        let mut key = None;
        let mut requests = std::collections::HashMap::new();
        let mut prompts = std::collections::HashMap::new();
        let deliveries = prompt_deliveries(self.records.iter().chain(appended.iter()));
        for (offset, record) in &self.records {
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
        for (offset, record) in appended {
            if let Some(enqueue) = deliveries.get(offset) {
                start = start.min(*enqueue);
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

    pub fn refresh(&mut self, requested: usize) -> io::Result<crate::event::Update> {
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
        let has_older = self.oldest_offset > 0 || self.shown < self.entries.len();
        if !reset && prepended == 0 && appended == 0 && replaced == 0 {
            return Ok(crate::event::Update {
                parsed_bytes,
                has_older,
                oldest_offset: self.oldest_offset,
                read_offset: self.read_offset,
                ..Default::default()
            });
        }
        Ok(crate::event::Update {
            events: clip_turn_start(
                crate::event::agent::merge(
                    self.entries
                        .iter()
                        .skip(self.entries.len() - self.shown)
                        .map(|entry| entry.event.clone())
                        .collect(),
                ),
                self.entries
                    .len()
                    .checked_sub(self.shown + 1)
                    .and_then(|index| self.entries[index].event.turn),
            ),
            prepended: if reset { 0 } else { prepended },
            appended: if reset { self.shown } else { appended },
            replaced: replaced.min(shown_before),
            reset,
            parsed_bytes,
            has_older,
            oldest_offset: self.oldest_offset,
            read_offset: self.read_offset,
        })
    }
}

impl crate::event::Reader for Reader {
    fn refresh(&mut self, requested: usize) -> io::Result<crate::event::Update> {
        Reader::refresh(self, requested)
    }
}

#[derive(Default)]
struct EventReader {
    turns: Option<crate::activity::EventTurns>,
    turn: Option<crate::event::Turn>,
    path: Option<PathBuf>,
    offset: u64,
    companions: std::collections::HashMap<String, Vec<crate::event::SourceRecord>>,
    images: super::images::PromptImages,
    delivered: std::collections::HashSet<u64>,
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
    last_result: Option<usize>,
    labels: Vec<(String, Option<String>)>,
}
struct ResultAt {
    source_records: Vec<crate::event::SourceRecord>,
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

/// Unwraps the `<tool_use_error>` tag Claude Code puts around its own tool errors.
fn result_content(value: &serde_json::Value) -> serde_json::Value {
    value
        .as_str()
        .and_then(|text| {
            text.strip_prefix("<tool_use_error>")?
                .strip_suffix("</tool_use_error>")
        })
        .map_or_else(|| value.clone(), Into::into)
}

fn text_section(label: &str, text: &str) -> Section {
    Section::Text {
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

fn tool_call(name: &str, input: &serde_json::Value, result: Option<&ResultAt>) -> ToolCall {
    let (_, mut subject, sections) = crate::providers::tool::presentation(name, input);
    let mode = tool::rule(name).output;
    if matches!(mode, Some(Output::Read))
        && let Some(result) = result
    {
        subject = result.value["file"]["filePath"].as_str().map(str::to_owned);
        if (input.get("offset").is_some() || input.get("limit").is_some())
            && let (Some(path), Some(start), Some(count)) = (
                subject.as_mut(),
                result.value["file"]["startLine"].as_u64(),
                result.value["file"]["numLines"].as_u64(),
            )
        {
            path.push_str(&format!(
                ":{start}-{}",
                start.saturating_add(count.saturating_sub(1))
            ));
        }
    }
    let state = result
        .map(|result| tool::result_state(&result.content, &serde_json::Value::Null, result.error));
    let exit_code = state.as_ref().and_then(|state| state.exit_code);
    let output = result.map(|result| {
        tool::output(
            mode,
            input,
            &tool::ToolResult {
                content: &result.raw_content,
                value: &result.value,
                text: &result.content,
                error: result.error,
            },
        )
    });
    subject = crate::providers::tool::result_subject(name, output.as_deref()).or(subject);
    let (event_detail_left, event_detail_title) =
        tool::event_detail_titles(name, subject.as_deref());
    let mut call = ToolCall {
        agent_message: None,
        started_at: None,
        hide_output: result
            .is_some_and(|result| !result.error && tool::output_hidden(mode, &result.value)),
        model_settings: None,
        agent_thread_id: None,
        agent_path: None,
        event_detail_left,
        event_detail_title,
        show_event_detail: true,
        id: String::new(),
        title: tool::list_title(name, subject.as_deref(), input, exit_code),
        name: name.to_owned(),
        input: sections,
        output,
        exit_code,
        is_error: state.is_some_and(|state| state.is_error),
        duration: None,
    };
    call.agent_message = crate::event::agent::tool_message(name, input);
    if call
        .agent_message
        .as_ref()
        .is_some_and(|message| message.kind == "SPAWN_AGENT")
    {
        let model = result
            .and_then(|result| result.value["resolvedModel"].as_str())
            .or_else(|| input["model"].as_str());
        call.model_settings = model.map(|model| crate::event::ModelSettings {
            provider: crate::providers::ProviderId::Claude,
            model: Some(model.to_owned()),
            effort: None,
        });
    }
    crate::event::agent::present(&mut call);
    crate::providers::tool::annotate(&mut call);
    call
}

impl EventReader {
    fn source_records(&self) -> Vec<crate::event::SourceRecord> {
        self.path
            .as_ref()
            .map(|path| crate::event::SourceRecord {
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
        let turn = self.turns.as_ref().and_then(|turns| turns.at(self.offset));
        if turn.map(|turn| turn.id) != self.turn.map(|turn| turn.id) {
            self.batch = None;
            self.assistant = None;
        }
        self.turn = turn;
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
                            let content = result_content(&block["content"]);
                            let result = self.results.entry(id.to_owned()).or_insert(ResultAt {
                                source_records: Vec::new(),
                                at,
                                sequence: self.sequence,
                                error: false,
                                denial: None,
                                content: result_text(&content),
                                raw_content: content,
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
                let text = prompt_text(content);
                let wrapped = text.as_deref().and_then(wrapped_message);
                let is_peer = wrapped
                    .as_ref()
                    .is_some_and(|(kind, _, _)| matches!(kind, Kind::Agent | Kind::Session));
                if let Some(text) = text.filter(|text| !text.trim().is_empty())
                    && (record["isMeta"] != true || is_peer)
                    && !["<task-notification>", "<system-reminder>"]
                        .iter()
                        .any(|prefix| text.trim_start().starts_with(prefix))
                {
                    if let Some((kind, sender, body)) = wrapped {
                        self.push_wrapped(&record, kind, sender, body, at);
                    } else if is_prompt_text(&record, &text) {
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
            Some("attachment") if super::images::queued_prompt(&record).is_some() => {
                let prompt = super::images::queued_prompt(&record).unwrap();
                if let Some(text) = prompt_text(prompt) {
                    if let Some((kind, sender, body)) = wrapped_message(&text)
                        .filter(|(kind, _, _)| matches!(kind, Kind::Agent | Kind::Session))
                    {
                        self.push_wrapped(&record, kind, sender, body, at);
                    } else {
                        let body = prompt_body(&text);
                        let images = self.images.take(&record, &body)?;
                        self.push(Kind::User, body, at);
                        let event = &mut self.entries.last_mut().unwrap().event;
                        event.source = record["uuid"].as_str().map(str::to_owned);
                        event.images = images;
                    }
                }
            }
            Some("queue-operation") if record["operation"] == "enqueue" => {
                if self.delivered.contains(&self.offset) {
                    return Ok(!self.ready(count));
                }
                if let Some(text) = record["content"].as_str() {
                    if self.notification(text, at, record["uuid"].as_str()) {
                        return Ok(!self.ready(count));
                    }
                    let (kind, body) = if text.starts_with('<') {
                        (Kind::QueuedSystem, text)
                    } else {
                        (Kind::QueuedUser, text)
                    };
                    let peer = wrapped_message(body);
                    let is_peer = peer
                        .as_ref()
                        .is_some_and(|(kind, _, _)| matches!(kind, Kind::Agent | Kind::Session));
                    let (sender, body) = peer
                        .map(|(_, sender, body)| (sender, body))
                        .unwrap_or_else(|| (None, body.to_owned()));
                    let images = self.images.take(&record, &body)?;
                    self.push(kind, body, at);
                    self.entries.last_mut().unwrap().event.images = images;
                    self.entries.last_mut().unwrap().event.source =
                        record["uuid"].as_str().map(str::to_owned);
                    self.entries.last_mut().unwrap().event.sender = sender;
                    if is_peer {
                        self.peer_message();
                    }
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
        let output = field("output-file").map(|path| {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) => {
                    return Section::Placeholder(if error.kind() == io::ErrorKind::NotFound {
                        "File no longer exists".to_owned()
                    } else {
                        error.to_string()
                    });
                }
            };
            if let Some(text) = crate::codex_await_output::message(&text) {
                return Section::Markdown {
                    label: String::new(),
                    text,
                };
            }
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
                return Section::Text {
                    label: String::new(),
                    text,
                };
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
        });
        self.push(Kind::Tool, summary.to_owned(), at);
        let event = &mut self.entries.last_mut().unwrap().event;
        event.source = source.map(str::to_owned);
        if field("task-id").is_some() || output.is_some() {
            event.calls.push(ToolCall {
                agent_message: None,
                started_at: None,
                hide_output: false,
                model_settings: None,
                agent_thread_id: None,
                agent_path: None,
                event_detail_left: String::new(),
                event_detail_title: summary.to_owned(),
                show_event_detail: false,
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

    /// Pushes a wrapped message, taking the sender and body from the record's origin when it
    /// carries one.
    fn push_wrapped(
        &mut self,
        record: &serde_json::Value,
        kind: Kind,
        sender: Option<String>,
        body: String,
        at: DateTime<Utc>,
    ) {
        let (sender, body) = super::peer::origin_message(record)
            .map(|(name, body)| (name.map(str::to_owned), body.to_owned()))
            .unwrap_or((sender, body));
        let is_peer = matches!(kind, Kind::Agent | Kind::Session);
        self.push(kind, body, at);
        let event = &mut self.entries.last_mut().unwrap().event;
        event.source = record["uuid"].as_str().map(str::to_owned);
        event.sender = sender;
        if is_peer {
            self.peer_message();
        }
    }

    fn peer_message(&mut self) {
        let event = &mut self.entries.last_mut().unwrap().event;
        let message = crate::event::agent::Message {
            author: event.sender.clone(),
            recipient: None,
            direction: crate::event::agent::Direction::Incoming,
            kind: "MESSAGE".to_owned(),
            content: crate::event::agent::Content::Text(event.body.clone()),
        };
        let mut call = ToolCall {
            model_settings: None,
            agent_message: Some(message),
            started_at: Some(event.started_at),
            hide_output: true,
            agent_thread_id: None,
            agent_path: event.sender.clone(),
            event_detail_left: String::new(),
            event_detail_title: String::new(),
            show_event_detail: true,
            id: event.source.clone().unwrap_or_default(),
            title: String::new(),
            name: "MESSAGE".to_owned(),
            input: Vec::new(),
            output: None,
            exit_code: None,
            is_error: false,
            duration: None,
        };
        crate::event::agent::present(&mut call);
        event.calls.push(call);
        self.coalesce();
    }

    fn coalesce(&mut self) {
        if self.entries.len() < 2 {
            return;
        }
        let n = self.entries.len();
        if !crate::event::agent::can_merge(&self.entries[n - 1].event, &self.entries[n - 2].event) {
            return;
        }
        let mut earlier = self.entries.pop().unwrap();
        let later = self.entries.pop().unwrap();
        earlier.event = crate::event::agent::merge(vec![earlier.event, later.event]).remove(0);
        self.entries.push(earlier);
    }

    fn push(&mut self, kind: Kind, body: String, started_at: DateTime<Utc>) {
        let ended_at = matches!(kind, Kind::Tool).then_some(started_at);
        self.entries.push(Entry {
            offset: self.offset,
            event: Event {
                turn: self.turn,
                completed_time: false,
                background: false,
                source: None,
                source_records: self.source_records(),
                sender: None,
                detail_body: None,
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
            last_result: None,
            labels: Vec::new(),
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
        let event_detail_call = tool_call(name, input, result);
        if let Some(message) = &event_detail_call.agent_message {
            self.batch = None;
            let records = result
                .map(|result| result.source_records.clone())
                .unwrap_or_default();
            let ended_at = result.map(|result| result.at);
            let denied = result.is_some_and(|result| result.denial.is_some());
            self.push(tool_kind(name), message.content.text().to_owned(), at);
            let event = &mut self.entries.last_mut().unwrap().event;
            event.source_records.extend(records);
            let mut call = event_detail_call;
            call.id = block["id"].as_str().unwrap_or("").to_owned();
            event.source = Some(call.id.clone());
            call.started_at = Some(at);
            call.duration = ended_at.map(|end| end - at);
            event.ended_at = ended_at;
            if denied {
                event.kind = Kind::Denied;
            } else if call.is_error {
                event.kind = Kind::Error;
            }
            event.calls.push(call);
            self.coalesce();
            return;
        }
        let subject = (!event_detail_call.event_detail_left.is_empty())
            .then_some(event_detail_call.event_detail_title.as_str());
        let label = tool::list_label(name, subject, input, event_detail_call.exit_code);
        let path = (tool::rule(name).subject == Some(Subject::Field("file_path")))
            .then(|| event_detail_call.event_detail_title.clone());
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
                    turn: self.turn,
                    completed_time: false,
                    background: false,
                    source: None,
                    source_records: self.source_records(),
                    sender: None,
                    detail_body: None,
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
                last_result: None,
                labels: Vec::new(),
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
        let mut path_subjects: Vec<_> = entry
            .event
            .paths
            .iter()
            .map(|range| entry.event.body[range.clone()].to_owned())
            .collect();
        path_subjects.extend(path);
        entry.labels.insert(0, label);
        let mut groups = crate::providers::tool::groups::Groups::default();
        for (head, member) in &entry.labels {
            groups.add(head, member.as_deref());
        }
        let (body, members) = groups.finish_with_ranges().unwrap();
        entry.event.paths = members
            .into_iter()
            .filter(|range| {
                path_subjects
                    .iter()
                    .any(|path| path == &body[range.clone()])
            })
            .collect();
        entry.event.body = body;
        let mut detail = event_detail_call;
        detail.id = block["id"].as_str().unwrap_or("").to_owned();
        detail.duration = result.map(|result| result.at - at);
        // Records and content blocks arrive newest first; this call is the earliest so far.
        entry.event.source = Some(detail.id.clone());
        entry.event.calls.insert(0, detail);
        entry.event.background |= input["run_in_background"] == true;
        entry.event.started_at = at;
        if let Some(result) = result {
            if result.denial.is_some() {
                entry.event.kind = Kind::Denied;
            } else if entry.event.kind != Kind::Denied {
                entry.event.kind = tool::result_kind(
                    entry.event.kind.clone(),
                    &tool::result_state(&result.content, &serde_json::Value::Null, result.error),
                );
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
    if pasted_content(text).is_some() {
        return Some((Kind::User, None, prompt_body(text)));
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

/// Human-origin prompts can start with XML; system wrappers remain system records.
pub(super) fn is_prompt_text(record: &serde_json::Value, text: &str) -> bool {
    !["<task-notification>", "<system-reminder>"]
        .iter()
        .any(|prefix| text.trim_start().starts_with(prefix))
        && (record["origin"]["kind"] == "human" || !text.trim_start().starts_with('<'))
}

/// Unwrap pasted text without changing its code blocks or whitespace.
pub(super) fn prompt_body(text: &str) -> String {
    crate::providers::text::printable(&pasted_content(text).unwrap_or_else(|| text.to_owned()))
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn dequeued_prompt_uses_delivery_position_and_time_in_incremental_and_tail_reads() {
        let fixture = include_str!("../../../tests/fixtures/claude-queued-dequeue.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut reader = Reader::new(file.path().into(), false);
        let mut loaded = Vec::new();
        for line in fixture.split_inclusive('\n') {
            file.write_all(line.as_bytes()).unwrap();
            let update = reader.refresh(20).unwrap();
            if update.reset || update.prepended > 0 || update.appended > 0 || update.replaced > 0 {
                loaded = update.events;
            }
            assert_eq!(loaded, tail(file.path(), 20).unwrap());
        }
        let prompts: Vec<_> = loaded
            .iter()
            .filter(|event| matches!(event.kind, Kind::User | Kind::QueuedUser))
            .collect();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].kind, Kind::User);
        assert_eq!(
            prompts[0].started_at,
            "2026-10-20T19:47:03.044Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(loaded.last().unwrap(), prompts[0]);
        assert_eq!(tail(file.path(), 1).unwrap(), vec![prompts[0].clone()]);
    }

    #[test]
    fn dequeued_prompts_match_pending_text_in_fifo_order() {
        let records: Vec<Value> =
            include_str!("../../../tests/fixtures/claude-queued-dequeue.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut later = records[0].clone();
        later["timestamp"] = json!("2026-10-20T19:47:01.000Z");
        let mut unrelated = records[0].clone();
        unrelated["content"] = json!("Still waiting");
        for record in [&records[0], &unrelated, &later, &records[2], &records[6]] {
            writeln!(file, "{record}").unwrap();
        }
        let events = tail(file.path(), 20).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].body, "Still waiting");
        assert_eq!(events[1].kind, Kind::QueuedUser);
        assert_eq!(
            events[1].started_at,
            "2026-10-20T19:47:01.000Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(events[2].kind, Kind::User);
        let mut reader = Reader::new(file.path().into(), false);
        assert_eq!(reader.refresh(20).unwrap().events, events);
        writeln!(file, "{}", records[6]).unwrap();
        assert_eq!(
            reader.refresh(20).unwrap().events,
            tail(file.path(), 20).unwrap()
        );
        assert_eq!(
            tail(file.path(), 20)
                .unwrap()
                .iter()
                .filter(|e| e.kind == Kind::QueuedUser)
                .count(),
            1
        );
    }

    #[test]
    fn turn_groups_keep_queued_steering_in_the_running_turn() {
        let fixture = include_str!("../../../tests/fixtures/turn-groups/claude-steer.jsonl");
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().write_all(fixture.as_bytes()).unwrap();
        let events = tail(file.path(), usize::MAX).unwrap();
        assert!(events.len() >= 4);
        let turn = events[0].turn.expect("prompt starts a turn");
        assert!(turn.started && turn.complete);
        assert!(events.iter().all(|event| event.turn == Some(turn)));
        let steering = events
            .iter()
            .find(|event| event.body == "说的是起一个，不是七个。")
            .unwrap();
        assert_eq!(steering.kind, Kind::User);
        assert_eq!(
            steering.started_at,
            "2026-10-20T02:59:09.229Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert!(events.iter().all(|event| event.kind != Kind::QueuedUser));
        let tail = tail(file.path(), 2).unwrap();
        assert_eq!(tail.len(), 2);
        assert!(tail.iter().all(|event| {
            event
                .turn
                .is_some_and(|part| part.id == turn.id && !part.started && part.complete)
        }));
    }

    #[test]
    fn turn_groups_refresh_completion_without_an_event_row() {
        let fixture = include_str!("../../../tests/fixtures/turn-groups/claude-steer.jsonl");
        let split = fixture.rfind('\n').unwrap();
        let split = fixture[..split].rfind('\n').unwrap() + 1;
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file()
            .write_all(&fixture.as_bytes()[..split])
            .unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let running = reader.refresh(usize::MAX).unwrap();
        assert!(
            running
                .events
                .iter()
                .all(|event| event.turn.is_some_and(|turn| !turn.complete))
        );
        file.as_file()
            .write_all(&fixture.as_bytes()[split..])
            .unwrap();
        let finished = reader.refresh(usize::MAX).unwrap();
        assert_eq!(finished.events.len(), running.events.len());
        assert!(
            finished
                .events
                .iter()
                .all(|event| event.turn.is_some_and(|turn| turn.complete))
        );
        assert!(finished.replaced > 0);
    }

    #[test]
    fn turn_groups_notification_wakes_without_a_user_row() {
        let fixture = include_str!("../../../tests/fixtures/turn-groups/claude-wakeup.jsonl");
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().write_all(fixture.as_bytes()).unwrap();
        let events = tail(file.path(), usize::MAX).unwrap();
        assert!(!events.is_empty());
        assert!(events.iter().all(|event| event.kind != Kind::User));
        assert!(
            events
                .iter()
                .all(|event| event.turn.is_some_and(|turn| turn.started && turn.complete))
        );
    }

    #[test]
    fn detach_subject_uses_result_name() {
        let records = include_str!("../../../tests/fixtures/claude-manager-detach-name.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records);
        assert_eq!(events[0].body, "codex-manager detach 模型和new挪位");
        assert_eq!(events[0].calls[0].event_detail_title, "模型和new挪位");
    }

    #[test]
    fn detach_subject_falls_back_to_thread_id() {
        let records: Vec<Value> =
            include_str!("../../../tests/fixtures/claude-manager-detach.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let thread_id = records[0]["message"]["content"][0]["input"]["threadId"]
            .as_str()
            .unwrap()
            .to_owned();
        for records in [records[..1].to_vec(), records] {
            let events = events_from(records);
            assert_eq!(events[0].body, format!("codex-manager detach {thread_id}"));
            assert_eq!(events[0].calls[0].event_detail_title, thread_id);
        }
    }

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
            vec![Section::Structured {
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
                vec![Section::Structured {
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

    include!("notification_tests.rs");

    #[test]
    fn task_notification_delivery_keeps_only_enqueue_rows() {
        use std::io::Write;
        let fixture =
            include_str!("../../../tests/fixtures/claude-task-notification-delivery.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut reader = super::Reader::new(file.path().to_owned(), false);
        let mut enqueued = Vec::new();
        let mut loaded = Vec::new();
        for line in fixture.lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            writeln!(file, "{line}").unwrap();
            if record["type"] == "queue-operation" {
                enqueued.push(record.clone());
            }
            let update = reader.refresh(usize::MAX).unwrap();
            let changed =
                update.reset || update.prepended > 0 || update.appended > 0 || update.replaced > 0;
            if changed {
                loaded = update.events;
            } else {
                assert!(
                    update.events.is_empty(),
                    "unchanged updates carry no event copies"
                );
            }
            if record["type"] != "queue-operation" {
                assert_eq!(update.appended, 0, "delivery must not append a row");
            }
            let events = super::tail(file.path(), usize::MAX).unwrap();
            assert_eq!(loaded, events);
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
            let mut loaded = Vec::new();
            for line in fixture.split_inclusive('\n') {
                file.write_all(line.as_bytes()).unwrap();
                let update = reader.refresh(usize::MAX).unwrap();
                let changed = update.reset
                    || update.prepended > 0
                    || update.appended > 0
                    || update.replaced > 0;
                if changed {
                    loaded = update.events;
                } else {
                    assert!(
                        update.events.is_empty(),
                        "unchanged updates carry no event copies"
                    );
                }
                assert_eq!(loaded, super::tail(file.path(), usize::MAX).unwrap());
                assert_eq!(update.parsed_bytes, line.len() as u64);
            }
            assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        }
    }

    #[test]
    fn single_tool_source_is_its_real_tool_use_id() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // Include ordinary tools and the separately rendered Agent call.
        for pair in records[..16].chunks_exact(2) {
            let id = pair[0]["message"]["content"][0]["id"].as_str().unwrap();
            for prefix in 1..=pair.len() {
                let events = events_from(pair[..prefix].to_vec());
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].calls[0].id, id);
                assert_eq!(events[0].source.as_deref(), Some(id));
            }
        }
    }

    #[test]
    fn parallel_tool_source_stays_on_first_real_call_as_transcript_grows() {
        let lines: Vec<_> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .split_inclusive('\n')
            .collect();
        let records: Vec<Value> = lines[16..19]
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[0]["message"]["id"], records[1]["message"]["id"]);
        let ids: Vec<_> = records[..2]
            .iter()
            .map(|record| record["message"]["content"][0]["id"].as_str().unwrap())
            .collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut reader = Reader::new(file.path().into(), false);
        for (index, line) in lines[16..19].iter().enumerate() {
            file.write_all(line.as_bytes()).unwrap();
            let incremental = reader.refresh(usize::MAX).unwrap().events;
            let full = tail(file.path(), usize::MAX).unwrap();
            assert_eq!(incremental, full);
            assert_eq!(full.len(), 1);
            let event = &full[0];
            assert_eq!(
                event
                    .calls
                    .iter()
                    .map(|call| call.id.as_str())
                    .collect::<Vec<_>>(),
                ids[..(index + 1).min(2)]
            );
            assert_eq!(event.source.as_deref(), Some(ids[0]));
            assert_eq!(event.calls[0].output.is_some(), index == 2);
        }
    }

    #[test]
    fn real_mcp_calls_share_display_names_and_keep_raw_names() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records.clone());
        for call in events
            .iter()
            .flat_map(|event| &event.calls)
            .filter(|call| call.name.starts_with("mcp__"))
        {
            assert!(!call.title.contains("mcp__"));
        }
    }

    #[test]
    fn declared_tool_details_restore_real_transcript_inputs() {
        for fixture in [
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl"),
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl"),
            include_str!("../../../tests/fixtures/claude-skill-call.jsonl"),
        ] {
            for line in fixture.lines() {
                let record: Value = serde_json::from_str(line).unwrap();
                for block in record["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = block["name"].as_str() else {
                        continue;
                    };
                    let (subject, field, markdown) = match name {
                        "Agent" | "Task" => ("name", "prompt", true),
                        "SendMessage" => ("to", "message", true),
                        "WebFetch" => ("url", "prompt", false),
                        "Skill" => ("skill", "args", false),
                        _ => continue,
                    };
                    let input = &block["input"];
                    let call = super::tool_call(name, input, None);
                    let expected = input[subject]
                        .as_str()
                        .or_else(|| {
                            matches!(name, "Agent" | "Task")
                                .then(|| input["description"].as_str())
                                .flatten()
                        })
                        .unwrap_or(name);
                    assert_eq!(
                        call.event_detail_title,
                        if name == "SendMessage" {
                            format!("← {}", crate::event::agent::name(expected))
                        } else {
                            expected.to_owned()
                        }
                    );
                    if let Some(message) = &call.agent_message {
                        assert_eq!(call.event_detail_left, message.label());
                    } else {
                        assert_eq!(
                            call.event_detail_left,
                            if input[subject]
                                .as_str()
                                .is_some_and(|s| !s.trim().is_empty())
                            {
                                name
                            } else {
                                ""
                            }
                        );
                    }
                    let text = input[field].as_str().unwrap_or("").to_owned();
                    match name {
                        "Agent" | "Task" => {
                            assert_eq!(
                                call.input[0],
                                Section::Markdown {
                                    label: String::new(),
                                    text
                                }
                            );
                        }
                        "WebFetch" => assert!(
                            matches!(&call.input[0], Section::Text { text, .. } if text == &format!("url: {}", input["url"].as_str().unwrap()))
                        ),
                        "SendMessage" => assert_eq!(
                            call.input[0],
                            Section::Markdown {
                                label: String::new(),
                                text
                            }
                        ),
                        "Skill" => assert_eq!(
                            call.input,
                            vec![Section::Text {
                                label: "args".into(),
                                text
                            }]
                        ),
                        _ => assert_eq!(call.input, vec![Section::Prose { text, markdown }]),
                    }
                }
            }
        }
    }

    #[test]
    fn skill_input_uses_args_and_skill_subject() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-skill-call.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records.clone());
        let call = &events[0].calls[0];
        let input = &records[0]["message"]["content"][0]["input"];
        assert_eq!(events[0].body, "Skill computer-use:capture-window");
        assert_eq!(call.event_detail_title, input["skill"].as_str().unwrap());
        assert_eq!(call.event_detail_left, "Skill");
        assert_eq!(
            call.input,
            vec![Section::Text {
                text: input["args"].as_str().unwrap().to_owned(),
                label: "args".into(),
            }]
        );
    }

    #[test]
    fn agent_input_uses_markdown_prompt_and_description_subject() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-agent-display.jsonl");
        for call in tail(&fixture, 10)
            .unwrap()
            .iter()
            .flat_map(|event| &event.calls)
        {
            assert!(!call.event_detail_title.contains("实测 attach 发提示词"));
            assert_eq!(call.event_detail_left, "Spawn agent");
            assert!(matches!(call.input[0], Section::Markdown { .. }));
        }
    }

    #[test]
    fn real_call_titles_and_ids_match_inputs() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
        let events = tail(&path, 100).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let bash = calls.iter().find(|call| call.name == "Bash").unwrap();
        assert_eq!(bash.title, "改到持久目录");
        assert_eq!(bash.id, "toolu_01XU5dCe5eBxX8ccqC2DwN2G");
        assert_eq!(
            calls
                .iter()
                .find(|call| call.name == "Agent")
                .unwrap()
                .event_detail_title,
            "写工具详情计划"
        );
        assert_eq!(
            calls
                .iter()
                .find(|call| call.name == "Skill")
                .unwrap()
                .event_detail_title,
            "secrets:secrets-manager"
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
    fn bash_subject_is_removed_from_command_only_input() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
        let events = tail(&path, 100).unwrap();
        let bash = events
            .iter()
            .flat_map(|event| &event.calls)
            .find(|call| call.name == "Bash")
            .unwrap();
        assert_eq!(bash.event_detail_left, "Bash");
        assert_eq!(bash.input.len(), 1);
        assert!(matches!(bash.input[0], Section::Shell { .. }));
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
        assert_eq!(events[0].kind, Kind::User);
        assert_eq!(
            events[0].started_at,
            delivery["timestamp"]
                .as_str()
                .unwrap()
                .parse::<DateTime<Utc>>()
                .unwrap()
        );
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
        let events = super::tail(&path, 20).unwrap();
        assert_eq!(events.len(), 2);
        for (event, numbers) in events.iter().zip([&[6][..], &[8, 9]]) {
            assert_eq!(event.kind, crate::event::Kind::User);
            super::super::transcript::tests::assert_embedded_images(&event.images, numbers);
        }
        assert_eq!(super::tail(&path, 1).unwrap()[0].images, events[1].images);
    }

    use super::*;
    use serde_json::{Value, json};
    use std::io::Write;

    #[test]
    fn declared_tool_rules_remove_subjects_and_move_patch_to_output() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records.clone());
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let read = calls.iter().find(|call| call.name == "Read").unwrap();
        let file = &records[3]["toolUseResult"]["file"];
        assert_eq!(
            read.event_detail_title,
            format!(
                "{}:{}-{}",
                file["filePath"].as_str().unwrap(),
                file["startLine"],
                file["startLine"].as_u64().unwrap() + file["numLines"].as_u64().unwrap() - 1
            )
        );
        assert_eq!(read.title, format!("Read {}", read.event_detail_title));
        assert!(read.input.is_empty());
        let edit = calls.iter().find(|call| call.name == "Edit").unwrap();
        assert!(edit.input.is_empty());
        assert!(
            matches!(&edit.output.as_ref().unwrap()[0], Section::Diff { text, .. } if text.starts_with("@@ -"))
        );
        let agent = calls.iter().find(|call| call.name == "Agent").unwrap();
        assert_ne!(agent.event_detail_title, "Agent");
        assert!(matches!(&agent.input[0], Section::Markdown { .. }));
    }

    #[test]
    fn captured_shell_diff_pipeline_uses_diff_section() {
        let records = include_str!("../../../tests/fixtures/claude-shell-diff.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records);
        assert!(matches!(&events[0].calls[0].output.as_ref().unwrap()[0],
            Section::Diff { text, .. } if text.starts_with("diff --git ") && text.contains("@@")));
    }

    #[test]
    fn shell_file_reads_highlight_stdout() {
        for (command, highlighted) in [
            ("sed -n '10,80p' src/foo.rs", true),
            ("cat 'src/foo bar.rs'", true),
            ("head -n 50 src/foo.rs", true),
            ("tail src/foo.rs", true),
            ("cat foo.unknown-language", false),
            ("sed -n 1,5p a.rs | grep x", false),
        ] {
            let mut records: Vec<Value> =
                include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
                    .lines()
                    .take(2)
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
            records[0]["message"]["content"][0]["input"]["command"] = command.into();
            records[1]["toolUseResult"]["stdout"] = "fn main() {}".into();
            records[1]["toolUseResult"]["stderr"] = "diagnostic".into();
            records[1]["message"]["content"][0]["content"] = "fn main() {}".into();
            let events = events_from(records);
            let output = events[0].calls[0].output.as_ref().unwrap();
            if highlighted {
                assert_eq!(output[0], code_section("stdout", "rs", "fn main() {}"));
            } else {
                assert!(matches!(&output[0], Section::Text { .. }));
            }
            assert!(matches!(&output[1], Section::Label(label) if label == "stderr"));
            assert!(
                matches!(&output[2], Section::Text { label, text } if label == "stderr" && text == "diagnostic")
            );
        }
    }

    #[test]
    fn shell_known_separator_keeps_mixed_language_block_plain() {
        let mut records: Vec<Value> =
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .take(2)
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let command = "sed -n '1p' src/main.rs\necho '---'; head -n 1 probe.py && cat Cargo.toml";
        let stdout = "fn main() {}\n---\nprint('ready')\n[package]\nname = \"agents\"";
        records[0]["message"]["content"][0]["input"]["command"] = command.into();
        records[1]["toolUseResult"]["stdout"] = stdout.into();
        records[1]["toolUseResult"]["stderr"] = "".into();
        records[1]["message"]["content"][0]["content"] = stdout.into();
        let events = events_from(records);
        let output = events[0].calls[0].output.as_ref().unwrap();
        assert_eq!(
            output,
            &vec![
                code_section("stdout", "rs", "fn main() {}\n"),
                Section::Text {
                    label: "stdout".into(),
                    text: "---\n".into(),
                },
                Section::Text {
                    label: "stdout".into(),
                    text: "print('ready')\n[package]\nname = \"agents\"".into(),
                },
            ]
        );
    }

    #[test]
    fn tool_calls_keep_input_and_result_sections() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let events = events_from(records.clone());
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let bash = calls.iter().find(|call| call.name == "Bash").unwrap();
        assert!(matches!(bash.input[0], Section::Shell { prefix: 0, .. }));
        let output = bash.output.as_ref().unwrap();
        assert!(matches!(&output[0], Section::Text { label, .. } if label == "stdout"));
        assert!(matches!(&output[1], Section::Label(label) if label == "stderr"));
        let read = calls.iter().find(|call| call.name == "Read").unwrap();
        assert_eq!(
            read.output.as_ref().unwrap()[0],
            code_section(
                "output",
                "conf",
                records[3]["toolUseResult"]["file"]["content"]
                    .as_str()
                    .unwrap()
            )
        );
        let write = calls.iter().find(|call| call.name == "Write").unwrap();
        assert_eq!(
            write.input,
            vec![Section::Markdown {
                label: "input".into(),
                text: records[6]["message"]["content"][0]["input"]["content"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            }]
        );
        for call in calls.iter().filter(|call| call.name.starts_with("mcp__")) {
            let block = records
                .iter()
                .map(|record| &record["message"]["content"][0])
                .find(|block| block["id"] == call.id)
                .unwrap();
            assert_eq!(
                call.input,
                vec![Section::Markdown {
                    label: "prompt".into(),
                    text: block["input"]["prompt"].as_str().unwrap().to_owned(),
                }]
            );
        }
    }

    #[test]
    fn tool_reference_results_keep_tool_names_on_separate_lines() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-search.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let expected = records[1]["toolUseResult"]["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let events = events_from(records);
        assert_eq!(
            events[0].calls[0].output,
            Some(vec![text_section("output", &expected)])
        );
        assert!(events[0].calls[0].input.is_empty());
    }

    #[test]
    fn captured_read_with_empty_file_content_has_no_output_sections() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let call = records
            .iter()
            .find(|record| record["message"]["content"][0]["name"] == "Read")
            .unwrap()
            .clone();
        let id = call["message"]["content"][0]["id"].as_str().unwrap();
        let mut result = records
            .iter()
            .find(|record| record["message"]["content"][0]["tool_use_id"] == id)
            .unwrap()
            .clone();
        result["toolUseResult"]["file"]["content"] = "".into();
        result["message"]["content"][0]["content"] = "".into();
        let events = events_from(vec![call, result]);
        assert_eq!(events[0].calls[0].output, Some(Vec::new()));
    }

    #[test]
    fn captured_shell_failure_has_the_same_status_in_row_and_event_detail() {
        let events = events_from(
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect(),
        );
        let event = events
            .iter()
            .find(|event| event.calls.iter().any(|call| call.exit_code == Some(1)))
            .unwrap();
        let call = event
            .calls
            .iter()
            .find(|call| call.exit_code == Some(1))
            .unwrap();
        assert!(call.title.ends_with(" · 1"), "{}", call.title);
        assert!(event.body.contains(" · 1"), "{}", event.body);
        assert!(call.is_error);
    }

    #[test]
    fn tool_use_error_shows_its_body() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let body = "This background session hasn't isolated its changes yet. Call EnterWorktree first so edits land in a worktree instead of the shared checkout, then retry this edit using the worktree path (a path inside a linked git worktree, including one you create with `git worktree add`, is accepted). (To disable this guard for this repo, set `\"worktree\": {\"bgIsolation\": \"none\"}` in .claude/settings.json.)";
        let mut error = records[1].clone();
        error["message"]["content"][0]["is_error"] = true.into();
        error["message"]["content"][0]["content"] =
            format!("<tool_use_error>{body}</tool_use_error>").into();
        error["toolUseResult"] = format!("Error: {body}").into();
        let events = events_from(vec![records[0].clone(), error]);
        let call = &events[0].calls[0];
        assert_eq!(call.output, Some(vec![text_section("output", body)]));
        assert!(call.is_error);
    }

    #[test]
    fn tool_call_fallbacks_keep_reasons() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut error = records[1].clone();
        error["message"]["content"][0]["is_error"] = true.into();
        error["message"]["content"][0]["content"] = "Exit code 1\nreason".into();
        error["toolUseResult"] = "Error: Exit code 1\nreason".into();
        let events = events_from(vec![records[0].clone(), error]);
        let call = &events[0].calls[0];
        assert_eq!(call.exit_code, Some(1));
        assert_eq!(call.output, Some(vec![text_section("output", "reason")]));
        assert!(call.is_error);
        assert_eq!(
            result_text(
                &json!([{"type":"image"},{"type":"text","text":"one"},{"type":"text","text":"two"}])
            ),
            "one\ntwo"
        );
    }

    #[test]
    fn real_merged_peer_activity_uses_the_event_row() {
        let peer = include_str!("../../../tests/fixtures/claude-peer-prompts.jsonl")
            .lines()
            .next()
            .unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // A different real event bounds the consecutive peer deliveries.
        writeln!(
            file,
            "{}",
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
                .lines()
                .next()
                .unwrap()
        )
        .unwrap();
        for count in 1..=3 {
            writeln!(file, "{peer}").unwrap();
            let events = tail_session(file.path(), usize::MAX, true).unwrap();
            let event = events.last().unwrap();
            assert_eq!(event.calls.len(), count);
            let summary = super::super::transcript::read_subagent(
                file.path(),
                file.as_file().metadata().unwrap().len(),
            )
            .unwrap();
            let session = super::super::session("peer-activity".into(), summary, None, Utc::now());
            let activity = session.latest_activity().unwrap();
            assert_eq!(activity.label, event.summary());
            assert_eq!(activity.kind, event.kind);
            assert_eq!(activity.at, event.started_at);
        }
    }

    #[test]
    fn real_absorbed_handback_shows_sender_and_report() {
        let fixture = include_str!("../../../tests/fixtures/claude-peer-handback-absorbed.jsonl");
        let origin = &serde_json::from_str::<Value>(fixture.lines().last().unwrap()).unwrap()
            ["attachment"]["origin"];
        let report = origin["handbackReportText"].as_str().unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{fixture}").unwrap();
        let events = tail_session(file.path(), usize::MAX, true).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Agent);
        assert_eq!(events[0].sender.as_deref(), Some("general-purpose"));
        assert_eq!(events[0].body, report);
        let summary =
            super::super::transcript::read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap();
        assert_eq!(
            summary.prompt,
            crate::event::participant_text(Some("general-purpose"), report)
        );
    }

    #[test]
    fn real_activity_uses_the_latest_event() {
        for fixture in [
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl"),
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl"),
            include_str!("../../../tests/fixtures/claude-questions-pending.jsonl"),
            include_str!("../../../tests/fixtures/claude-manager-detach-name.jsonl"),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            for line in fixture.lines() {
                writeln!(file, "{line}").unwrap();
                let events = tail_session(file.path(), usize::MAX, true).unwrap();
                let Some(event) = events.iter().rev().find(|event| !event.calls.is_empty()) else {
                    continue;
                };
                let summary = super::super::transcript::read_subagent(
                    file.path(),
                    file.as_file().metadata().unwrap().len(),
                )
                .unwrap();
                let activity = summary.activity.unwrap();
                assert_eq!(activity.label, event.summary());
                assert_eq!(activity.kind, event.kind);
                assert_eq!(activity.at, event.started_at);
                assert_eq!(
                    summary.task_details_sources[0].as_deref(),
                    event.calls.first().map(|call| call.id.as_str())
                );
            }
        }
    }

    #[test]
    fn questions_use_the_same_headers_in_event_and_activity() {
        for fixture in [
            include_str!("../../../tests/fixtures/claude-questions-pending.jsonl"),
            include_str!("../../../tests/fixtures/claude-questions-answered.jsonl"),
        ] {
            let record: serde_json::Value =
                serde_json::from_str(fixture.lines().next().unwrap()).unwrap();
            let mut file = tempfile::NamedTempFile::new().unwrap();
            write!(file, "{fixture}").unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0].body,
                format!(
                    "AskUserQuestion {}",
                    record["message"]["content"][0]["input"]["questions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|question| question["header"].as_str().unwrap())
                        .collect::<Vec<_>>()
                        .join(" · ")
                )
            );
            assert_eq!(events[0].kind, Kind::Assistant);
            let len = file.as_file().metadata().unwrap().len();
            let summary = super::super::transcript::read(file.path(), len).unwrap();
            let activity = summary.activity.unwrap();
            assert_eq!(activity.label, events[0].summary());
            assert_eq!(activity.kind, Kind::Assistant);
        }
    }

    #[test]
    fn real_tool_details_match_event_and_latest_activity() {
        for line in include_str!("../../../tests/fixtures/claude-tool-details.jsonl").lines() {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{line}").unwrap();
            let events = tail_session(file.path(), 8, true).unwrap();
            assert_eq!(events.len(), 1);
            let record: Value = serde_json::from_str(line).unwrap();
            let block = &record["message"]["content"][0];
            let rows = [
                "mkdir, cd, ls, wc, head",
                "找那个会话",
                "hide-idle-teammates ← message",
                "team-lead ← message",
                "Report: /tmp/research",
                "WebFetch https://code.claude.com/docs/en/hooks",
                "cd, ls, sed, wc",
                "cd, cat, wc, sed",
            ];
            let index = include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
                .lines()
                .position(|candidate| candidate == line)
                .unwrap();
            let expected = if block["name"] == "SubagentHandback" {
                block["input"]["message"].as_str().unwrap()
            } else {
                rows[index]
            };
            assert_eq!(events[0].summary(), expected);
            assert_eq!(events[0].calls[0].name, block["name"].as_str().unwrap());
            let len = file.as_file().metadata().unwrap().len();
            let summary = super::super::transcript::read_subagent(file.path(), len).unwrap();
            assert_eq!(summary.activity.unwrap().label, events[0].summary());
        }
    }

    #[test]
    fn captured_agent_request_preserves_input_model_until_resolved() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let request = include_str!("../../../tests/fixtures/claude-agent-launch.jsonl")
            .lines()
            .next()
            .unwrap();
        std::fs::write(file.path(), request).unwrap();
        let events = tail(file.path(), usize::MAX).unwrap();
        let call = &events
            .iter()
            .find(|event| !event.calls.is_empty())
            .unwrap()
            .calls[0];
        assert_eq!(
            call.model_settings.as_ref().unwrap().model.as_deref(),
            Some("sonnet")
        );
        assert!(call.output.is_none());
    }

    #[test]
    fn captured_agent_launch_uses_description_and_resolved_model() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            include_str!("../../../tests/fixtures/claude-agent-launch.jsonl"),
        )
        .unwrap();
        let events = tail(file.path(), usize::MAX).unwrap();
        let event = events.iter().find(|event| !event.calls.is_empty()).unwrap();
        let call = &event.calls[0];
        assert_eq!(event.summary(), "Spawn agent 查 Claude 前台转后台");
        assert_eq!(call.event_detail_title, "查 Claude 前台转后台");
        let settings = call.model_settings.as_ref().unwrap();
        assert_eq!(settings.provider, crate::providers::ProviderId::Claude);
        assert_eq!(settings.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(settings.effort, None);
        assert!(call.hide_output);
        assert!(call.output.as_ref().unwrap().is_empty());
    }

    #[test]
    fn real_agent_names_in_event_and_activity() {
        let expected = [
            "reviewer · Review files",
            "Checks passed.\nReport details.",
            "worker-id → message",
            "team-lead → message",
            "peer-session → message",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
            .lines()
            .zip(expected)
        {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), line).unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            let row_expected = if expected == "reviewer · Review files" {
                "Spawn agent reviewer"
            } else {
                expected
            };
            assert_eq!(events[0].summary(), row_expected);
            if let Some(call) = events[0].calls.first()
                && let Some(message) = &call.agent_message
                && let Some(author) = &message.author
                && message.recipient.is_none()
            {
                assert_eq!(
                    call.event_detail_title,
                    format!("{} →", crate::event::agent::name(author))
                );
            }
            let record: Value = serde_json::from_str(line).unwrap();
            if record["type"] == "assistant" {
                let len = file.as_file().metadata().unwrap().len();
                let summary = super::super::transcript::read_subagent(file.path(), len).unwrap();
                assert_eq!(summary.activity.unwrap().label, events[0].summary());
            }
        }
    }

    #[test]
    fn task_and_handback_use_declared_titles_in_event_and_activity() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for (mut record, tool_name) in [
            (records[0].clone(), "Task"),
            (records[1].clone(), "SubagentHandback"),
        ] {
            let block = &mut record["message"]["content"][0];
            block["name"] = tool_name.into();
            block["input"]["name"] = "reviewer".into();
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), record.to_string()).unwrap();
            let row_expected = record["message"]["content"][0]["input"]["message"].as_str();
            assert_eq!(
                tail(file.path(), 1).unwrap()[0].summary(),
                if tool_name == "Task" {
                    "Spawn agent reviewer"
                } else {
                    row_expected.unwrap()
                }
            );
            let summary = super::super::transcript::read_subagent(
                file.path(),
                file.as_file().metadata().unwrap().len(),
            )
            .unwrap();
            assert_eq!(
                summary.activity.unwrap().label,
                tail(file.path(), 1).unwrap()[0].summary()
            );
        }
    }

    #[test]
    fn undeclared_tools_do_not_infer_subjects_from_participant_names() {
        let record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        for name in ["Agent", "Task", "SendMessage", "SubagentHandback"] {
            for participant in ["reviewer", " "] {
                let mut record = record.clone();
                let block = &mut record["message"]["content"][0];
                block["name"] = name.into();
                block["input"] =
                    json!({if name == "SendMessage" { "to" } else { "name" }: participant});
                let file = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(file.path(), record.to_string()).unwrap();
                let row_expected = match (name, participant.trim().is_empty()) {
                    ("Agent" | "Task", false) => "Spawn agent reviewer".to_owned(),
                    ("Agent" | "Task", true) => "Spawn agent".to_owned(),
                    ("SendMessage", false) => "reviewer ← message".to_owned(),
                    ("SendMessage", true) => "message".to_owned(),
                    _ => name.to_owned(),
                };
                assert_eq!(tail(file.path(), 1).unwrap()[0].summary(), row_expected);
                let summary = super::super::transcript::read_subagent(
                    file.path(),
                    file.as_file().metadata().unwrap().len(),
                )
                .unwrap();
                assert_eq!(
                    summary.activity.unwrap().label,
                    tail(file.path(), 1).unwrap()[0].summary()
                );
            }
        }
    }

    #[test]
    fn send_message_to_a_session_address_names_no_recipient() {
        let mut record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let block = &mut record["message"]["content"][0];
        block["name"] = "SendMessage".into();
        block["input"] = json!({"to": "uds:/tmp/cc-socks/29973.sock", "message": "No conflicts"});
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), record.to_string()).unwrap();
        assert_eq!(tail(file.path(), 1).unwrap()[0].summary(), "message");
        let summary = super::super::transcript::read_subagent(
            file.path(),
            file.as_file().metadata().unwrap().len(),
        )
        .unwrap();
        assert_eq!(
            summary.activity.unwrap().label,
            tail(file.path(), 1).unwrap()[0].summary()
        );
    }

    #[test]
    fn bash_blank_description_keeps_the_same_name_and_subject() {
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
        assert_eq!(summary.activity.unwrap().label, events[0].summary());
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
        assert!(events[0].body.starts_with("SyntaxWarning 修好了"));
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
    fn automode_blocked_preserves_denied_kind_and_result_reason() {
        let records = denial_records(include_str!(
            "../../../tests/fixtures/automode_blocked.jsonl"
        ));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let events = events_from(records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "echo");
        assert!(
            matches!(&events[0].calls[0].output.as_ref().unwrap()[0], Section::Text { text, .. } if text.contains("Create Unsafe Agents"))
        );
        assert_eq!(events[0].ended_at, Some(end));
    }

    #[test]
    fn automode_blocked_without_brackets_keeps_denied_kind() {
        let records = denial_records(include_str!(
            "../../../tests/fixtures/automode_blocked_dangerous.jsonl"
        ));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let events = events_from(records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Edit");
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
        assert_eq!(events[0].body, "echo");
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
    fn automode_reason_without_complete_brackets_keeps_denied_kind() {
        for text in ["denied", "Reason: [unfinished"] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[1]["message"]["content"][0]["content"] = text.into();
            let events = events_from(records);
            assert_eq!(events[0].kind, Kind::Denied);
            assert_eq!(events[0].body, "echo");
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
            assert!(events[0].body.contains("echo"));
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
    fn tool_after_answered_question_starts_at_the_answer() {
        let fixture = include_str!("../../../tests/fixtures/claude-questions-answered.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{fixture}").unwrap();
        let answered: DateTime<Utc> = "2026-10-13T17:45:49.705Z".parse().unwrap();
        for (seconds, mut r) in [
            (55, tool(0, "after", "b", "Bash", json!({"command":"ls"}))),
            (56, result(0, "b", false)),
        ] {
            r["timestamp"] = (answered + chrono::Duration::seconds(seconds - 49))
                .to_rfc3339()
                .into();
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 2).unwrap();
        assert_eq!(events[0].kind, Kind::Assistant);
        assert_eq!(events[1].previous_end, Some(answered));
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
    fn task_details_sources_identify_real_transcript_events() {
        let file = fixture();
        let summary =
            super::super::transcript::read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap();
        let events = tail(file.path(), usize::MAX).unwrap();
        for (index, kind) in [(1, Kind::Assistant), (2, Kind::User)] {
            let source = summary.task_details_sources[index].as_ref().unwrap();
            assert!(
                events
                    .iter()
                    .any(|event| event.source.as_ref() == Some(source) && event.kind == kind)
            );
        }
        let source = summary.task_details_sources[0].as_ref().unwrap();
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
            (&events[0..2], Kind::User, None, "first line\nsecond line\n"),
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
            assert!(event.body.contains("17:58:53 User    实话实说吧啊"));
            assert!(event.body.contains("那个五点五十八五十三秒"));
            assert!(event.body.contains("```"));
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
                assert_eq!(event.body, pasted_content(content).unwrap());
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
    fn only_declared_subject_fields_affect_rows() {
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
                "inspect files",
            ),
            (
                "bash",
                json!({"description":"inspect files"}),
                "inspect files",
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
    fn parallel_calls_use_declared_subjects_or_tool_names() {
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
        assert_eq!(events[0].body, "inspect files, Bash, read config");
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
        assert_eq!(events[0].body, "codex-manager detach, TaskStop");
    }

    #[test]
    fn trailing_asterisk_does_not_mark_a_foreground_tool_as_background() {
        let mut record: serde_json::Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/background/two-awaits.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let input = &mut record["message"]["content"][0]["input"];
        input["run_in_background"] = false.into();
        input["description"] = "literal *".into();
        let file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file.as_file(), "{record}").unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events[0].body, "literal *");
        assert!(!events[0].background);
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
        assert_eq!(events[2].body, "Read, Grep");
        assert_eq!(events[2].started_at, at(30));
        assert_eq!(events[2].ended_at, Some(at(35)));
        assert_eq!(events[3].body, "build");
        assert!(events[3].background);
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
        let mut expected_tail = events[5..].to_vec();
        for event in &mut expected_tail {
            event.turn.as_mut().unwrap().started = false;
        }
        assert_eq!(tail(file.path(), 2).unwrap(), expected_tail);
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
        assert_eq!(events[0].body, "Read b");
        assert_eq!(events[0].paths.len(), 1);
        assert_eq!(events[0].paths[0], "Read ".len().."Read b".len());
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
        assert_eq!(events[0].body, "Read");
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
            // The same transcript fragment occupies different byte positions in these files.
            if let (Some(actual), Some(expected)) = (&mut actual.turn, expected.turn) {
                actual.id = expected.id;
            }
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
        let mut event_panel = EventReader::default();
        super::super::transcript::walk_records(&mut measured, len, Vec::new(), false, |record| {
            event_panel.record(record, 2)
        })
        .unwrap();
        assert_eq!(measured.bytes_read, 64 * 1024);
        assert_eq!(measured.earliest, len - 64 * 1024);
        assert_eq!(
            event_panel.entries[0].event.body,
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

#[cfg(test)]
mod refresh_work_tests {
    #[test]
    fn unchanged_transcript_returns_no_event_copies() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            include_bytes!("../../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut reader = super::Reader::new(file.path().into(), false);
        let first = reader.refresh(8).unwrap();
        assert!(!first.events.is_empty());
        let unchanged = reader.refresh(8).unwrap();
        assert!(unchanged.events.is_empty(), "unchanged events were cloned");
        assert_eq!(unchanged.parsed_bytes, 0);
    }
}
