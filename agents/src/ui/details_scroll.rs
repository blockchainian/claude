use super::DetailsScroll;

impl DetailsScroll {
    /// Restores unchanged bodies and folds bodies that changed while away.
    pub(super) fn prepare(&mut self, keys: Vec<u64>) {
        if let Some(left) = self.left_keys.take()
            && left != keys
        {
            self.reveal = keys
                .iter()
                .map(|key| {
                    left.iter()
                        .position(|old| old == key)
                        .and_then(|index| self.reveal.get(index).copied())
                        .unwrap_or(0)
                })
                .collect();
            self.offset = 0;
        }
        if self.reveal.len() != keys.len() {
            self.reveal = vec![0; keys.len()];
        }
        self.keys = keys;
    }

    pub(super) fn budget(&mut self, wants: &[usize], allocations: &[usize]) {
        self.fallback_max = None;
        self.offset = 0;
        self.hidden = wants
            .iter()
            .zip(allocations)
            .map(|(want, rows)| want - rows)
            .collect();
        for (reveal, hidden) in self.reveal.iter_mut().zip(&self.hidden) {
            *reveal = (*reveal).min(*hidden);
        }
    }

    pub(super) fn position(&self) -> usize {
        self.fallback_max
            .map_or_else(|| self.reveal.iter().sum(), |_| self.offset)
    }

    /// Tries the pointed section, then the rest of the window before handing off.
    pub(super) fn scroll_window(&mut self, forward: bool, row: u16) -> bool {
        self.scroll(forward, row)
            || self.scroll(forward, if forward { self.content_top } else { u16::MAX })
    }

    /// Consumes one row inside the window, returning false at its directional boundary.
    pub(super) fn scroll(&mut self, forward: bool, row: u16) -> bool {
        if let Some(max) = self.fallback_max {
            let offset = if forward {
                self.offset.saturating_add(1).min(max)
            } else {
                self.offset.saturating_sub(1).min(max)
            };
            return std::mem::replace(&mut self.offset, offset) != offset;
        }
        let d =
            usize::from(row.saturating_sub(self.content_top)) + self.reveal.iter().sum::<usize>();
        let Some(target) = curtain_target(&self.bodies, &self.reveal, &self.hidden, d, forward)
        else {
            return false;
        };
        if forward {
            self.reveal[target] += 1;
            self.bodies[target].end += 1;
            for body in &mut self.bodies[target + 1..] {
                body.start += 1;
                body.end += 1;
            }
        } else {
            self.reveal[target] -= 1;
            self.bodies[target].end -= 1;
            for body in &mut self.bodies[target + 1..] {
                body.start -= 1;
                body.end -= 1;
            }
        }
        true
    }
}

/// Choose the body to open or close at a document row.
pub(super) fn curtain_target(
    bodies: &[std::ops::Range<usize>],
    reveal: &[usize],
    hidden: &[usize],
    d: usize,
    forward: bool,
) -> Option<usize> {
    let eligible = |i: usize| {
        if forward {
            reveal[i] < hidden[i]
        } else {
            reveal[i] > 0
        }
    };
    bodies
        .iter()
        .enumerate()
        .find(|(i, body)| body.contains(&d) && eligible(*i))
        .map(|(i, _)| i)
        .or_else(|| {
            bodies
                .iter()
                .enumerate()
                .find(|(i, body)| body.start > d && eligible(*i))
                .map(|(i, _)| i)
        })
        .or_else(|| {
            if forward {
                None
            } else {
                bodies
                    .iter()
                    .enumerate()
                    .rfind(|(i, body)| body.end <= d && eligible(*i))
                    .map(|(i, _)| i)
            }
        })
}
