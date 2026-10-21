#[derive(Default)]
pub(crate) struct Groups(Vec<(String, Vec<String>)>);
impl Groups {
    pub(crate) fn add(&mut self, name: &str, member: Option<&str>) {
        let index = self
            .0
            .iter()
            .position(|(n, _)| n == name)
            .unwrap_or_else(|| {
                self.0.push((name.into(), vec![]));
                self.0.len() - 1
            });
        if let Some(member) = member
            && !self.0[index].1.iter().any(|s| s == member)
        {
            self.0[index].1.push(member.into());
        }
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
        for (name, members) in self.0 {
            if !text.is_empty() {
                text.push_str(", ");
            }
            text.push_str(&name);
            if !members.is_empty() {
                text.push(' ');
                if members.len() > 1 {
                    text.push('{');
                }
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        text.push_str(", ");
                    }
                    let start = text.len();
                    text.push_str(member);
                    ranges.push(start..text.len());
                }
                if members.len() > 1 {
                    text.push('}');
                }
            }
        }
        Some((text, ranges))
    }
}
