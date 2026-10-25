use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Input,
    List,
}

#[derive(Debug)]
pub(super) struct ViewState {
    pub viewed: Option<String>,
    pub focus: Focus,
    pub selected: Option<usize>,
    pub rows: Vec<Option<String>>,
}

impl ViewState {
    pub fn target(&self, description: Option<&str>) -> io::Result<usize> {
        let matches: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.as_deref() == description)
            .map(|(index, _)| index)
            .collect();
        match matches.as_slice() {
            [index] => Ok(*index),
            [] => Err(io::Error::other("Subagent is no longer running")),
            _ => Err(io::Error::other(
                "Multiple running subagents have the same description",
            )),
        }
    }
}

/// Only the footer below the editor is authoritative: transcript text can contain list markers.
pub(super) fn view_state(screen: &str) -> Option<ViewState> {
    input_box(screen)?;
    let (prompt_row, text) = prompt(screen)?;
    let footer: Vec<_> = screen.lines().skip(prompt_row + 2).collect();
    let list_focus = footer.iter().any(|line| {
        let text = line.trim();
        text == "↑/↓ to select"
            || text == "↑/↓ to select · Enter to view"
            || text == "Enter to view · x to stop"
    });
    let input_focus = footer.iter().any(|line| {
        (line.contains("shift+tab to cycle")
            && (line.contains('←') || matches!(input_box(screen), Some(PromptState::Editing(_)))))
            || line.trim() == "? for shortcuts"
    });
    if list_focus == input_focus {
        return None;
    }
    let mut rows = Vec::new();
    let mut viewed = Vec::new();
    let mut selected = Vec::new();
    for line in &footer {
        let line = line.trim_start();
        let (cursor, line) = match line.strip_prefix("❯ ") {
            Some(line) => (true, line),
            None => (false, line),
        };
        let (active, body) = if let Some(body) = line.strip_prefix("⏺ ") {
            (true, body)
        } else if let Some(body) = line.strip_prefix("◯ ") {
            (false, body)
        } else {
            continue;
        };
        let body = body.trim();
        let description = if body == "main" {
            None
        } else {
            // Claude right-aligns descriptions after a run of padding spaces.
            let (_, description) = body.split_once("  ")?;
            let description = description.trim();
            if description.is_empty() {
                return None;
            }
            Some(description.to_owned())
        };
        if active {
            viewed.push(description.clone());
        }
        if cursor {
            selected.push(rows.len());
        }
        rows.push(description);
    }
    if rows.is_empty() {
        // Without running Agents Claude removes the entire list.
        if list_focus || text.trim().starts_with("Message @") {
            return None;
        }
        return Some(ViewState {
            viewed: None,
            focus: Focus::Input,
            selected: None,
            rows: vec![None],
        });
    }
    if rows.first() != Some(&None)
        || viewed.len() != 1
        || (list_focus && selected.len() != 1)
        || (input_focus && !selected.is_empty())
    {
        return None;
    }
    Some(ViewState {
        viewed: viewed.pop()?,
        focus: if list_focus {
            Focus::List
        } else {
            Focus::Input
        },
        selected: selected.first().copied(),
        rows,
    })
}

impl Terminal {
    pub(super) fn current_view(&mut self) -> io::Result<ViewState> {
        self.read(Duration::ZERO)?;
        self.wait_at("view-and-focus", |screen| view_state(screen).is_some())?;
        Ok(view_state(&self.screen()).expect("confirmed view"))
    }

