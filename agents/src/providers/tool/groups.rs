#[derive(Default)]
pub(crate) struct Groups(Vec<Group>);

struct Group {
    name: String,
    children: Groups,
    subject: bool,
    terminal: bool,
}

impl Groups {
    pub(crate) fn add(&mut self, name: &str, member: Option<&str>) {
        self.add_path(&[name.to_owned()], member);
    }

    /// Tool-name components form branches; titles remain leaves with path ranges.
    pub(crate) fn add_path(&mut self, names: &[String], member: Option<&str>) {
        let Some((name, rest)) = names.split_first() else {
            return;
        };
        let group = self.node(name, false, rest.is_empty());
        if rest.is_empty() {
            if let Some(member) = member {
                group.children.node(member, true, true);
            }
        } else {
            group.children.add_path(rest, member);
        }
    }

    fn node(&mut self, name: &str, subject: bool, terminal: bool) -> &mut Group {
        let index = self
            .0
            .iter()
            .position(|group| {
                group.name == name && group.subject == subject && group.terminal == terminal
            })
            .unwrap_or_else(|| {
                self.0.push(Group {
                    name: name.into(),
                    children: Self::default(),
                    subject,
                    terminal,
                });
                self.0.len() - 1
            });
        &mut self.0[index]
    }

    pub(crate) fn finish(self) -> Option<String> {
        self.finish_with_ranges().map(|(text, _)| text)
    }

    /// Member ranges keep path subjects available to the existing path formatter.
    pub(crate) fn finish_with_ranges(self) -> Option<(String, Vec<std::ops::Range<usize>>)> {
        if self.0.is_empty() {
            return None;
        }
        let mut text = String::new();
        let mut ranges = Vec::new();
        self.render(&mut text, &mut ranges);
        Some((text, ranges))
    }

    fn render(self, text: &mut String, ranges: &mut Vec<std::ops::Range<usize>>) {
        for (index, group) in self.0.into_iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            let start = text.len();
            text.push_str(&group.name);
            if group.subject {
                ranges.push(start..text.len());
            }
            if !group.children.0.is_empty() {
                text.push(' ');
                let braces = group.children.0.len() > 1;
                if braces {
                    text.push('{');
                }
                group.children.render(text, ranges);
                if braces {
                    text.push('}');
                }
            }
        }
    }
}
