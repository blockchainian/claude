use crate::model::PullRequest;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const CHUNK: u64 = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct Summary {
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub pr: Option<PullRequest>,
    pub prompt: String,
    pub ai_title: Option<String>,
    pub custom_title: Option<String>,
    pub last_prompt: Option<String>,
    pub last_message: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

impl Summary {
    fn record_pr(&mut self, record: &Value) {
        if record["type"] == "pr-link"
            && let Some(number) = record["prNumber"].as_u64()
            && let Some(url) = text(record, "prUrl")
        {
            self.pr = Some(PullRequest { number, url });
        }
    }
}

pub(super) fn preview(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(512)
        .collect()
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)?
        .as_str()
        .map(preview)
        .filter(|text| !text.trim().is_empty())
}

fn content_text(record: &Value, last: bool) -> Option<String> {
    let content = record.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let blocks = content.as_array()?;
    let mut texts = blocks
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block.get("text").and_then(Value::as_str));
    if last {
        texts.next_back().map(str::to_owned)
    } else {
        Some(texts.collect::<String>())
    }
}

fn records(bytes: &[u8]) -> impl Iterator<Item = Value> + '_ {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(|record| record["isSidechain"] != true)
}

pub(super) fn read(path: &Path, len: u64) -> io::Result<Summary> {
    let mut file = File::open(path)?;
    let mut head = Vec::new();
    let mut tail = Vec::new();
    if len <= 2 * CHUNK {
        (&mut file).take(2 * CHUNK).read_to_end(&mut head)?;
    } else {
        (&mut file).take(CHUNK).read_to_end(&mut head)?;
        // The preceding byte tells whether the tail begins on a complete line.
        file.seek(SeekFrom::Start(len - CHUNK - 1))?;
        (&mut file).take(CHUNK + 1).read_to_end(&mut tail)?;
        let skip = if tail.first() == Some(&b'\n') {
            1
        } else {
            tail.iter()
                .position(|byte| *byte == b'\n')
                .map_or(tail.len(), |index| index + 1)
        };
        tail.drain(..skip);
    }
    let mut summary = Summary::default();
    for record in records(&head) {
        summary.record_pr(&record);
        if !matches!(record["type"].as_str(), Some("user" | "assistant")) {
            continue;
        }
        if summary.cwd.is_none() {
            summary.cwd = record["cwd"]
                .as_str()
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from);
        }
        if summary.git_branch.is_none() {
            summary.git_branch = text(&record, "gitBranch");
        }
        if summary.prompt.is_empty()
            && record["type"] == "user"
            && let Some(prompt) = content_text(&record, false)
            && !prompt.trim().is_empty()
            && !prompt.trim_start().starts_with('<')
        {
            summary.prompt = preview(&prompt);
        }
    }
    let tail = if len <= 2 * CHUNK { &head } else { &tail };
    for record in records(tail) {
        summary.record_pr(&record);
        if let Some(timestamp) = record["timestamp"]
            .as_str()
            .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        {
            summary.timestamp = Some(timestamp.with_timezone(&Utc));
        }
        // A head chunk swallowed by one huge record still leaves these in the tail.
        if summary.cwd.is_none() {
            summary.cwd = record["cwd"]
                .as_str()
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from);
        }
        if matches!(record["type"].as_str(), Some("user" | "assistant")) {
            if summary.cwd.is_none() {
                summary.cwd = record["cwd"]
                    .as_str()
                    .filter(|cwd| !cwd.is_empty())
                    .map(PathBuf::from);
            }
            if summary.git_branch.is_none() {
                summary.git_branch = text(&record, "gitBranch");
            }
        }
        match record["type"].as_str() {
            Some("ai-title") => {
                if let Some(title) = text(&record, "aiTitle") {
                    summary.ai_title = Some(title);
                }
            }
            Some("custom-title") => {
                if let Some(title) = text(&record, "customTitle") {
                    summary.custom_title = Some(title);
                }
            }
            Some("last-prompt") => {
                if let Some(prompt) = text(&record, "lastPrompt") {
                    summary.last_prompt = Some(prompt);
                }
            }
            Some("assistant") => {
                if let Some(message) = content_text(&record, true) {
                    summary.last_message = Some(preview(&message));
                }
                if let Some(model) =
                    text(&record, "requestedModel").or_else(|| text(&record["message"], "model"))
                {
                    summary.model = Some(model);
                }
                if let Some(effort) = text(&record, "effort") {
                    summary.effort = Some(effort);
                }
            }
            _ => {}
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = include_str!("../../tests/fixtures/transcript.jsonl");

    fn assert_summary(summary: &Summary) {
        let records: Vec<Value> = FIXTURE
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(summary.ai_title.as_deref(), records[2]["aiTitle"].as_str());
        assert_eq!(
            summary.prompt,
            preview(records[0]["message"]["content"].as_str().unwrap())
        );
        assert_eq!(summary.cwd, records[0]["cwd"].as_str().map(PathBuf::from));
        assert_eq!(summary.git_branch.as_deref(), Some("main"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(summary.effort.as_deref(), Some("medium"));
        assert_eq!(
            summary.last_message,
            Some(preview(
                records[1]["message"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
            ))
        );
        assert_eq!(
            summary.last_prompt.as_deref(),
            records[3]["lastPrompt"].as_str()
        );
        assert_eq!(
            summary.timestamp.unwrap().to_rfc3339(),
            "2026-10-06T16:39:29.554+00:00"
        );
    }

    const PR: &str = include_str!("../../tests/fixtures/pr-link.jsonl");

    #[test]
    fn pull_request_absent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        assert!(
            read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap()
                .pr
                .is_none()
        );
    }

    #[test]
    fn pull_request_last_wins_in_head_and_tail() {
        let expected: Value = serde_json::from_str(PR).unwrap();
        for large in [false, true] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(FIXTURE.as_bytes()).unwrap();
            file.write_all(PR.as_bytes()).unwrap();
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            let pr = summary.pr.unwrap();
            assert_eq!(pr.number, expected["prNumber"].as_u64().unwrap());
            assert_eq!(pr.url, expected["prUrl"].as_str().unwrap());
            if large {
                for _ in 0..2000 {
                    writeln!(file, "{}", FIXTURE.lines().nth(2).unwrap()).unwrap();
                }
                assert!(
                    read(file.path(), file.as_file().metadata().unwrap().len())
                        .unwrap()
                        .pr
                        .is_some()
                );
            }
            let mut last = expected.clone();
            last["prNumber"] = 833.into();
            last["prUrl"] = "https://github.com/0xbabedead/chadwallet/pull/833".into();
            writeln!(file, "{last}").unwrap();
            let pr = read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap()
                .pr
                .unwrap();
            assert_eq!(pr.number, 833);
            assert_eq!(pr.url, last["prUrl"].as_str().unwrap());
        }
    }

    #[test]
    fn truncated_last_line_preserves_preceding_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        let assistant = FIXTURE.lines().nth(1).unwrap();
        file.write_all(&assistant.as_bytes()[..assistant.len() / 2])
            .unwrap();
        assert_summary(&read(file.path(), file.as_file().metadata().unwrap().len()).unwrap());
    }

    #[test]
    fn real_transcript_summary_and_large_head_tail() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        assert_summary(&read(file.path(), file.as_file().metadata().unwrap().len()).unwrap());
        let mut large = tempfile::NamedTempFile::new().unwrap();
        let lines: Vec<_> = FIXTURE.lines().collect();
        writeln!(large, "{}", lines[0]).unwrap();
        // Repeated real title records fill the middle without changing the summary.
        for _ in 0..2000 {
            writeln!(large, "{}", lines[2]).unwrap();
        }
        for line in &lines[1..] {
            writeln!(large, "{line}").unwrap();
        }
        let len = large.as_file().metadata().unwrap().len();
        assert!(len > 2 * CHUNK);
        assert_summary(&read(large.path(), len).unwrap());
    }

    #[test]
    fn cwd_and_branch_come_from_the_tail_when_a_huge_record_fills_the_head() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let lines: Vec<_> = FIXTURE.lines().collect();
        let huge = format!(
            r#"{{"type":"file-history-snapshot","snapshot":"{}"}}"#,
            "x".repeat(CHUNK as usize + 1)
        );
        writeln!(file, "{huge}").unwrap();
        for _ in 0..2000 {
            writeln!(file, "{}", lines[2]).unwrap();
        }
        for line in &lines[1..] {
            writeln!(file, "{line}").unwrap();
        }
        let len = file.as_file().metadata().unwrap().len();
        let summary = read(file.path(), len).unwrap();
        let records = FIXTURE
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let with_cwd = records
            .iter()
            .find(|record| record["cwd"].is_string())
            .unwrap();
        assert_eq!(
            summary.cwd.as_deref(),
            with_cwd["cwd"].as_str().map(Path::new)
        );
        assert_eq!(
            summary.git_branch.as_deref(),
            with_cwd["gitBranch"].as_str()
        );
    }

    #[test]
    fn previews_preserve_unicode_and_allowed_whitespace() {
        assert_eq!(preview("a\0b\n\t\u{7}c"), "ab\n\tc");
        assert_eq!(preview(&"中".repeat(600)).chars().count(), 512);
    }

    #[test]
    fn sidechains_are_skipped_and_latest_text_and_titles_win() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        let mut assistant: Value = serde_json::from_str(FIXTURE.lines().nth(1).unwrap()).unwrap();
        assistant["requestedModel"] = "claude-opus".into();
        let text_block = assistant["message"]["content"][0].clone();
        assistant["message"]["content"]
            .as_array_mut()
            .unwrap()
            .push(text_block);
        assistant["message"]["content"][1]["text"] = "latest\u{0} text".into();
        writeln!(file, "{assistant}").unwrap();
        assistant["isSidechain"] = true.into();
        assistant["message"]["content"][1]["text"] = "subagent text".into();
        writeln!(file, "{assistant}").unwrap();
        let mut title: Value = serde_json::from_str(FIXTURE.lines().nth(2).unwrap()).unwrap();
        title["type"] = "custom-title".into();
        title["customTitle"] = "User title".into();
        writeln!(file, "{title}").unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(summary.last_message.as_deref(), Some("latest text"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus"));
        assert_eq!(summary.custom_title.as_deref(), Some("User title"));
        assert_eq!(
            summary.ai_title.as_deref(),
            Some("Codex manager 停止时线程处理")
        );
    }
}
