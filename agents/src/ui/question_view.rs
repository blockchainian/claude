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
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use unicode_width::UnicodeWidthStr;

pub(super) enum Outcome {
    Submit(Answer),
    Dismiss,
}

/// What a mouse position points at: the question tab row, or an option row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Target {
    /// The tab row, with the question whose tab is under the pointer.
    Tabs(Option<usize>),
    Option(usize),
}

#[derive(Clone, Default)]
struct Page {
    focus: usize,
    selected: Vec<usize>,
    other: LineEdit,
}

impl Page {
    fn answered(&self) -> bool {
        !self.selected.is_empty() || !self.other.is_empty()
    }
}

/// Rows on the Submit tab, which follows the questions when Claude Code shows one.
const REVIEW_ROWS: [&str; 2] = ["Submit", "Cancel"];

#[derive(Clone)]
pub(super) struct QuestionView {
    ask: Ask,
    /// The shown tab: a question, or the Submit tab after the last question.
    page: usize,
    pages: Vec<Page>,
    /// The focused Submit tab row; it starts on Submit.
    review_focus: usize,
    error: Option<String>,
}

impl QuestionView {
    pub(super) fn new(ask: Ask) -> Self {
        let AskKind::Questions { questions, .. } = &ask.kind;
        let pages = questions.iter().map(|_| Page::default()).collect();
        Self {
            ask,
            page: 0,
            pages,
            review_focus: 0,
            error: None,
        }
    }

    pub(super) fn ask(&self) -> &Ask {
        &self.ask
    }

    pub(super) fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    fn footer(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if let Some(error) = &self.error {
            lines.extend(
                textwrap::wrap(error, usize::from(width.max(1)))
                    .into_iter()
                    .map(|text| {
                        Line::styled(text.into_owned(), super::style::attention_title_style())
                    }),
            );
        }
        let switch = if self.tabs() > 1 {
            "←/→ switch · "
        } else {
            ""
        };
        lines.push(Line::styled(
            format!("{switch}↑/↓ move · Enter to confirm · Esc to close"),
            Style::default().dim(),
        ));
        lines
    }

    fn body_height(&self, area: Rect) -> usize {
        let height = usize::from(area.height);
        height.saturating_sub(self.footer(area.width).len())
    }

    fn questions(&self) -> &[Question] {
        let AskKind::Questions { questions, .. } = &self.ask.kind;
        questions
    }

    /// Whether the questions end with a Submit tab, as Claude Code shows for several
    /// questions or a multi-select one; a lone single-select question submits its choice.
    fn has_review(&self) -> bool {
        self.questions().len() > 1 || self.questions().iter().any(|question| question.multi)
    }

    fn tabs(&self) -> usize {
        self.pages.len() + usize::from(self.has_review())
    }

    fn reviewing(&self) -> bool {
        self.has_review() && self.page == self.pages.len()
    }

    fn other_row(question: &Question) -> bool {
        question.other || question.options.is_empty()
    }

    fn editing(&self) -> bool {
        self.questions().get(self.page).is_some_and(|question| {
            Self::other_row(question) && self.pages[self.page].focus == question.options.len()
        })
    }

    fn rows(&self) -> usize {
        if self.reviewing() {
            return REVIEW_ROWS.len();
        }
        self.questions().get(self.page).map_or(0, |question| {
            question.options.len()
                + usize::from(Self::other_row(question))
        })
    }

    /// Moves the focus; on a single-select question the focused option is its answer.
    fn set_focus(&mut self, row: usize) {
        let question = &self.questions()[self.page];
        let (multi, option_count) = (question.multi, question.options.len());
        let page = &mut self.pages[self.page];
        page.focus = row;
        if !multi {
            page.selected = if row < option_count { vec![row] } else { Vec::new() };
        }
    }

    fn submit(&self) -> Outcome {
        let answers = self
            .questions()
            .iter()
            .zip(&self.pages)
            .map(|(question, page)| (question.id.clone(), Self::answer(question, page)))
            .collect();
        Outcome::Submit(Answer { answers })
    }

