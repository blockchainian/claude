//! Provider-neutral question pages and their collected answer.
use super::line_edit::LineEdit;
use crate::ask::{Answer, Ask, AskKind, Question, QuestionAnswer};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

pub(super) enum Outcome {
    Submit(Answer),
    Dismiss,
}

#[derive(Default)]
struct Page {
    focus: usize,
    selected: Vec<usize>,
    other: LineEdit,
    committed: bool,
}

pub(super) struct QuestionView {
    ask: Ask,
    page: usize,
    pages: Vec<Page>,
}

impl QuestionView {
    pub(super) fn new(ask: Ask) -> Self {
        let AskKind::Questions { questions, .. } = &ask.kind;
        let pages = questions.iter().map(|_| Page::default()).collect();
        Self {
            ask,
            page: 0,
            pages,
        }
    }

    pub(super) fn ask(&self) -> &Ask {
        &self.ask
    }

    fn questions(&self) -> &[Question] {
        let AskKind::Questions { questions, .. } = &self.ask.kind;
        questions
    }

    fn editing(&self) -> bool {
        self.questions().get(self.page).is_some_and(|question| {
            question.options.is_empty() || self.pages[self.page].focus == question.options.len()
        })
    }

    fn rows(&self) -> usize {
        self.questions().get(self.page).map_or(0, |question| {
            (question.options.len() + usize::from(question.other)).max(1)
        })
    }

    fn finish_page(&mut self) -> Option<Outcome> {
        self.pages[self.page].committed = true;
        if self.page + 1 < self.pages.len() {
            self.page += 1;
            return None;
        }
        if let Some(unanswered) = self.pages.iter().position(|page| !page.committed) {
            self.page = unanswered;
            return None;
        }
        let answers = self
            .questions()
            .iter()
            .zip(&self.pages)
            .map(|(question, page)| {
                let mut answers: Vec<String> = page
                    .selected
                    .iter()
                    .map(|&index| question.options[index].label.clone())
                    .collect();
                if !page.other.is_empty() {
                    answers.push(format!("user_note: {}", page.other.text()));
                }
                (question.id.clone(), QuestionAnswer { answers })
            })
            .collect();
        Some(Outcome::Submit(Answer { answers }))
    }

