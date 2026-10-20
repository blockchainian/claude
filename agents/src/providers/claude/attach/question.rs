//! Answers Claude's native question pages after the HTTP hook disconnects.
use super::*;
use crate::ask::{Answer, Ask, AskKind, Question};

fn compact(text: &str) -> String {
    text.split_whitespace().collect()
}

fn review(screen: &str) -> bool {
    screen
        .lines()
        .any(|line| line.trim() == "Review your answers")
        && screen.contains("Submit answers")
}

fn page(screen: &str, question: &Question) -> bool {
    let lines: Vec<_> = screen.lines().collect();
    let Some(footer) = lines
        .iter()
        .position(|line| line.contains("Enter to select"))
    else {
        return false;
    };
    let borders: Vec<_> = lines[..footer]
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with("────"))
        .map(|(index, _)| index)
        .collect();
    let Some(start) = borders.iter().rev().nth(1) else {
        return false;
    };
    let body = compact(&lines[start + 1..footer].join("\n"));
    body.contains(&compact(&question.text))
        && question.options.iter().enumerate().all(|(index, option)| {
            body.contains(&format!("{}.", index + 1)) && body.contains(&compact(&option.label))
        })
}

fn checked(screen: &str, index: usize) -> Option<bool> {
    let prefix = format!("{}.", index + 1);
    screen.lines().find_map(|line| {
        let line = line.trim().trim_start_matches('❯').trim();
        let rest = line.strip_prefix(&prefix)?.trim_start();
        if rest.starts_with("[✔]") {
            Some(true)
        } else if rest.starts_with("[ ]") {
            Some(false)
        } else {
            None
        }
    })
}

fn focused(screen: &str, question: &Question) -> Option<usize> {
    screen
        .lines()
        .filter_map(|line| line.trim().strip_prefix('❯'))
        .find_map(|line| {
            let line = line.trim();
            if matches!(line, "Next" | "Submit") {
                return Some(question.options.len() + 1);
            }
            line.split_once('.')?
                .0
                .parse::<usize>()
                .ok()?
                .checked_sub(1)
        })
}

impl Terminal {
    fn question_focus(&mut self, question: &Question, target: usize) -> io::Result<()> {
        loop {
            let current = focused(&self.screen(), question)
                .ok_or_else(|| self.unexpected("Claude question selection is missing"))?;
            if current == target {
                return Ok(());
            }
            self.write(if current < target {
                b"\x1b[B"
            } else {
                b"\x1b[A"
            })?;
            self.wait_at("question-focus", |screen| {
                page(screen, question)
                    && focused(screen, question).is_some_and(|index| index != current)
            })?;
        }
    }

