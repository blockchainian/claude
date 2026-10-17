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
fn pending_ready(screen: &str, questions: usize) -> bool {
    headers(screen)
        .is_ok_and(|names| names.len() == questions && names.iter().all(|name| !name.is_empty()))
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
        terminal.wait_allowing(
            |screen| pending_ready(screen, source.len()),
            Some("Enter to select"),
        )?;
        let names = headers(&terminal.screen())?;
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
                                    && qs.iter().zip(questions).all(|(v, q)| {
                                        v["question"] == q.text
                                            && v["header"] == q.header
                                            && v["multiSelect"].as_bool() == Some(q.multi)
                                            && serde_json::from_value::<Vec<QuestionOption>>(
                                                v["options"].clone(),
                                            )
                                            .is_ok_and(|options| options == q.options)
                                    })
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum Delivery {
    Keys(Vec<u8>),
    Paste(String),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Select,
    OtherFocus,
    OtherPasted,
    OtherSubmitRow,
    Review,
    Done,
}
#[derive(Clone)]
struct AnswerFlow<'a> {
    ask: &'a Ask,
    answer: &'a Answer,
    expected: BTreeMap<String, String>,
    page: usize,
    choice: usize,
    selected: Vec<usize>,
    focus: Option<usize>,
    phase: Phase,
}
impl<'a> AnswerFlow<'a> {
    fn new(ask: &'a Ask, answer: &'a Answer) -> io::Result<Self> {
        Ok(Self {
            ask,
            answer,
            expected: raw_answers(ask, answer)?,
            page: 0,
            choice: 0,
            selected: Vec::new(),
            focus: None,
            phase: Phase::Select,
        })
    }
    fn advance_page(&mut self) {
        let AskKind::Questions { questions, .. } = &self.ask.kind;
        let review = questions.len() > 1 || questions[self.page].multi;
        self.page += 1;
        self.choice = 0;
        self.selected.clear();
        self.focus = None;
        self.phase = if self.page < questions.len() {
            Phase::Select
        } else if review {
            Phase::Review
        } else {
            Phase::Done
        };
    }
    fn next(&self, screen: &str) -> io::Result<(Self, Delivery)> {
        let mut next = self.clone();
        if self.phase == Phase::Review {
            if !screen.contains("Review your answers")
                || !screen
                    .lines()
                    .any(|line| line.trim() == "❯ 1. Submit answers")
                || !self.expected.iter().all(|(q, a)| {
                    normalized(screen).contains(&format!("● {} → {}", normalized(q), normalized(a)))
                })
            {
                return Err(mismatch());
            }
            next.phase = Phase::Done;
            return Ok((next, Delivery::Keys(vec![b'\r'])));
        }
        let AskKind::Questions { questions, .. } = &self.ask.kind;
        let q = questions.get(self.page).ok_or_else(mismatch)?;
        if parse_page(screen, &question_value(q))? != *q || checked(screen) != self.selected {
            return Err(mismatch());
        }
        let choices = &self.answer.answers[&q.id].answers;
        if self.phase == Phase::OtherSubmitRow {
            if !body(screen)?.lines().any(|line| {
                line.trim()
                    .strip_prefix('❯')
                    .is_some_and(|s| s.trim() == "Submit")
            }) {
                return Err(mismatch());
            }
            next.advance_page();
            return Ok((next, Delivery::Keys(vec![b'\r'])));
        }
        let focus = body(screen)?
            .lines()
            .filter_map(numbered)
            .find(|(_, f, _)| *f)
            .map(|(n, _, _)| n)
            .ok_or_else(mismatch)?;
        if self.focus.is_some_and(|expected| focus != expected) {
            return Err(mismatch());
        }
        let other = q.options.len() + 1;
        let editing_other = self.phase == Phase::Select
            && focus == other
            && choices
                .get(self.choice)
                .is_some_and(|choice| choice.starts_with("user_note: "));
        if matches!(self.phase, Phase::OtherFocus | Phase::OtherPasted) || editing_other {
            let note = choices[self.choice]
                .strip_prefix("user_note: ")
                .ok_or_else(mismatch)?;
            if focus != other {
                if self.phase != Phase::OtherFocus || focus > other {
                    return Err(mismatch());
                }
                next.focus = Some(other);
                return Ok((next, Delivery::Keys(b"\x1b[B".repeat(other - focus))));
            }
            if self.phase == Phase::OtherFocus || editing_other {
                next.phase = Phase::OtherPasted;
                next.focus = Some(other);
                return Ok((next, Delivery::Paste(note.to_owned())));
            }
            let page = body(screen)?;
            let lines: Vec<_> = page.lines().collect();
            let row = lines
                .iter()
                .position(|line| numbered(line).is_some_and(|(n, f, _)| n == other && f))
                .ok_or_else(mismatch)?;
            let first = numbered(lines[row])
                .unwrap()
                .2
                .trim_start_matches("[✔] ")
                .trim_start_matches("[ ] ");
            let tail = lines
                .iter()
                .skip(row + 1)
                .take_while(|line| !line.trim().starts_with('─') && line.trim() != "Submit")
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            if normalized(&format!("{first} {tail}")) != normalized(note) {
                return Err(mismatch());
            }
            if q.multi {
                next.phase = Phase::OtherSubmitRow;
                next.focus = None;
            } else {
                next.advance_page();
            }
            return Ok((
                next,
                Delivery::Keys(if q.multi { vec![b'\t'] } else { vec![b'\r'] }),
            ));
        }
        if !focused_option(screen, q) {
            return Err(mismatch());
        }
        if self.choice == choices.len() {
            if !q.multi {
                return Err(mismatch());
            }
            next.advance_page();
            return Ok((next, Delivery::Keys(vec![b'\t'])));
        }
        let choice = &choices[self.choice];
        let n = if choice.starts_with("user_note: ") {
            if self.choice + 1 != choices.len() {
                return Err(mismatch());
            }
            next.phase = Phase::OtherFocus;
            next.focus = Some(if q.multi { focus } else { q.options.len() + 1 });
            q.options.len() + 1
        } else {
            q.options
                .iter()
                .position(|o| &o.label == choice)
                .ok_or_else(mismatch)?
                + 1
        };
        if n > 9 {
            return Err(mismatch());
        }
        if q.multi {
            next.selected.push(n);
            next.selected.sort_unstable();
        }
        if next.phase == Phase::Select {
            if q.multi {
                next.choice += 1;
            } else {
                next.advance_page();
            }
        }
        Ok((next, Delivery::Keys(vec![b'0' + n as u8])))
    }
}

