use super::*;
use crate::ask::{Answer, Ask, AskKind, Question, QuestionOption};
use crate::providers::SessionKey;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn mismatch() -> io::Error {
    io::Error::other("Claude question changed; no further keys sent")
}
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn body(screen: &str) -> io::Result<&str> {
    let start = screen
        .lines()
        .scan(0, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .filter(|(_, line)| line.contains('☐') || line.contains('☒'))
        .map(|(at, _)| at)
        .last()
        .ok_or_else(mismatch)?;
    Ok(&screen[start..])
}
fn headers(screen: &str) -> io::Result<Vec<String>> {
    let line = body(screen)?.lines().next().ok_or_else(mismatch)?;
    Ok(line
        .split(['☐', '☒'])
        .skip(1)
        .map(|s| {
            s.split('✔')
                .next()
                .unwrap_or(s)
                .trim()
                .trim_end_matches('→')
                .trim()
                .to_owned()
        })
        .collect())
}
fn numbered(line: &str) -> Option<(usize, bool, &str)> {
    let trimmed = line.trim();
    let focused = trimmed.starts_with('❯');
    let text = trimmed.strip_prefix('❯').unwrap_or(trimmed).trim();
    let (n, label) = text.split_once(". ")?;
    Some((n.parse().ok()?, focused, label))
}
fn parse_page(screen: &str, question: &Value) -> io::Result<Question> {
    let text = question["question"].as_str().ok_or_else(mismatch)?;
    let options: Vec<QuestionOption> =
        serde_json::from_value(question["options"].clone()).map_err(|_| mismatch())?;
    let page = body(screen)?;
    let lines: Vec<_> = page.lines().collect();
    let first = lines
        .iter()
        .position(|line| numbered(line).is_some())
        .ok_or_else(mismatch)?;
    if normalized(&lines[1..first].join(" ")) != normalized(text) {
        return Err(mismatch());
    }
    let mut multi = None;
    for (index, option) in options.iter().enumerate() {
        let row = lines
            .iter()
            .position(|line| numbered(line).is_some_and(|(n, _, _)| n == index + 1))
            .ok_or_else(mismatch)?;
        let (_, _, value) = numbered(lines[row]).ok_or_else(mismatch)?;
        let is_multi = value.starts_with("[ ] ") || value.starts_with("[✔] ");
        if multi.is_some_and(|m| m != is_multi) {
            return Err(mismatch());
        }
        multi = Some(is_multi);
        let label = if is_multi {
            value
                .strip_prefix("[✔] ")
                .or_else(|| value.strip_prefix("[ ] "))
                .ok_or_else(mismatch)?
        } else {
            value
        };
        if label != option.label {
            return Err(mismatch());
        }
        let end = lines
            .iter()
            .enumerate()
            .skip(row + 1)
            .find(|(_, line)| numbered(line).is_some())
            .map(|(at, _)| at)
            .ok_or_else(mismatch)?;
        if !option.description.is_empty()
            && normalized(&lines[row + 1..end].join(" ")) != normalized(&option.description)
        {
            return Err(mismatch());
        }
    }
    let header = question["header"].as_str().ok_or_else(mismatch)?.to_owned();
    if !headers(screen)?.contains(&header) {
        return Err(mismatch());
    }
    let other = lines.iter().any(|line| {
        numbered(line).is_some_and(|(n, _, label)| n == options.len() + 1 && !label.is_empty())
    });
    if !other {
        return Err(mismatch());
    }
    Ok(Question {
        id: text.to_owned(),
        text: text.to_owned(),
        header,
        options,
        multi: multi.ok_or_else(mismatch)?,
        other: true,
        secret: false,
    })
}

pub(in crate::providers::claude) fn pending(
    job_id: &str,
    cwd: &Path,
    session: SessionKey,
    since: DateTime<Utc>,
    block: &Value,
) -> io::Result<Ask> {
    let source = block["questions"]
        .as_array()
        .filter(|q| !q.is_empty())
        .ok_or_else(mismatch)?;
    let mut terminal = Terminal::spawn(job_id, cwd)?;
    let result = (|| {
        terminal.wait_allowing(|screen| body(screen).is_ok(), Some("Enter to select"))?;
        let names = headers(&terminal.screen())?;
        if names.len() != source.len() {
            return Err(mismatch());
        }
        let mut questions = Vec::new();
        // Read each page without committing an answer. Tab only navigates; never enter review.
        for (index, value) in source.iter().enumerate() {
            let mut value = value.clone();
            value["header"] = json!(names[index]);
            terminal.wait_allowing(
                |screen| parse_page(screen, &value).is_ok(),
                Some("Enter to select"),
            )?;
            questions.push(parse_page(&terminal.screen(), &value)?);
            if index + 1 < source.len() {
                terminal.write(b"\t")?;
            }
        }
        for index in (0..source.len().saturating_sub(1)).rev() {
            terminal.write(b"\x1b[Z")?;
            let value = question_value(&questions[index]);
            terminal.wait_allowing(
                |screen| parse_page(screen, &value).is_ok(),
                Some("Enter to select"),
            )?;
        }
        Ok(Ask {
            id: format!("screen:{job_id}:{}", since.timestamp_millis()),
            kind: AskKind::Questions {
                questions,
                blocking: true,
            },
            session,
            since,
        })
    })();
    let detached = terminal.detach();
    result.and_then(|ask| detached.map(|()| ask))
}

fn question_value(question: &Question) -> Value {
    json!({"question":question.text,"header":question.header,"options":question.options.iter().map(|o| json!({"label":o.label,"description":o.description,"preview":o.preview})).collect::<Vec<_>>()})
}
fn raw_answers(ask: &Ask, answer: &Answer) -> io::Result<BTreeMap<String, String>> {
    let AskKind::Questions { questions, .. } = &ask.kind;
    if questions.len() != answer.answers.len() {
        return Err(mismatch());
    }
    let mut result = BTreeMap::new();
    for q in questions {
        let values = &answer.answers.get(&q.id).ok_or_else(mismatch)?.answers;
        if values.is_empty() || (!q.multi && values.len() != 1) {
            return Err(mismatch());
        }
        let mut raw = Vec::new();
        for value in values {
            let text = if let Some(note) = value.strip_prefix("user_note: ") {
                if !q.other || note.is_empty() {
                    return Err(mismatch());
                }
                note
            } else {
                if !q.options.iter().any(|o| &o.label == value) {
                    return Err(mismatch());
                }
                value
            };
            validate_text(text)?;
            raw.push(text.to_owned());
        }
        result.insert(q.text.clone(), raw.join(", "));
    }
    Ok(result)
}
fn focused_option(screen: &str, question: &Question) -> bool {
    body(screen).is_ok_and(|page| {
        page.lines().any(|line| {
            numbered(line).is_some_and(|(n, focus, _)| focus && n <= question.options.len())
        })
    })
}
fn checked(screen: &str) -> Vec<usize> {
    body(screen)
        .map(|page| {
            page.lines()
                .filter_map(numbered)
                .filter(|(_, _, s)| s.starts_with("[✔]"))
                .map(|(n, _, _)| n)
                .collect()
        })
        .unwrap_or_default()
}
fn verify_result(text: &str, ask: &Ask, expected: &BTreeMap<String, String>) -> io::Result<bool> {
    let AskKind::Questions { questions, .. } = &ask.kind;
    let complete = text
        .rsplit_once('\n')
        .map(|(complete, _)| complete)
        .unwrap_or("");
    for line in complete.lines() {
        let value: Value = serde_json::from_str(line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let content = value["message"]["content"].as_array();
        let Some(content) = content else { continue };
        let matching = content.iter().any(|item| {
            item["type"] == "tool_result"
                && (item["tool_use_id"] == ask.id
                    || (ask.id.starts_with("screen:")
                        && value["timestamp"]
                            .as_str()
                            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                            .is_some_and(|time| time >= ask.since)
                        && value["toolUseResult"]["questions"]
                            .as_array()
                            .is_some_and(|qs| {
                                qs.len() == questions.len()
                                    && qs
                                        .iter()
                                        .zip(questions)
                                        .all(|(v, q)| v["question"] == q.text)
                            })))
        });
        if !matching {
            continue;
        }
        if content
            .iter()
            .any(|item| item["type"] == "tool_result" && item["is_error"] == true)
        {
            return Err(io::Error::other("Claude rejected the answer"));
        }
        let actual: BTreeMap<String, String> =
            serde_json::from_value(value["toolUseResult"]["answers"].clone())
                .map_err(|_| mismatch())?;
        return if &actual == expected {
            Ok(true)
        } else {
            Err(io::Error::other(
                "Claude transcript answer differs from the submitted answer",
            ))
        };
    }
    Ok(false)
}

impl Terminal {
    pub(super) fn answer_questions(
        &mut self,
        ask: &Ask,
        answer: &Answer,
        transcript: &Path,
    ) -> io::Result<()> {
        let expected = raw_answers(ask, answer)?;
        let AskKind::Questions { questions, .. } = &ask.kind;
        for (index, q) in questions.iter().enumerate() {
            let value = question_value(q);
            self.wait_allowing(
                |screen| parse_page(screen, &value).is_ok(),
                Some("Enter to select"),
            )?;
            let current = self.screen();
            if !focused_option(&current, q) || !checked(&current).is_empty() {
                return Err(mismatch());
            }
            let mut selected = Vec::new();
            for choice in &answer.answers[&q.id].answers {
                // Re-read the PTY immediately before each step to catch another client's changes.
                self.read(Duration::from_millis(25))?;
                let screen = self.screen();
                if parse_page(&screen, &value)? != *q
                    || !focused_option(&screen, q)
                    || checked(&screen) != selected
                {
                    return Err(mismatch());
                }
                if let Some(note) = choice.strip_prefix("user_note: ") {
                    if q.multi {
                        return Err(io::Error::other(
                            "Claude multi-select Other is not verified",
                        ));
                    }
                    self.write((q.options.len() + 1).to_string().as_bytes())?;
                    self.wait_allowing(
                        |screen| {
                            body(screen).is_ok_and(|page| {
                                page.lines().any(|line| {
                                    numbered(line)
                                        .is_some_and(|(n, f, _)| f && n == q.options.len() + 1)
                                })
                            })
                        },
                        Some("Enter to select"),
                    )?;
                    self.read(Duration::from_millis(25))?;
                    if parse_page(&self.screen(), &value)? != *q {
                        return Err(mismatch());
                    }
                    self.paste(note)?;
                    self.wait_allowing(
                        |screen| {
                            body(screen).is_ok_and(|page| {
                                page.lines().any(|line| {
                                    numbered(line).is_some_and(|(n, f, s)| {
                                        f && n == q.options.len() + 1 && s == note
                                    })
                                })
                            })
                        },
                        Some("Enter to select"),
                    )?;
                    self.read(Duration::from_millis(25))?;
                    let screen = self.screen();
                    if parse_page(&screen, &value)? != *q
                        || !body(&screen)?.lines().any(|line| {
                            numbered(line)
                                .is_some_and(|(n, f, s)| f && n == q.options.len() + 1 && s == note)
                        })
                    {
                        return Err(mismatch());
                    }
                    self.write(b"\r")?;
                } else {
                    let n = q
                        .options
                        .iter()
                        .position(|o| o.label == *choice)
                        .ok_or_else(mismatch)?
                        + 1;
                    if n > 9 {
                        return Err(mismatch());
                    }
                    self.write(n.to_string().as_bytes())?;
                    if q.multi {
                        selected.push(n);
                        selected.sort_unstable();
                        self.wait_allowing(
                            |screen| {
                                parse_page(screen, &value).is_ok() && checked(screen) == selected
                            },
                            Some("Enter to select"),
                        )?;
                    }
                }
            }
            if q.multi {
                self.read(Duration::from_millis(25))?;
                if parse_page(&self.screen(), &value)? != *q || checked(&self.screen()) != selected
                {
                    return Err(mismatch());
                }
                self.write(b"\t")?;
            }
            if index + 1 == questions.len() && (questions.len() > 1 || q.multi) {
                self.wait_allowing(
                    |screen| screen.contains("Review your answers"),
                    Some("Enter to select"),
                )?;
                self.read(Duration::from_millis(25))?;
                let screen = self.screen();
                if !screen
                    .lines()
                    .any(|line| line.trim() == "❯ 1. Submit answers")
                    || !expected.iter().all(|(q, a)| {
                        normalized(&screen).contains(&format!(
                            "● {} → {}",
                            normalized(q),
                            normalized(a)
                        ))
                    })
                {
                    return Err(mismatch());
                }
                self.write(b"\r")?;
            }
        }
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let text = std::fs::read_to_string(transcript)?;
            if verify_result(&text, ask, &expected)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "Claude answer sent but transcript confirmation timed out",
                ));
            }
            self.read(Duration::from_millis(100))?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CAPTURE: &str =
        include_str!("../../../tests/fixtures/claude-attach/ask/transcript.jsonl");
    fn question(text: &str) -> Value {
        CAPTURE
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .flat_map(|value| {
                value["toolUseResult"]["questions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .find(|q| q["question"] == text)
            .unwrap()
    }
    fn fixture_ask(text: &str) -> (Ask, BTreeMap<String, String>) {
        let event: Value = CAPTURE
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|event| {
                event["toolUseResult"]["answers"]
                    .get(text)
                    .is_some_and(|a| a.as_str().is_some_and(|s| !s.is_empty()))
            })
            .unwrap();
        let questions = event["toolUseResult"]["questions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| Question {
                id: q["question"].as_str().unwrap().to_owned(),
                text: q["question"].as_str().unwrap().to_owned(),
                header: q["header"].as_str().unwrap().to_owned(),
                multi: q["multiSelect"].as_bool().unwrap(),
                other: true,
                secret: false,
                options: serde_json::from_value(q["options"].clone()).unwrap(),
            })
            .collect();
        let ask = Ask {
            id: event["message"]["content"][0]["tool_use_id"]
                .as_str()
                .unwrap()
                .to_owned(),
            kind: AskKind::Questions {
                questions,
                blocking: true,
            },
            session: "claude:recorded".parse().unwrap(),
            since: "2026-10-17T04:00:00Z".parse().unwrap(),
        };
        (
            ask,
            serde_json::from_value(event["toolUseResult"]["answers"].clone()).unwrap(),
        )
    }
    #[test]
    fn recorded_results_confirm_single_multi_two_and_other() {
        for text in [
            "Probe single choice?",
            "Probe multiple choices?",
            "Probe first page?",
            "Probe custom text?",
        ] {
            let (ask, expected) = fixture_ask(text);
            assert!(verify_result(CAPTURE, &ask, &expected).unwrap(), "{text}");
            let mut wrong = expected.clone();
            wrong.insert(text.to_owned(), "different answer".into());
            assert!(verify_result(CAPTURE, &ask, &wrong).is_err());
        }
    }
    #[test]
    fn transcript_verification_waits_for_complete_jsonl_record() {
        let (ask, expected) = fixture_ask("Probe single choice?");
        assert!(!verify_result("{\"type\":", &ask, &expected).unwrap());
    }
    #[test]
    fn recorded_multi_checkbox_transitions_are_observed() {
        for (screen, expected) in [
            (
                include_str!(
                    "../../../tests/fixtures/claude-attach/ask/13-multi-initial.screen.txt"
                ),
                vec![],
            ),
            (
                include_str!("../../../tests/fixtures/claude-attach/ask/14-multi-digit.screen.txt"),
                vec![2],
            ),
            (
                include_str!(
                    "../../../tests/fixtures/claude-attach/ask/16-multi-untoggle.screen.txt"
                ),
                vec![2],
            ),
        ] {
            let q = parse_page(screen, &question("Probe multiple choices?")).unwrap();
            assert!(q.multi);
            assert!(focused_option(screen, &q));
            assert_eq!(checked(screen), expected);
        }
    }

    #[test]
    fn recorded_question_pages_recover_exact_fields() {
        for (screen, text, multi, header) in [
            (
                include_str!(
                    "../../../tests/fixtures/claude-attach/ask/01-single-attach.screen.txt"
                ),
                "Probe single choice?",
                false,
                "Single",
            ),
            (
                include_str!(
                    "../../../tests/fixtures/claude-attach/ask/13-multi-initial.screen.txt"
                ),
                "Probe multiple choices?",
                true,
                "Multi",
            ),
            (
                include_str!(
                    "../../../tests/fixtures/claude-attach/ask/28-two-digit-advance.screen.txt"
                ),
                "Probe second page?",
                false,
                "Second",
            ),
        ] {
            let parsed = parse_page(screen, &question(text)).unwrap();
            assert_eq!(parsed.multi, multi);
            assert_eq!(parsed.header, header);
            assert_eq!(parsed.options[1].label, "Green");
        }
    }
    #[test]
    fn recorded_other_paste_preserves_the_question() {
        let screen =
            include_str!("../../../tests/fixtures/claude-attach/ask/32-custom-paste.screen.txt");
        let q = question("Probe custom text?");
        let parsed = parse_page(screen, &q).unwrap();
        assert_eq!(parsed.text, "Probe custom text?");
        assert!(parsed.other);
    }

    #[test]
    fn recorded_question_rejects_reordered_or_changed_options() {
        let screen =
            include_str!("../../../tests/fixtures/claude-attach/ask/01-single-attach.screen.txt");
        let mut q = question("Probe single choice?");
        q["options"].as_array_mut().unwrap().swap(0, 1);
        assert!(parse_page(screen, &q).is_err());
    }
}

#[cfg(test)]
#[path = "ask_live_tests.rs"]
mod ask_live_tests;