    fn answer(question: &Question, page: &Page) -> QuestionAnswer {
        QuestionAnswer {
            answers: page
                .selected
                .iter()
                .map(|&index| question.options[index].label.clone())
                .collect(),
            other: (!page.other.is_empty()).then(|| page.other.text().to_owned()),
        }
    }

    /// Moves to the next tab, or submits when there is no Submit tab to move to.
    fn advance(&mut self) -> Option<Outcome> {
        if self.page + 1 < self.tabs() {
            self.page += 1;
            self.review_focus = 0;
            None
        } else {
            Some(self.submit())
        }
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
            self.scroll(false, true);
            return None;
        }
        if matches!(
            (key.code, modifiers),
            (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL)
        ) {
            self.scroll(true, true);
            return None;
        }
        if self.editing() {
            match key.code {
                KeyCode::Enter if modifiers == KeyModifiers::NONE => {
                    if self.pages[self.page].other.is_empty() {
                        return None;
                    }
                    if !self.questions()[self.page].multi {
                        self.pages[self.page].selected.clear();
                    }
                    return self.advance();
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
        if self.reviewing() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                    self.review_focus = 1 - self.review_focus;
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => self.scroll(false, true),
                KeyCode::Char(digit @ '1'..='9') => {
                    return self.choose(digit as usize - '1' as usize);
                }
                KeyCode::Char(' ') | KeyCode::Enter => return self.choose(self.review_focus),
                _ => {}
            }
            return None;
        }
        let focus = self.pages[self.page].focus;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.set_focus((focus + self.rows() - 1) % self.rows());
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.set_focus((focus + 1) % self.rows());
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => self.scroll(false, true),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => self.scroll(true, true),
            KeyCode::Char(digit @ '1'..='9') => {
                return self.choose(digit as usize - '1' as usize);
            }
            KeyCode::Char(' ') | KeyCode::Enter => return self.choose(focus),
            KeyCode::Backspace | KeyCode::Delete => {
                self.pages[self.page].selected.clear();
            }
            _ => {}
        }
        None
    }

    /// Acts on a row the way its digit key does: a single-select option answers and moves
    /// on, a multi-select option toggles, the free-text row takes focus, and the Submit
    /// tab's rows submit or cancel.
    pub(super) fn choose(&mut self, index: usize) -> Option<Outcome> {
        if self.reviewing() {
            return match index {
                0 => Some(self.submit()),
                1 => Some(Outcome::Dismiss),
                _ => None,
            };
        }
        let question = self.questions().get(self.page)?;
        let (multi, option_count, other) = (
            question.multi,
            question.options.len(),
            Self::other_row(question),
        );
        if index < option_count {
            self.pages[self.page].focus = index;
            if multi {
                self.toggle(index);
                return None;
            }
            self.pages[self.page].selected = vec![index];
            self.pages[self.page].other.clear();
            return self.advance();
        }
        if other && index == option_count {
            self.pages[self.page].focus = index;
        }
        None
    }

    pub(super) fn show_page(&mut self, page: usize) {
        if page < self.tabs() {
            self.page = page;
        }
    }

    #[cfg(test)]
    pub(super) fn page(&self) -> usize {
        self.page
    }

    /// Steps the focused row, or the shown tab, stopping at either end.
    pub(super) fn scroll(&mut self, forward: bool, tabs: bool) {
        let step = |value: usize, len: usize| {
            if forward {
                (value + 1).min(len.saturating_sub(1))
            } else {
                value.saturating_sub(1)
            }
        };
        if tabs {
            self.page = step(self.page, self.tabs());
        } else if self.reviewing() {
            self.review_focus = step(self.review_focus, REVIEW_ROWS.len());
        } else if !self.pages.is_empty() {
            self.set_focus(step(self.pages[self.page].focus, self.rows()));
        }
    }

    fn tab_labels(&self) -> Vec<&str> {
        let mut labels: Vec<&str> = self
            .questions()
            .iter()
            .map(|question| question.header.as_str())
            .collect();
        if self.has_review() {
            labels.push("Submit");
        }
        labels
    }

    pub(super) fn target(&self, area: Rect, column: u16, row: u16) -> Option<Target> {
        if !area.contains((column, row).into()) {
            return None;
        }
        let (_, rows, offset) = self.visible(area);
        let y = usize::from(row - area.y);
        if offset == 0 && y == 0 && !self.pages.is_empty() {
            let mut x = area.x;
            for (index, label) in self.tab_labels().into_iter().enumerate() {
                if index > 0 {
                    x += "·".width() as u16;
                }
                let width = (label.width() + 2) as u16;
                if (x..x.saturating_add(width)).contains(&column) {
                    return Some(Target::Tabs(Some(index)));
                }
                x = x.saturating_add(width);
            }
            return Some(Target::Tabs(None));
        }
        rows.get(y).copied().flatten().map(Target::Option)
    }

    pub(super) fn paste(&mut self, text: &str) {
        if self.editing() {
            self.pages[self.page]
                .other
                .insert(text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }

    /// A row with the focus marker and its number, such as "› 2. Green".
    fn row_line(focused: bool, number: usize, text: Vec<Span<'static>>) -> Line<'static> {
        let marker = if focused { "›" } else { " " };
        let mut spans = vec![Span::raw(format!("{marker} {number}. "))];
        spans.extend(text);
        Line::from(spans)
    }

    /// The Submit tab: a warning while questions are unanswered, one line per answer, then
    /// its rows. The warning and answers keep one line per question, so the tab's height
    /// never changes as questions are answered.
    fn review_lines(&self, width: u16, lines: &mut Vec<Line<'static>>) -> Vec<Option<usize>> {
        let start = lines.len();
        if self.pages.iter().any(|page| !page.answered()) {
            lines.push(Line::styled(
                "You have not answered all questions",
                super::style::attention_title_style(),
            ));
            lines.push(Line::default());
        }
        for (question, page) in self.questions().iter().zip(&self.pages) {
            if !page.answered() {
                continue;
            }
            let answer = Self::answer(question, page);
            let values: Vec<String> = answer.answers.into_iter().chain(answer.other).collect();
            lines.push(truncate_line_with_ellipsis_if_overflow(
                Line::from(format!("● {} → {}", question.header, values.join(", "))),
                usize::from(width),
            ));
        }
        lines.resize(start + self.pages.len() + 2, Line::default());
        lines.push(Line::default());
        let mut rows = vec![None; lines.len()];
        for (index, label) in REVIEW_ROWS.into_iter().enumerate() {
            rows.push(Some(index));
            lines.push(Self::row_line(
                self.review_focus == index,
                index + 1,
                vec![Span::raw(label)],
            ));
        }
        rows
    }

    /// The view's lines, the focused line, and the row each body line belongs to.
    fn lines(&self, width: u16) -> (Vec<Line<'static>>, usize, Vec<Option<usize>>) {
        if self.pages.is_empty() {
            return (vec![Line::from("No pending questions")], 0, vec![None]);
        }
        let mut tabs = Vec::new();
        for (index, label) in self.tab_labels().into_iter().enumerate() {
            if index > 0 {
                tabs.push(Span::styled("·", Style::default().dim()));
            }
            let style = if index == self.page {
                super::style::active_tab_style()
            } else {
                super::style::inactive_tab_style()
            };
            tabs.push(Span::styled(format!(" {label} "), style));
        }
        let mut lines = vec![Line::from(tabs), Line::default()];
        let mut rows = if self.reviewing() {
            self.review_lines(width, &mut lines)
        } else {
            self.question_lines(width, &mut lines)
        };
        let focus = Some(if self.reviewing() {
            self.review_focus
        } else {
            self.pages[self.page].focus
        });
        let focus_row = rows
            .iter()
            .position(|&row| row.is_some() && row == focus)
            .unwrap_or(0);
        lines.push(Line::default());
        rows.push(None);
        lines.extend(self.footer(width));
        (lines, focus_row, rows)
    }

    fn question_lines(&self, width: u16, lines: &mut Vec<Line<'static>>) -> Vec<Option<usize>> {
        let question = &self.questions()[self.page];
        let page = &self.pages[self.page];
        lines.extend(super::text::lines(
            &question.text,
            std::path::Path::new(""),
            usize::from(width.max(1)),
            false,
        ));
        lines.push(Line::default());
        let mut rows = vec![None; lines.len()];
        for (index, option) in question.options.iter().enumerate() {
            let focused = page.focus == index;
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
                lines.push(Line::from(format!("{prefix}{text}")));
            }
            if !option.description.is_empty() {
                for text in textwrap::wrap(
                    &option.description,
                    usize::from(width.saturating_sub(5)).max(1),
                ) {
                    lines.push(Line::styled(format!("     {text}"), Style::default().dim()));
                }
            }
            rows.resize(lines.len(), Some(index));
        }
        if Self::other_row(question) {
            let row = question.options.len();
            let text = if page.other.is_empty() {
                Span::styled("Type something.", Style::default().dim())
            } else if question.secret {
                Span::raw("•".repeat(page.other.text().chars().count()))
            } else {
                Span::raw(
                    page.other
                        .window(usize::from(width.saturating_sub(5)))
                        .0
                        .to_owned(),
                )
            };
            lines.push(Self::row_line(page.focus == row, row + 1, vec![text]));
            rows.push(Some(row));
        }
        rows
    }

    /// The tallest tab's height, so switching tabs never moves the panels around the view.
    pub(super) fn height(&self, width: u16) -> u16 {
        let mut tab = self.clone();
        (0..self.tabs().max(1))
            .map(|page| {
                tab.page = page;
                tab.lines(width).0.len()
            })
            .max()
            .unwrap_or(0)
            .min(usize::from(u16::MAX)) as u16
    }

    /// The lines that fit `area`, scrolled to keep the focus and footer visible, with each
    /// visible body line's row and the number of body lines scrolled off the top.
    fn visible(&self, area: Rect) -> (Vec<Line<'static>>, Vec<Option<usize>>, usize) {
        let (mut lines, focus, rows) = self.lines(area.width);
        let height = usize::from(area.height);
        if lines.len() <= height {
            // Blank rows above the footer keep it on the area's last line on a shorter tab.
            let footer = lines.len() - self.footer(area.width).len();
            lines.splice(
                footer..footer,
                std::iter::repeat_n(Line::default(), height - lines.len()),
            );
            return (lines, rows, 0);
        }
        let footer = lines.split_off(lines.len() - self.footer(area.width).len());
        let body_height = self.body_height(area);
        let offset = (focus + 1).saturating_sub(body_height);
        let rows = rows.into_iter().skip(offset).take(body_height).collect();
        lines = lines.into_iter().skip(offset).take(body_height).collect();
        lines.extend(footer.into_iter().take(height - body_height));
        (lines, rows, offset)
    }

    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        Paragraph::new(self.visible(area).0).render(area, buf);
    }

    pub(super) fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        if !self.editing() || area.is_empty() {
            return None;
        }
        let (_, rows, _) = self.visible(area);
        let focus = self.pages[self.page].focus;
        let row = rows.iter().position(|&row| row == Some(focus))?;
        let (_, caret) = self.pages[self.page]
            .other
            .window(usize::from(area.width.saturating_sub(5)));
        Some((
            area.x + (5 + caret).min(area.width - 1),
            area.y + row as u16,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ask::QuestionOption;
    use chrono::Utc;
    use ratatui::style::{Color, Modifier};
    use serde_json::Value;

    fn fixture(header: &str) -> Ask {
        let records = include_str!("../../tests/fixtures/claude-questions/transcript.jsonl");
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
        assert!(answer.answers["Probe single choice?"].answers.is_empty());
        assert_eq!(
            answer.answers["Probe single choice?"].other.as_deref(),
            Some("custom probe note")
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
        assert!(press(&mut view, KeyCode::Right).is_none());
        let answer = submitted(press(&mut view, KeyCode::Char('1')));
        assert_eq!(
            answer.answers["Probe multiple choices?"].answers,
            ["Blue", "Red"]
        );
    }

    #[test]
    fn ask_view_multi_enter_toggles_and_right_moves_on() {
        let mut view = QuestionView::new(fixture("Multi"));
        assert!(press(&mut view, KeyCode::Enter).is_none());
        assert!(screen(&view, 60, 14).contains("› 1. [x] Red"));
        assert!(press(&mut view, KeyCode::Enter).is_none());
        assert!(
            !screen(&view, 60, 14)
                .lines()
                .any(|line| line.trim() == "Submit")
        );
        assert!(press(&mut view, KeyCode::Right).is_none());
        assert!(screen(&view, 60, 14).contains("You have not answered all questions"));
        let answer = submitted(press(&mut view, KeyCode::Char('1')));
        assert_eq!(
            answer.answers["Probe multiple choices?"],
            QuestionAnswer::default()
        );
    }

    #[test]
    fn ask_view_two_questions_submit_together() {
        let mut view = QuestionView::new(fixture("First"));
        assert!(press(&mut view, KeyCode::Char('2')).is_none());
        press(&mut view, KeyCode::Down);
        assert!(press(&mut view, KeyCode::Enter).is_none());
        let review = screen(&view, 60, 14);
        assert!(review.contains("● First → Green"), "{review}");
        assert!(review.contains("● Second → Green"), "{review}");
        assert!(!review.contains("not answered"), "{review}");
        let answer = submitted(press(&mut view, KeyCode::Char('1')));
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
            answer.answers["Probe custom text?"].other.as_deref(),
            Some("custom probe note")
        );
        assert!(matches!(
            press(&mut view, KeyCode::Esc),
            Some(Outcome::Dismiss)
        ));
    }

    #[test]
    fn question_body_renders_recorded_inline_code() {
        let record: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/claude-questions-pending.jsonl"
        ))
        .unwrap();
        let mut ask = fixture("Single");
        let AskKind::Questions { questions, .. } = &mut ask.kind;
        questions[0].text = record["message"]["content"][0]["input"]["questions"][0]["question"]
            .as_str()
            .unwrap()
            .to_owned();
        let view = QuestionView::new(ask);
        let mut lines = Vec::new();
        view.question_lines(160, &mut lines);
        assert!(lines[0].to_string().contains("rg -n 'background' src"));
        assert!(!lines[0].to_string().contains('`'));
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
            if let Some(directory) = std::env::var_os("UPDATE_ASK_VIEW_SNAPSHOT") {
                let name = match header {
                    "Single" => "ask_view_single.txt",
                    "Multi" => "ask_view_multi.txt",
                    _ => "ask_view_two.txt",
                };
                std::fs::write(
                    std::path::Path::new(&directory).join(name),
                    screen(&view, 60, 14),
                )
                .unwrap();
            }
            assert_eq!(screen(&view, 60, 14), expected, "{header}");
        }
    }
    #[test]
    fn ask_view_highlights_only_the_active_tab() {
        let mut view = QuestionView::new(fixture("First"));
        press(&mut view, KeyCode::Right);
        press(&mut view, KeyCode::Up);
        let area = Rect::new(0, 0, 60, 13);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let selection = super::super::style::selection_style();
        assert_eq!(buf[(1, 0)].symbol(), "F");
        assert_eq!(buf[(1, 0)].bg, Color::Reset);
        assert_eq!(buf[(9, 0)].symbol(), "S");
        assert_eq!(Some(buf[(9, 0)].bg), selection.bg);
        let other = (0..13)
            .find(|&y| buf[(0, y)].symbol() == "›")
            .expect("focused row");
        assert_eq!(buf[(5, other)].symbol(), "T");
        assert!(buf[(5, other)].modifier.contains(Modifier::DIM));
        assert!((0..60).all(|x| buf[(x, other)].bg == Color::Reset));
    }

    #[test]
    fn ask_view_tab_row_targets_questions_and_wheel_stops_at_ends() {
        let mut view = QuestionView::new(fixture("First"));
        let area = Rect::new(2, 5, 60, 13);
        assert_eq!(view.target(area, 4, 5), Some(Target::Tabs(Some(0))));
        assert_eq!(view.target(area, 12, 5), Some(Target::Tabs(Some(1))));
        assert_eq!(view.target(area, 40, 5), Some(Target::Tabs(None)));
        let green = (5..18)
            .find(|&y| screen(&view, 60, 13).lines().nth(usize::from(y - 5)) == Some("  2. Green"))
            .unwrap();
        assert_eq!(view.target(area, 3, green), Some(Target::Option(1)));
        assert_eq!(view.target(area, 3, green + 1), Some(Target::Option(1)));
        view.show_page(1);
        assert!(screen(&view, 60, 13).contains("Probe second page?"));
        view.scroll(true, true);
        view.scroll(true, true);
        assert_eq!(view.page(), 2);
        view.scroll(false, true);
        view.scroll(false, true);
        view.scroll(false, true);
        assert_eq!(view.page(), 0);
    }

    #[test]
    fn ask_view_focused_single_option_is_its_answer() {
        let mut view = QuestionView::new(fixture("First"));
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Right);
        press(&mut view, KeyCode::Right);
        let review = screen(&view, 60, 14);
        assert!(review.contains("● First → Green"), "{review}");
        assert!(review.contains("You have not answered all questions"), "{review}");
        assert!(!review.contains("Review your answers"), "{review}");
        assert!(review.contains("› 1. Submit"), "{review}");
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(answer.answers["Probe first page?"].answers, ["Green"]);
        assert!(answer.answers["Probe second page?"].answers.is_empty());
    }

    #[test]
    fn ask_view_height_stays_fixed_across_tabs() {
        let mut view = QuestionView::new(fixture("First"));
        let height = view.height(60);
        press(&mut view, KeyCode::Char('2'));
        press(&mut view, KeyCode::Char('3'));
        assert_eq!(view.height(60), height);
        for page in [2, 1, 0] {
            view.show_page(page);
            assert_eq!(view.height(60), height, "tab {page}");
        }
        view.show_page(2);
        let review = screen(&view, 60, height);
        assert!(
            review.lines().last().unwrap().ends_with("Esc to close"),
            "{review}"
        );
    }

    #[test]
    fn ask_view_other_digits_are_text_and_multi_space_toggles() {
        let mut view = QuestionView::new(fixture("Custom"));
        press(&mut view, KeyCode::Char('4'));
        press(&mut view, KeyCode::Char('2'));
        let answer = submitted(press(&mut view, KeyCode::Enter));
        assert_eq!(
            answer.answers["Probe custom text?"].other.as_deref(),
            Some("2")
        );
        let mut view = QuestionView::new(fixture("Multi"));
        press(&mut view, KeyCode::Char(' '));
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Char(' '));
        press(&mut view, KeyCode::Char(' '));
        press(&mut view, KeyCode::Right);
        let answer = submitted(press(&mut view, KeyCode::Char('1')));
        assert_eq!(answer.answers["Probe multiple choices?"].answers, ["Red"]);
    }

    #[test]
    fn ask_view_submit_tab_sends_unanswered_questions() {
        let mut view = QuestionView::new(fixture("First"));
        press(&mut view, KeyCode::Right);
        assert!(press(&mut view, KeyCode::Char('3')).is_none());
        let review = screen(&view, 60, 14);
        assert!(
            review.contains("You have not answered all questions"),
            "{review}"
        );
        assert!(press(&mut view, KeyCode::Down).is_none());
        assert!(matches!(
            press(&mut view, KeyCode::Char('2')),
            Some(Outcome::Dismiss)
        ));
        let answer = submitted(press(&mut view, KeyCode::Char('1')));
        assert_eq!(
            answer.answers["Probe first page?"],
            QuestionAnswer::default()
        );
        assert_eq!(answer.answers["Probe second page?"].answers, ["Blue"]);
    }

    #[test]
    fn ask_view_release_and_modified_digits_do_not_submit() {
        let mut view = QuestionView::new(fixture("Single"));
        let mut key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert!(view.key(key).is_none());
        assert!(
            view.key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL))
                .is_none()
        );
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
        view.set_error(Some("Question changed".into()));
        let rendered = screen(&view, 24, 4);
        assert!(rendered.contains("› 4."), "{rendered}");
        assert!(rendered.contains("Question changed"), "{rendered}");
        assert!(rendered.contains("Enter"), "{rendered}");
    }
}
