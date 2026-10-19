//! Token usage of one transcript over time, summed into fixed bins for the usage chart.
//!
//! Input counts what a request's prompt gained since the last request, the user's messages and
//! tool results, whatever the cache held; it lands when its request started. Output, thinking
//! and reasoning included, is spread evenly over the request's duration.

use crate::providers::ProviderId;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::Path;

/// Seconds of activity one bin sums.
pub const BIN_SECONDS: i64 = 5;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Bin {
    pub input: u64,
    pub output: u64,
}

/// The bin holding `at`.
pub fn bin_of(at: DateTime<Utc>) -> i64 {
    at.timestamp().div_euclid(BIN_SECONDS)
}

/// Usage of one transcript, read from where the last update stopped.
#[derive(Debug, Default)]
pub struct Series {
    /// Nonempty bins by bin number.
    pub bins: BTreeMap<i64, Bin>,
    context_tokens: Option<u64>,
    context_window: Option<u64>,
    /// Bytes of complete records read so far.
    read: u64,
    /// When the next request started: the latest input record, else the end of the last request.
    request_start: Option<DateTime<Utc>>,
    /// The prompt tokens and output of the request counted last.
    last_request: Option<(u64, u64)>,
    /// The Claude message counted last; its content blocks arrive as separate records repeating
    /// the usage.
    message: Option<String>,
}

impl Series {
    pub fn context_percent(&self) -> Option<u64> {
        let tokens = self.context_tokens?;
        let window = self.context_window.filter(|window| *window > 0)?;
        u64::try_from((u128::from(tokens) * 100 + u128::from(window) / 2) / u128::from(window)).ok()
    }

    /// Reads appended records; returns whether the bins or context percentage changed.
    pub fn update(&mut self, path: &Path, provider: ProviderId) -> io::Result<bool> {
        let before_context = self.context_percent();
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < self.read {
            // The transcript was rewritten; its old offsets mean nothing now.
            *self = Self::default();
        }
        if len == self.read {
            return Ok(self.context_percent() != before_context);
        }
        let before = self.bins.clone();
        self.read =
            crate::jsonl::read_lines(&mut file, self.read, |line| self.record(line, provider))?;
        Ok(self.bins != before || self.context_percent() != before_context)
    }

    fn record(&mut self, line: &[u8], provider: ProviderId) {
        match provider {
            ProviderId::Claude => self.claude_record(line),
            ProviderId::Codex => self.codex_record(line),
        }
    }

    fn claude_record(&mut self, line: &[u8]) {
        let Ok(record) = serde_json::from_slice::<ClaudeRecord>(line) else {
            return;
        };
        let Some(at) = record.timestamp.as_deref().and_then(parse_time) else {
            return;
        };
        match record.kind.as_deref() {
            Some("user") => self.request_start = Some(at),
            Some("assistant") => {
                let Some(ClaudeMessage {
                    id: Some(id),
                    usage: Some(usage),
                }) = record.message
                else {
                    return;
                };
                let prompt = usage.prompt_tokens();
                let output = usage.output_tokens;
                match &mut self.last_request {
                    Some((_, counted_output)) if self.message.as_ref() == Some(&id) => {
                        let extra = output.saturating_sub(*counted_output);
                        *counted_output = (*counted_output).max(output);
                        self.add(at, 0, extra);
                    }
                    _ => {
                        self.request(at, prompt, output);
                        self.message = Some(id);
                    }
                }
                self.request_start = Some(at);
            }
            _ => {}
        }
    }

    fn codex_record(&mut self, line: &[u8]) {
        let Ok(record) = serde_json::from_slice::<CodexRecord>(line) else {
            return;
        };
        let Some(at) = parse_time(&record.timestamp) else {
            return;
        };
        let Some(payload) = record.payload else {
            return;
        };
        match record.kind.as_str() {
            "event_msg" => match payload.kind.as_deref() {
                Some("task_started") => self.context_window = payload.model_context_window,
                Some("token_count") => {
                    if let Some(info) = payload.info {
                        self.context_tokens =
                            info.last_token_usage.and_then(|usage| usage.total_tokens);
                        if let Some(window) = info.model_context_window {
                            self.context_window = Some(window);
                        }
                    }
                }
                _ => {}
            },
            "response_item" => {
                let kind = payload.kind.as_deref().unwrap_or_default();
                let sent = kind.ends_with("_output")
                    || (kind == "message" && payload.role.as_deref() != Some("assistant"));
                if sent {
                    self.request_start = Some(at);
                }
            }
            "token_usage_record" => {
                let Some(usage) = payload.usage else {
                    return;
                };
                self.request(at, usage.input_tokens, usage.output_tokens);
                self.request_start = Some(at);
            }
            _ => {}
        }
    }