    pub(super) fn answer_question(
        &mut self,
        ask: &Ask,
        answer: &Answer,
        transcript: &Path,
    ) -> io::Result<()> {
        let expected = super::super::ask_user_question::encode_answers(ask, answer)?;
        for text in expected.values() {
            validate_text(text)?;
        }
        let mut file = File::open(transcript)?;
        let mut offset = crate::jsonl::complete_end(&mut file)?;
        let summary = super::super::transcript::read_cached(transcript, offset, None)?;
        let pending = summary
            .ask
            .ok_or_else(|| io::Error::other("Claude question is no longer pending"))?;
        let AskKind::Questions { questions, .. } = &ask.kind;
        let AskKind::Questions {
            questions: current, ..
        } = pending.kind;
        if pending.id != ask.id || current != *questions || questions.is_empty() {
            return Err(io::Error::other("Claude question changed"));
        }
        self.wait_at("question-ready", |screen| {
            review(screen) || questions.iter().any(|q| page(screen, q))
        })?;
        for _ in 0..=questions.len() {
            if page(&self.screen(), &questions[0]) {
                break;
            }
            let before = self.screen();
            self.write(b"\x1b[D")?;
            self.wait_at("question-first", |screen| {
                screen != before && (review(screen) || questions.iter().any(|q| page(screen, q)))
            })?;
        }
        for (index, question) in questions.iter().enumerate() {
            self.wait_at("question-page", |screen| page(screen, question))?;
            let value = answer
                .answers
                .get(&question.id)
                .cloned()
                .unwrap_or_default();
            if question.multi {
                // Clear selections before applying the dashboard's selection order.
                for option in 0..=question.options.len() {
                    match checked(&self.screen(), option) {
                        Some(true) => {
                            self.write((option + 1).to_string().as_bytes())?;
                            self.wait_at("question-uncheck", |screen| {
                                checked(screen, option) == Some(false)
                            })?;
                        }
                        Some(false) => {}
                        None => return Err(self.unexpected("Claude question checkbox is missing")),
                    }
                }
            }
            for label in &value.answers {
                let option = question
                    .options
                    .iter()
                    .position(|option| &option.label == label)
                    .ok_or_else(|| io::Error::other("Claude answer option is missing"))?;
                self.write((option + 1).to_string().as_bytes())?;
                if question.multi {
                    self.wait_at("question-check", |screen| {
                        checked(screen, option) == Some(true)
                    })?;
                }
            }
            if let Some(other) = &value.other {
                if question.multi {
                    self.question_focus(question, question.options.len())?;
                } else {
                    self.write((question.options.len() + 1).to_string().as_bytes())?;
                }
                self.wait_at("question-text", |screen| screen.contains("ctrl+g to edit"))?;
                self.write(b"\x15")?;
                self.paste(other)?;
                self.wait_at("question-text-visible", |screen| {
                    compact(screen).contains(&compact(other))
                })?;
                if question.multi {
                    self.wait_at("question-text-accepted", |screen| {
                        checked(screen, question.options.len()) == Some(true)
                    })?;
                } else {
                    self.write(b"\r")?;
                }
            }
            if question.multi {
                self.question_focus(question, question.options.len() + 1)?;
                self.write(b"\r")?;
            } else if value.answers.is_empty() && value.other.is_none() {
                self.write(b"\x1b[C")?;
            }
            if let Some(next) = questions.get(index + 1) {
                self.wait_at("question-next", |screen| page(screen, next))?;
            }
        }
        if questions.len() > 1 || questions.iter().any(|q| q.multi) {
            self.wait_at("question-review", review)?;
            self.write(b"1")?;
        }
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if file.metadata()?.len() < offset {
                return Err(io::Error::other("Claude transcript was truncated"));
            }
            let mut result = None;
            let mut parse_error = None;
            offset =
                crate::jsonl::read_lines(&mut file, offset, |line| {
                    match serde_json::from_slice::<serde_json::Value>(line) {
                        Ok(record) => {
                            if record["type"] == "user"
                                && record["message"]["content"]
                                    .as_array()
                                    .is_some_and(|blocks| {
                                        blocks.iter().any(|block| {
                                            block["type"] == "tool_result"
                                                && block["tool_use_id"] == ask.id
                                        })
                                    })
                            {
                                result = Some(record);
                            }
                        }
                        Err(error) => parse_error = Some(error),
                    }
                })?;
            if let Some(error) = parse_error {
                return Err(error.into());
            }
            if let Some(record) = result {
                let actual = &record["toolUseResult"]["answers"];
                if *actual != serde_json::to_value(&expected)? {
                    return Err(io::Error::other(format!(
                        "Claude recorded different answers: {actual}"
                    )));
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(self.unexpected("Claude question result timed out"));
            }
            self.read(Duration::from_millis(50))?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_native_question_uses_dialog_text_instead_of_transcript_prompt() {
        let mut parser = OutputScreen::new();
        parser.process(include_bytes!(
            "../../../../tests/fixtures/claude-attach/synchronized-question.raw"
        ));
        let screen = parser.complete().unwrap();
        let input: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/claude-ask-user-question/post.json"
        ))
        .unwrap();
        let input = &input["tool_input"]["questions"][0];
        let mut question = Question {
            id: input["question"].as_str().unwrap().into(),
            text: input["question"].as_str().unwrap().into(),
            header: input["header"].as_str().unwrap().into(),
            options: serde_json::from_value(input["options"].clone()).unwrap(),
            multi: input["multiSelect"].as_bool().unwrap(),
            other: true,
            secret: false,
        };
        assert!(!page(&screen, &question));
        question.text = "Ask debug two?".into();
        question.options[1].label = "Green".into();
        assert!(page(&screen, &question));
    }
}