impl Terminal {
    pub(super) fn answer_questions(
        &mut self,
        ask: &Ask,
        answer: &Answer,
        transcript: &Path,
    ) -> io::Result<()> {
        let expected = raw_answers(ask, answer)?;
        let mut confirmation = File::open(transcript)?;
        let mut offset = crate::jsonl::complete_end(&mut confirmation)?;
        let mut flow = AnswerFlow::new(ask, answer)?;
        #[cfg(test)]
        let mut capture_index = 0;
        while flow.phase != Phase::Done {
            self.wait_allowing(|screen| flow.next(screen).is_ok(), Some("Enter to select"))?;
            let (next, delivery) = flow.next(&self.screen())?;
            #[cfg(test)]
            if let Some(directory) = std::env::var_os("AGENTS_ASK_CAPTURE") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory)?;
                let AskKind::Questions { questions, .. } = &ask.kind;
                std::fs::write(
                    directory.join(format!(
                        "{}-{capture_index:02}.screen.txt",
                        questions[0].header
                    )),
                    self.screen(),
                )?;
                capture_index += 1;
            }
            match delivery {
                Delivery::Keys(keys) => self.write(&keys)?,
                Delivery::Paste(text) => self.paste(&text)?,
            }
            flow = next;
        }
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let mut result = Ok(false);
            offset = crate::jsonl::read_lines(&mut confirmation, offset, |line| {
                if !matches!(result, Ok(false)) {
                    return;
                }
                result = std::str::from_utf8(line)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
                    .and_then(|line| verify_result(&format!("{line}\n"), ask, &expected));
            })?;
            if result? {
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
    fn fixture_ask(capture: &str, text: &str) -> (Ask, BTreeMap<String, String>) {
        let event: Value = capture
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
    fn recorded_cold_other_focus_accepts_an_answer() {
        let mut parser = OutputScreen::new();
        parser.process(include_bytes!(
            "../../../tests/fixtures/claude-attach/ask/cold-start/other-focused.raw"
        ));
        let screen = parser.complete().unwrap();
        let state: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-attach/ask/cold-start/pending-state.json"
        ))
        .unwrap();
        let mut question = state["block"]["questions"][0].clone();
        question["header"] = json!(headers(&screen).unwrap()[0]);
        let ask = Ask {
            id: "screen:recorded-cold".into(),
            session: format!("claude:{}", state["sessionId"].as_str().unwrap())
                .parse()
                .unwrap(),
            since: Utc::now(),
            kind: AskKind::Questions {
                blocking: true,
                questions: vec![parse_page(&screen, &question).unwrap()],
            },
        };
        let answer = Answer {
            answers: BTreeMap::from([(
                "Cold Other?".into(),
                crate::ask::QuestionAnswer {
                    answers: vec!["user_note: hey".into()],
                },
            )]),
        };
        let (next, delivery) = AnswerFlow::new(&ask, &answer)
            .unwrap()
            .next(&screen)
            .unwrap();
        assert_eq!(delivery, Delivery::Paste("hey".into()));
        assert_eq!(next.phase, Phase::OtherPasted);
    }

    #[test]
    fn recorded_unicode_other_checks_text_before_enter() {
        let capture =
            include_str!("../../../tests/fixtures/claude-attach/ask/unicode/transcript.jsonl");
        let (ask, expected) = fixture_ask(capture, "Probe single choice?");
        let answer = Answer {
            answers: BTreeMap::from([(
                "Probe single choice?".into(),
                crate::ask::QuestionAnswer {
                    answers: vec!["user_note: 嗨".into()],
                },
            )]),
        };
        let raw = include_bytes!("../../../tests/fixtures/claude-attach/ask/unicode/single-hi.raw");
        for size in [1, 7, 37, 512] {
            let mut parser = OutputScreen::new();
            let mut flow = AnswerFlow::new(&ask, &answer).unwrap();
            let mut deliveries = Vec::new();
            for chunk in raw.chunks(size) {
                parser.process(chunk);
                while flow.phase != Phase::Done
                    && let Some(screen) = parser.complete()
                    && let Ok((next, delivery)) = flow.next(&screen)
                {
                    deliveries.push(delivery);
                    flow = next;
                }
            }
            assert_eq!(
                deliveries,
                [
                    Delivery::Keys(b"3".to_vec()),
                    Delivery::Paste("嗨".into()),
                    Delivery::Keys(b"\r".to_vec()),
                ],
                "{size}-byte chunks; {}",
                parser.contents(),
            );
            assert_eq!(flow.phase, Phase::Done);
        }
        assert_eq!(expected["Probe single choice?"], "嗨");
        assert!(verify_result(capture, &ask, &expected).unwrap());
    }

    #[test]
    fn recorded_two_question_attach_waits_for_all_headers() {
        let raw = include_bytes!(
            "../../../tests/fixtures/claude-attach/ask/other-submit/two-initial.raw"
        );
        let mut parser = OutputScreen::new();
        let mut saw_ready = false;
        for byte in raw {
            parser.process(&[*byte]);
            let Some(screen) = parser.complete() else {
                continue;
            };
            if pending_ready(&screen, 2) {
                assert_eq!(headers(&screen).unwrap(), ["First", "Second"], "{screen}");
                saw_ready = true;
            }
        }
        assert!(saw_ready);
    }

    #[test]
    fn recorded_results_confirm_single_multi_two_and_other() {
        for text in [
            "Probe single choice?",
            "Probe multiple choices?",
            "Probe first page?",
            "Probe custom text?",
        ] {
            let (ask, expected) = fixture_ask(CAPTURE, text);
            assert!(verify_result(CAPTURE, &ask, &expected).unwrap(), "{text}");
            let mut wrong = expected.clone();
            wrong.insert(text.to_owned(), "different answer".into());
            assert!(verify_result(CAPTURE, &ask, &wrong).is_err());
        }
    }
    #[test]
    fn transcript_verification_waits_for_complete_jsonl_record() {
        let (ask, expected) = fixture_ask(CAPTURE, "Probe single choice?");
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
    fn recorded_multi_other_focus_moves_in_one_delivery() {
        let capture =
            include_str!("../../../tests/fixtures/claude-attach/ask/production/transcript.jsonl");
        let (ask, _) = fixture_ask(capture, "Transport multi Other?");
        let answer = Answer {
            answers: BTreeMap::from([(
                "Transport multi Other?".into(),
                crate::ask::QuestionAnswer {
                    answers: vec!["Red".into(), "user_note: custom multi note".into()],
                },
            )]),
        };
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-attach/ask/production");
        let mut flow = AnswerFlow::new(&ask, &answer).unwrap();
        for index in 0..2 {
            let screen = std::fs::read_to_string(
                directory.join(format!("MultiOther-{index:02}.screen.txt")),
            )
            .unwrap();
            flow = flow.next(&screen).unwrap().0;
        }
        let screen = std::fs::read_to_string(directory.join("MultiOther-02.screen.txt")).unwrap();
        let (next, delivery) = flow.next(&screen).unwrap();
        assert_eq!(delivery, Delivery::Keys(b"\x1b[B".repeat(3)));
        let screen = std::fs::read_to_string(directory.join("MultiOther-05.screen.txt")).unwrap();
        assert_eq!(
            next.next(&screen).unwrap().1,
            Delivery::Paste("custom multi note".into())
        );
    }

    #[test]
    fn recorded_screens_replay_production_delivery_flow() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-attach/ask/production");
        let capture =
            include_str!("../../../tests/fixtures/claude-attach/ask/production/transcript.jsonl");
        let keys = |values: &[&[u8]]| {
            values
                .iter()
                .map(|value| Delivery::Keys(value.to_vec()))
                .collect::<Vec<_>>()
        };
        for (header, text, values, expected) in [
            (
                "Single",
                "Transport single?",
                vec![vec!["Green"]],
                keys(&[b"2"]),
            ),
            (
                "Multi",
                "Transport multi?",
                vec![vec!["Blue", "Red"]],
                keys(&[b"3", b"1", b"\t", b"\r"]),
            ),
            (
                "First",
                "Transport first?",
                vec![vec!["Green"], vec!["Green"]],
                keys(&[b"2", b"2", b"\r"]),
            ),
            (
                "Other",
                "Transport other?",
                vec![vec!["user_note: custom transport note"]],
                vec![
                    Delivery::Keys(b"4".to_vec()),
                    Delivery::Paste("custom transport note".into()),
                    Delivery::Keys(b"\r".to_vec()),
                ],
            ),
            (
                "MultiOther",
                "Transport multi Other?",
                vec![vec!["Red", "user_note: custom multi note"]],
                vec![
                    Delivery::Keys(b"1".to_vec()),
                    Delivery::Keys(b"4".to_vec()),
                    Delivery::Keys(b"\x1b[B".repeat(3)),
                    Delivery::Paste("custom multi note".into()),
                    Delivery::Keys(b"\t".to_vec()),
                    Delivery::Keys(b"\r".to_vec()),
                    Delivery::Keys(b"\r".to_vec()),
                ],
            ),
        ] {
            let (ask, result) = fixture_ask(capture, text);
            let AskKind::Questions { questions, .. } = &ask.kind;
            let answer = Answer {
                answers: questions
                    .iter()
                    .zip(values)
                    .map(|(q, answers)| {
                        (
                            q.id.clone(),
                            crate::ask::QuestionAnswer {
                                answers: answers.into_iter().map(str::to_owned).collect(),
                            },
                        )
                    })
                    .collect(),
            };
            let mut flow = AnswerFlow::new(&ask, &answer).unwrap();
            let captures: Vec<usize> = if header == "MultiOther" {
                vec![0, 1, 2, 5, 6, 7, 8]
            } else {
                (0..expected.len()).collect()
            };
            for (index, expected) in captures.into_iter().zip(expected) {
                let screen = std::fs::read_to_string(
                    directory.join(format!("{header}-{index:02}.screen.txt")),
                )
                .unwrap();
                let (next, delivery) = flow
                    .next(&screen)
                    .unwrap_or_else(|error| panic!("{header} step {index}: {error}"));
                assert_eq!(delivery, expected, "{header} step {index}");
                assert!(flow.next("different screen").is_err());
                flow = next;
            }
            assert_eq!(flow.phase, Phase::Done, "{header}");
            assert_eq!(raw_answers(&ask, &answer).unwrap(), result);
            assert!(verify_result(capture, &ask, &result).unwrap());
        }
    }
    #[test]
    fn recorded_result_rejects_stale_epoch_and_changed_question_signature() {
        let (mut ask, expected) = fixture_ask(CAPTURE, "Probe single choice?");
        ask.id = "screen:recorded:1".into();
        assert!(verify_result(CAPTURE, &ask, &expected).unwrap());
        ask.since = "2026-10-18T00:00:00Z".parse().unwrap();
        assert!(!verify_result(CAPTURE, &ask, &expected).unwrap());
        ask.since = "2026-10-17T04:00:00Z".parse().unwrap();
        let AskKind::Questions { questions, .. } = &mut ask.kind;
        questions[0].header = "Different".into();
        assert!(!verify_result(CAPTURE, &ask, &expected).unwrap());
    }
    #[test]
    fn recorded_rejected_result_is_an_error() {
        let event: Value = CAPTURE
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|v| {
                v["message"]["content"].as_array().is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item["type"] == "tool_result" && item["is_error"] == true)
                })
            })
            .unwrap();
        let (mut ask, expected) = fixture_ask(CAPTURE, "Probe single choice?");
        ask.id = event["message"]["content"][0]["tool_use_id"]
            .as_str()
            .unwrap()
            .into();
        assert!(verify_result(CAPTURE, &ask, &expected).is_err());
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