    pub(super) fn navigate(&mut self, target: Option<&str>) -> io::Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let state = self.current_view()?;
            let index = state.target(target)?;
            if Instant::now() >= deadline {
                return Err(self.unexpected("Claude view navigation timed out"));
            }
            match state.focus {
                Focus::Input if state.viewed.as_deref() == target => return Ok(()),
                Focus::Input => {
                    if !ready(&self.screen()) {
                        return Err(self.unexpected("Cannot navigate with text in Claude input"));
                    }
                    self.write(b"\x1b[B")?;
                    self.wait_at("list-focused", |screen| {
                        view_state(screen).is_some_and(|state| state.focus == Focus::List)
                    })?;
                }
                Focus::List if state.viewed.as_deref() == target => {
                    // Esc is safe only after the footer confirms list focus.
                    self.write(b"\x1b")?;
                    self.wait_at("input-focused", |screen| {
                        view_state(screen).is_some_and(|state| {
                            state.focus == Focus::Input && state.viewed.as_deref() == target
                        })
                    })?;
                }
                Focus::List => {
                    let cursor = state.selected.expect("list focus has cursor");
                    if cursor == index {
                        self.write(b"\r")?;
                        self.wait_at("view-selected", |screen| {
                            view_state(screen)
                                .is_some_and(|state| state.viewed.as_deref() == target)
                        })?;
                    } else {
                        self.write(if cursor < index { b"\x1b[B" } else { b"\x1b[A" })?;
                        // Confirm each single move, including list changes while navigating.
                        self.wait_at("list-cursor-moved", |screen| {
                            view_state(screen).is_some_and(|next| {
                                next.focus == Focus::List && next.selected != Some(cursor)
                            })
                        })?;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(name: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-attach/subagents")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn captured_view_and_focus() {
        for (file, viewed, focus, selected) in [
            ("main-input-focus.txt", None, Focus::Input, None),
            ("main-busy-list-focus.txt", None, Focus::List, Some(0)),
            (
                "viewing-agent-list-focus.txt",
                Some("probe looper"),
                Focus::List,
                Some(1),
            ),
            (
                "viewing-agent-input-focus.txt",
                Some("probe looper"),
                Focus::Input,
                None,
            ),
            (
                "viewing-agent-after-send.txt",
                Some("probe looper"),
                Focus::Input,
                None,
            ),
        ] {
            let screen = captured(file);
            let view = view_state(&screen).unwrap();
            assert_eq!(view.viewed.as_deref(), viewed, "{file}");
            assert_eq!(view.focus, focus, "{file}");
            assert_eq!(view.selected, selected, "{file}");
            assert_eq!(view.rows, vec![None, Some("probe looper".into())]);
            assert!(ready(&screen), "{file}");
            assert!(!left_input(&screen), "{file}");
        }
    }

    #[test]
    fn captured_target_requires_one_description() {
        let screen = captured("viewing-agent-input-focus.txt");
        let state = view_state(&screen).unwrap();
        assert_eq!(state.target(Some("probe looper")).unwrap(), 1);
        assert!(
            state
                .target(Some("gone"))
                .unwrap_err()
                .to_string()
                .contains("no longer running")
        );
        let row = screen.lines().last().unwrap();
        let duplicate = format!("{screen}\n{}", row.replace('⏺', "◯"));
        assert!(
            view_state(&duplicate)
                .unwrap()
                .target(Some("probe looper"))
                .unwrap_err()
                .to_string()
                .contains("same description")
        );
    }

    #[test]
    fn captured_input_marker_is_not_list_cursor() {
        let screen = captured("viewing-agent-list-focus.txt");
        assert_eq!(
            prompt(&screen).unwrap().1.trim(),
            "Message @general-purpose…"
        );
        assert!(view_state(&screen.replace("↑/↓ to select", "unknown focus")).is_none());
        assert!(view_state(&screen.replace("❯ ⏺", "❯ ◯")).is_none());
    }

    #[test]
    fn captured_paste_hides_the_agent_shortcut() {
        let screen = captured("main-pasted-input.txt");
        let view = view_state(&screen).unwrap();
        assert_eq!(view.focus, Focus::Input);
        assert_eq!(view.viewed, None);
        assert!(paste_visible(&screen));
    }

    #[test]
    fn empty_editor_without_loaded_footer_does_not_establish_a_view() {
        let screen = captured("main-pasted-input.txt");
        let (row, _) = prompt(&screen).unwrap();
        let lines: Vec<_> = screen.lines().collect();
        let bottom = lines
            .iter()
            .enumerate()
            .skip(row + 1)
            .find(|(_, line)| line.trim_start().starts_with('─'))
            .unwrap()
            .0;
        let partial = lines[..row]
            .iter()
            .copied()
            .chain(["❯ "])
            .chain(lines[bottom..].iter().copied())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(view_state(&partial).is_none());
    }
}