    fn toggle(&mut self, index: usize) {
        let selected = &mut self.pages[self.page].selected;
        if let Some(position) = selected.iter().position(|&value| value == index) {
            selected.remove(position);
        } else {
            selected.push(index);
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Option<Outcome> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Esc {
            return Some(Outcome::Dismiss);
        }
        if self.pages.is_empty() {
            return None;
        }
        let modifiers = key.modifiers - KeyModifiers::SHIFT;
        if matches!(
            (key.code, modifiers),
            (KeyCode::PageUp, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL)
        ) {
            self.page = self.page.saturating_sub(1);
            return None;
        }
        if matches!(
            (key.code, modifiers),
            (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL)
        ) {
            self.page = (self.page + 1).min(self.pages.len() - 1);
            return None;
        }
        let question = &self.questions()[self.page];
        let multi = question.multi;
        let option_count = question.options.len();
        let other = question.other;
        if self.editing() {
            match key.code {
                KeyCode::Enter if modifiers == KeyModifiers::NONE => {
                    if self.pages[self.page].other.is_empty() {
                        return None;
                    }
                    if !multi {
                        self.pages[self.page].selected.clear();
                    }
                    return self.finish_page();
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => {}
                _ => {
                    self.pages[self.page].other.handle(key);
                    return None;
                }
            }
        }
        if modifiers != KeyModifiers::NONE {
            return None;
        }
        let focus = self.pages[self.page].focus;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.pages[self.page].focus = (focus + self.rows() - 1) % self.rows();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.pages[self.page].focus = (focus + 1) % self.rows();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.page = self.page.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.page = (self.page + 1).min(self.pages.len() - 1);
            }
            KeyCode::Tab => {
                if other {
                    self.pages[self.page].focus = option_count;
                }
            }
            KeyCode::BackTab => self.pages[self.page].focus = 0,
            KeyCode::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                if index < option_count {
                    if multi {
                        self.toggle(index);
                    } else {
                        self.pages[self.page].focus = index;
                        self.pages[self.page].selected = vec![index];
                        self.pages[self.page].other.clear();
                        return self.finish_page();
                    }
                } else if other && index == option_count {
                    self.pages[self.page].focus = index;
                }
            }
            KeyCode::Char(' ') if multi && focus < option_count => self.toggle(focus),
            KeyCode::Char(' ') if !multi && focus < option_count => {
                self.pages[self.page].selected = vec![focus];
                self.pages[self.page].other.clear();
                return self.finish_page();
            }
            KeyCode::Enter => {
                if !multi {
                    self.pages[self.page].selected = vec![focus];
                    self.pages[self.page].other.clear();
                }
                return self.finish_page();
            }
            KeyCode::Backspace | KeyCode::Delete => {
                self.pages[self.page].selected.clear();
            }
            _ => {}
        }
        None
    }

    pub(super) fn paste(&mut self, text: &str) {
        if self.editing() {
            self.pages[self.page]
                .other
                .insert(text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }

    fn lines(&self, width: u16) -> (Vec<Line<'static>>, usize) {
        let Some(question) = self.questions().get(self.page) else {
            return (vec![Line::from("No pending questions")], 0);
        };
        let page = &self.pages[self.page];
        let mut lines = vec![
            Line::from(vec![
                Span::styled(question.header.clone(), Style::default().bold()),
                Span::styled(
                    format!("  ·  question {}/{}", self.page + 1, self.pages.len()),
                    Style::default().dim(),
                ),
            ]),
            Line::default(),
        ];
        for text in textwrap::wrap(&question.text, usize::from(width.max(1))) {
            lines.push(Line::from(text.into_owned()));
        }
        lines.push(Line::default());
        let mut focus_row = lines.len();
        for (index, option) in question.options.iter().enumerate() {
            let focused = page.focus == index;
            if focused {
                focus_row = lines.len();
            }
            let marker = if focused { "›" } else { " " };
            let check = if question.multi {
                if page.selected.contains(&index) {
                    "[x] "
                } else {
                    "[ ] "
                }
            } else {
                ""
            };
            let prefix = format!("{marker} {}. {check}", index + 1);
            let style = if focused {
                super::style::selection_style()
            } else {
                Style::default()
            };
            let available = usize::from(width).saturating_sub(prefix.width()).max(1);
            for (row, text) in textwrap::wrap(&option.label, available)
                .into_iter()
                .enumerate()
            {
                let prefix = if row == 0 {
                    prefix.clone()
                } else {
                    " ".repeat(prefix.chars().count())
                };
                lines.push(Line::styled(format!("{prefix}{text}"), style));
            }
            if !option.description.is_empty() {
                for text in textwrap::wrap(
                    &option.description,
                    usize::from(width.saturating_sub(5)).max(1),
                ) {
                    lines.push(Line::styled(format!("     {text}"), Style::default().dim()));
                }
            }
        }
        if question.other || question.options.is_empty() {
            let focused = page.focus == question.options.len();
            if focused {
                focus_row = lines.len();
            }
            let marker = if focused { "›" } else { " " };
            let text = if page.other.is_empty() {
                "Other".to_owned()
            } else if question.secret {
                "•".repeat(page.other.text().chars().count())
            } else {
                page.other
                    .window(usize::from(width.saturating_sub(5)))
                    .0
                    .to_owned()
            };
            let style = if focused {
                super::style::selection_style()
            } else {
                Style::default()
            };
            lines.push(Line::styled(
                format!("{marker} {}. {text}", question.options.len() + 1),
                style,
            ));
        }
        lines.push(Line::default());
        lines.push(Line::styled(
            "↑/↓ move · Enter to confirm · Esc to close",
            Style::default().dim(),
        ));
        (lines, focus_row)
    }

    pub(super) fn height(&self, width: u16) -> u16 {
        self.lines(width).0.len().min(usize::from(u16::MAX)) as u16
    }

    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (mut lines, focus) = self.lines(area.width);
        let height = usize::from(area.height);
        if lines.len() > height {
            let footer = lines.pop().expect("footer");
            let body_height = height.saturating_sub(1);
            let offset = (focus + 1).saturating_sub(body_height);
            lines = lines.into_iter().skip(offset).take(body_height).collect();
            lines.push(footer);
        }
        Paragraph::new(lines).render(area, buf);
    }

    pub(super) fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        if !self.editing() || area.is_empty() {
            return None;
        }
        let (_, focus) = self.lines(area.width);
        let visible = usize::from(area.height.saturating_sub(1));
        if visible == 0 {
            return None;
        }
        let row = focus.min(visible - 1) as u16;
        let (_, caret) = self.pages[self.page]
            .other
            .window(usize::from(area.width.saturating_sub(5)));
        Some((area.x + (5 + caret).min(area.width - 1), area.y + row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ask::QuestionOption;
    use chrono::Utc;
    use serde_json::Value;

    fn fixture(header: &str) -> Ask {
        let records = include_str!("../../tests/fixtures/claude-attach/ask/transcript.jsonl");
        for line in records.lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            let Some(content) = record["message"]["content"].as_array() else {
                continue;
            };
            for call in content {
                let Some(questions) = call["input"]["questions"].as_array() else {
                    continue;
                };
                if questions[0]["header"] != header {
                    continue;
                }
                return Ask {
                    id: call["id"].as_str().unwrap().into(),
                    session: "claude:probe".parse().unwrap(),
                    since: Utc::now(),
                    kind: AskKind::Questions {
                        blocking: true,
                        questions: questions
                            .iter()
                            .map(|question| Question {
                                id: question["question"].as_str().unwrap().into(),
                                text: question["question"].as_str().unwrap().into(),
                                header: question["header"].as_str().unwrap().into(),
                                options: question["options"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|option| QuestionOption {
                                        label: option["label"].as_str().unwrap().into(),
                                        description: option["description"].as_str().unwrap().into(),
                                        preview: None,
                                    })
                                    .collect(),
                                multi: question["multiSelect"].as_bool().unwrap(),
                                other: true,
                                secret: false,
                            })
                            .collect(),
                    },
                };
            }
        }
        panic!("real fixture lacks {header}")
    }

    fn press(view: &mut QuestionView, code: KeyCode) -> Option<Outcome> {
        view.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn submitted(outcome: Option<Outcome>) -> Answer {
        match outcome {
            Some(Outcome::Submit(answer)) => answer,
            _ => panic!("expected submit"),
        }
    }

    fn screen(view: &QuestionView, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    #[test]
    fn ask_view_single_digits_submit_real_labels() {
        let mut view = QuestionView::new(fixture("Single"));
        let answer = submitted(press(&mut view, KeyCode::Char('2')));
        assert_eq!(answer.answers["Probe single choice?"].answers, ["Green"]);
    }

    #[test]
    fn ask_view_navigation_wraps_and_enter_confirms() {
        let mut view = QuestionView::new(fixture("Single"));
        press(&mut view, KeyCode::Up);
        view.paste("custom probe note");
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(
            answer.answers["Probe single choice?"].answers,
            ["user_note: custom probe note"]
        );
        let mut view = QuestionView::new(fixture("Single"));
        press(&mut view, KeyCode::Down);
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(answer.answers["Probe single choice?"].answers, ["Green"]);
    }

    #[test]
    fn ask_view_multi_toggles_preserve_selection_order() {
        let mut view = QuestionView::new(fixture("Multi"));
        assert!(press(&mut view, KeyCode::Char('3')).is_none());
        assert!(press(&mut view, KeyCode::Char('1')).is_none());
        press(&mut view, KeyCode::Char('1'));
        press(&mut view, KeyCode::Char('1'));
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(
            answer.answers["Probe multiple choices?"].answers,
            ["Blue", "Red"]
        );
    }

    #[test]
    fn ask_view_two_questions_submit_together() {
        let mut view = QuestionView::new(fixture("First"));
        assert!(press(&mut view, KeyCode::Char('2')).is_none());
        press(&mut view, KeyCode::Down);
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(answer.answers.len(), 2);
        assert_eq!(answer.answers["Probe first page?"].answers, ["Green"]);
        assert_eq!(answer.answers["Probe second page?"].answers, ["Green"]);
    }

    #[test]
    fn ask_view_other_reuses_editor_and_escape_never_answers() {
        let mut view = QuestionView::new(fixture("Custom"));
        assert!(press(&mut view, KeyCode::Char('4')).is_none());
        view.paste("custom probe note!");
        press(&mut view, KeyCode::Backspace);
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(
            answer.answers["Probe custom text?"].answers,
            ["user_note: custom probe note"]
        );
        assert!(matches!(
            press(&mut view, KeyCode::Esc),
            Some(Outcome::Dismiss)
        ));
    }

    #[test]
    fn ask_view_snapshots_from_real_fixture() {
        for (header, expected) in [
            (
                "Single",
                include_str!("../../tests/ui_snapshots/ask_view_single.txt"),
            ),
            (
                "Multi",
                include_str!("../../tests/ui_snapshots/ask_view_multi.txt"),
            ),
            (
                "First",
                include_str!("../../tests/ui_snapshots/ask_view_two.txt"),
            ),
        ] {
            let view = QuestionView::new(fixture(header));
            assert_eq!(screen(&view, 60, 13), expected, "{header}");
        }
    }
    #[test]
    fn ask_view_other_digits_are_text_and_multi_space_toggles() {
        let mut view = QuestionView::new(fixture("Custom"));
        press(&mut view, KeyCode::Char('4'));
        press(&mut view, KeyCode::Char('2'));
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(
            answer.answers["Probe custom text?"].answers,
            ["user_note: 2"]
        );
        let mut view = QuestionView::new(fixture("Multi"));
        press(&mut view, KeyCode::Char(' '));
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Char(' '));
        press(&mut view, KeyCode::Char(' '));
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(answer.answers["Probe multiple choices?"].answers, ["Red"]);
    }

    #[test]
    fn ask_view_page_navigation_keeps_answers_and_blocks_unanswered() {
        let mut view = QuestionView::new(fixture("First"));
        press(&mut view, KeyCode::Right);
        assert!(press(&mut view, KeyCode::Char('3')).is_none());
        assert!(screen(&view, 60, 13).contains("First"));
        press(&mut view, KeyCode::Char('2'));
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(answer.answers["Probe first page?"].answers, ["Green"]);
        assert_eq!(answer.answers["Probe second page?"].answers, ["Blue"]);
    }

    #[test]
    fn ask_view_release_and_modified_digits_do_not_submit() {
        let mut view = QuestionView::new(fixture("Single"));
        let mut key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert!(view.key(key).is_none());
        assert!(view
            .key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL))
            .is_none());
        assert_eq!(
            submitted(press(&mut view, KeyCode::Enter)).answers["Probe single choice?"].answers,
            ["Red"]
        );
    }

    #[test]
    fn ask_view_narrow_layout_keeps_focused_other_and_footer_visible() {
        let mut view = QuestionView::new(fixture("Single"));
        press(&mut view, KeyCode::Up);
        view.paste("custom probe note");
        let rendered = screen(&view, 24, 4);
        assert!(rendered.contains("› 4."), "{rendered}");
        assert!(rendered.contains("Enter"), "{rendered}");
        let cursor = view.cursor(Rect::new(2, 3, 24, 4)).unwrap();
        assert!((2..26).contains(&cursor.0));
        assert!((3..7).contains(&cursor.1));
        assert!(view.height(12) > view.height(60));
        assert_eq!(view.ask().id, fixture("Single").id);
    }
}