    /// Counts a request with `prompt` tokens that ended at `end`: its input at its start, its
    /// output spread up to `end`. Its input is what the prompt gained over the last request's
    /// prompt and output; a compacted prompt shrinks and gains nothing.
    fn request(&mut self, end: DateTime<Utc>, prompt: u64, output: u64) {
        let input = self
            .last_request
            .map_or(prompt, |(last_prompt, last_output)| {
                prompt.saturating_sub(last_prompt + last_output)
            });
        self.last_request = Some((prompt, output));
        let start = self
            .request_start
            .filter(|start| *start < end)
            .unwrap_or(end);
        self.add(start, input, 0);
        let span = (end - start).num_milliseconds();
        if span == 0 {
            self.add(end, 0, output);
            return;
        }
        let mut left = output;
        for bin in bin_of(start)..=bin_of(end) {
            let from = (bin * BIN_SECONDS * 1000).max(start.timestamp_millis());
            let to = ((bin + 1) * BIN_SECONDS * 1000).min(end.timestamp_millis());
            let share = if bin == bin_of(end) {
                left
            } else {
                (output as i128 * i128::from(to - from) / i128::from(span)) as u64
            };
            let share = share.min(left);
            left -= share;
            if share > 0 {
                let entry = self.bins.entry(bin).or_default();
                entry.output += share;
            }
        }
    }

    fn add(&mut self, at: DateTime<Utc>, input: u64, output: u64) {
        if input == 0 && output == 0 {
            return;
        }
        let bin = self.bins.entry(bin_of(at)).or_default();
        bin.input += input;
        bin.output += output;
    }
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[derive(Deserialize)]
struct ClaudeRecord {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    message: Option<ClaudeMessage>,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

impl ClaudeUsage {
    fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }
}

#[derive(Deserialize)]
struct CodexRecord {
    timestamp: String,
    #[serde(rename = "type")]
    kind: String,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    role: Option<String>,
    usage: Option<CodexUsage>,
    info: Option<CodexContext>,
    model_context_window: Option<u64>,
}

#[derive(Deserialize)]
struct CodexContext {
    last_token_usage: Option<CodexUsage>,
    model_context_window: Option<u64>,
}

#[derive(Deserialize)]
struct CodexUsage {
    total_tokens: Option<u64>,
    /// Every prompt token, cached ones included.
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn at(text: &str) -> DateTime<Utc> {
        parse_time(text).unwrap()
    }

    fn series(provider: ProviderId, lines: &[&str]) -> (tempfile::NamedTempFile, Series) {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        let mut series = Series::default();
        series.update(file.path(), provider).unwrap();
        (file, series)
    }

    // Shapes of real Claude Code 2.x records, content replaced: a tool result, then one model
    // response written as a thinking record and a tool_use record that repeat its usage.
    const CLAUDE_TOOL_RESULT: &str = r#"{"parentUuid":"a","isSidechain":false,"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":"ok"}]},"uuid":"b","timestamp":"2026-10-14T10:25:28.100Z","sessionId":"s"}"#;
    const CLAUDE_THINKING: &str = r#"{"parentUuid":"b","isSidechain":false,"message":{"model":"claude-opus-5-5","id":"msg_01EYBBw4","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"","signature":"x"}],"stop_reason":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":36336,"cache_read_input_tokens":14755,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":36336},"output_tokens":260,"service_tier":"standard"}},"type":"assistant","uuid":"c","timestamp":"2026-10-14T10:25:37.798Z","sessionId":"s"}"#;
    const CLAUDE_TOOL_USE: &str = r#"{"parentUuid":"c","isSidechain":false,"message":{"model":"claude-opus-5-5","id":"msg_01EYBBw4","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"ls"}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"cache_creation_input_tokens":36336,"cache_read_input_tokens":14755,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":36336},"output_tokens":260,"service_tier":"standard"}},"type":"assistant","uuid":"d","timestamp":"2026-10-14T10:25:37.804Z","sessionId":"s"}"#;

    #[test]
    fn context_usage_claude_is_unavailable() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
        let mut usage = Series::default();
        usage.update(&path, ProviderId::Claude).unwrap();
        assert_eq!(usage.context_percent(), None);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-main-model-history.jsonl");
        usage = Series::default();
        usage.update(&path, ProviderId::Claude).unwrap();
        assert_eq!(usage.context_percent(), None);
    }

    #[test]
    fn context_usage_codex_uses_last_request_not_cumulative_or_cached_tokens() {
        let (_file, usage) = series(ProviderId::Codex, &[CODEX_TOKEN_COUNT]);
        assert_eq!(usage.context_percent(), Some(29));
    }

    #[test]
    fn context_usage_codex_can_take_window_from_task_started() {
        let started = include_str!("../tests/fixtures/codex-rollout.jsonl")
            .lines()
            .find(|line| line.contains("task_started"))
            .unwrap();
        let mut count: serde_json::Value = serde_json::from_str(CODEX_TOKEN_COUNT).unwrap();
        count["payload"]["info"]
            .as_object_mut()
            .unwrap()
            .remove("model_context_window");
        let count = count.to_string();
        let (_file, usage) = series(ProviderId::Codex, &[started, &count]);
        assert_eq!(usage.context_percent(), Some(16));
    }

    #[test]
    fn context_usage_changes_without_chart_bins_and_resets_after_rewrite() {
        let (mut file, mut usage) = series(ProviderId::Codex, &[]);
        writeln!(file, "{CODEX_TOKEN_COUNT}").unwrap();
        assert!(usage.update(file.path(), ProviderId::Codex).unwrap());
        assert_eq!(usage.context_percent(), Some(29));
        assert!(!usage.update(file.path(), ProviderId::Codex).unwrap());
        std::fs::write(file.path(), "").unwrap();
        assert!(usage.update(file.path(), ProviderId::Codex).unwrap());
        assert_eq!(usage.context_percent(), None);
    }

    #[test]
    fn context_usage_requires_codex_usage_and_positive_window() {
        for provider in [ProviderId::Claude, ProviderId::Codex] {
            let (_file, usage) = series(provider, &[]);
            assert_eq!(usage.context_percent(), None);
        }
        for model in [Some("claude-opus-5-5"), Some("claude-opus-5-5[1m]"), None] {
            let mut message: serde_json::Value = serde_json::from_str(CLAUDE_THINKING).unwrap();
            message["message"]["model"] = model.into();
            let message = message.to_string();
            let (_file, usage) = series(ProviderId::Claude, &[&message]);
            assert_eq!(usage.context_percent(), None);
        }
        let mut count: serde_json::Value = serde_json::from_str(CODEX_TOKEN_COUNT).unwrap();
        count["payload"]["info"]["model_context_window"] = 0.into();
        let count = count.to_string();
        let (_file, usage) = series(ProviderId::Codex, &[&count]);
        assert_eq!(usage.context_percent(), None);
    }

    #[test]
    fn claude_counts_one_response_once_with_input_at_its_start() {
        let (_file, series) = series(
            ProviderId::Claude,
            &[CLAUDE_TOOL_RESULT, CLAUDE_THINKING, CLAUDE_TOOL_USE],
        );
        let start = bin_of(at("2026-10-14T10:25:28.100Z"));
        // The first request takes in its whole prompt.
        assert_eq!(series.bins[&start].input, 2 + 36336 + 14755);
        let output: u64 = series.bins.values().map(|bin| bin.output).sum();
        assert_eq!(output, 260);
        let input: u64 = series.bins.values().map(|bin| bin.input).sum();
        assert_eq!(input, 2 + 36336 + 14755);
        // 28.1s to 37.8s spans the 25s, 30s and 35s bins.
        assert_eq!(series.bins.len(), 3);
    }

    #[test]
    fn appended_records_are_read_from_where_the_last_update_stopped() {
        let (mut file, mut series) = series(ProviderId::Claude, &[CLAUDE_TOOL_RESULT]);
        assert!(series.bins.is_empty());
        write!(file, "{CLAUDE_THINKING}\n{CLAUDE_TOOL_USE}").unwrap();
        assert!(series.update(file.path(), ProviderId::Claude).unwrap());
        // The unterminated tool_use record waits for its newline.
        writeln!(file).unwrap();
        assert!(!series.update(file.path(), ProviderId::Claude).unwrap());
        let output: u64 = series.bins.values().map(|bin| bin.output).sum();
        assert_eq!(output, 260);
    }

    // Shapes of real codex-cli 0.162 rollout records, content replaced: a tool output, the
    // response's usage record, then the token_count event that repeats it later.
    const CODEX_TOOL_OUTPUT: &str = r#"{"timestamp":"2026-09-20T16:32:54.622Z","ordinal":2950,"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call_1","output":"ok"}}"#;
    const CODEX_USAGE: &str = r#"{"timestamp":"2026-09-20T16:32:57.845Z","ordinal":2955,"type":"token_usage_record","payload":{"thread_id":"t","turn_id":"u","session_id":"t","root_turn_id":"u","response_id":"resp_1","usage":{"input_tokens":74313,"cached_input_tokens":73856,"cache_write_input_tokens":0,"output_tokens":925,"reasoning_output_tokens":624,"total_tokens":75238},"turn_token_usage":{"input_tokens":74313,"cached_input_tokens":73856,"cache_write_input_tokens":0,"output_tokens":925,"reasoning_output_tokens":624,"total_tokens":75238},"thread_token_usage":{"input_tokens":41886053,"cached_input_tokens":41113216,"cache_write_input_tokens":0,"output_tokens":158733,"reasoning_output_tokens":66097,"total_tokens":42044786}}}"#;
    const CODEX_TOKEN_COUNT: &str = r#"{"timestamp":"2026-09-20T16:33:00.206Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":208055,"cached_input_tokens":153344,"cache_write_input_tokens":0,"output_tokens":522,"reasoning_output_tokens":195,"total_tokens":208577},"last_token_usage":{"input_tokens":74313,"cached_input_tokens":73856,"cache_write_input_tokens":0,"output_tokens":925,"reasoning_output_tokens":624,"total_tokens":75238},"model_context_window":258400}}}"#;

    #[test]
    fn codex_counts_usage_records_with_the_whole_first_prompt() {
        let (_file, series) = series(
            ProviderId::Codex,
            &[CODEX_TOOL_OUTPUT, CODEX_USAGE, CODEX_TOKEN_COUNT],
        );
        let start = bin_of(at("2026-09-20T16:32:54.622Z"));
        assert_eq!(series.bins[&start].input, 74313);
        let output: u64 = series.bins.values().map(|bin| bin.output).sum();
        assert_eq!(output, 925);
        // 54.6s to 57.8s spans the 50s and 55s bins, split by time.
        let end = bin_of(at("2026-09-20T16:32:57.845Z"));
        assert_eq!(series.bins[&start].output, 925 * 378 / 3223);
        assert_eq!(series.bins[&end].output, 925 - 925 * 378 / 3223);
    }

    #[test]
    fn a_shorter_transcript_is_read_again_from_the_start() {
        let (file, mut series) = series(ProviderId::Codex, &[CODEX_TOOL_OUTPUT, CODEX_USAGE]);
        std::fs::write(file.path(), format!("{CODEX_USAGE}\n")).unwrap();
        series.update(file.path(), ProviderId::Codex).unwrap();
        let input: u64 = series.bins.values().map(|bin| bin.input).sum();
        assert_eq!(input, 74313);
    }

    #[test]
    fn later_requests_take_in_what_the_prompt_gained_whatever_the_cache_held() {
        let next = |prompt: u64, at: &str| {
            CODEX_USAGE
                .replace("2026-09-20T16:32:57.845Z", at)
                .replacen(
                    "\"input_tokens\":74313",
                    &format!("\"input_tokens\":{prompt}"),
                    1,
                )
                // A cache miss rewrites the whole prompt; it is no new input.
                .replacen(
                    "\"cached_input_tokens\":73856",
                    "\"cached_input_tokens\":0",
                    1,
                )
        };
        let sent = |at: &str| CODEX_TOOL_OUTPUT.replace("2026-09-20T16:32:54.622Z", at);
        let records = [
            CODEX_USAGE.to_owned(),
            sent("2026-09-20T16:40:00.000Z"),
            next(80_000, "2026-09-20T16:40:03.000Z"),
            sent("2026-09-20T16:50:00.000Z"),
            next(20_000, "2026-09-20T16:50:03.000Z"),
        ];
        let records: Vec<&str> = records.iter().map(String::as_str).collect();
        let (_file, series) = series(ProviderId::Codex, &records);
        let input = |time: &str| {
            series
                .bins
                .get(&bin_of(at(time)))
                .map_or(0, |bin| bin.input)
        };
        assert_eq!(input("2026-09-20T16:40:00.000Z"), 80_000 - 74313 - 925);
        assert_eq!(input("2026-09-20T16:50:00.000Z"), 0);
    }
}
