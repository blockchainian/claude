use super::DetailsScroll;

/// Source items and cuts used by both drawing and scroll boundaries.
pub(super) struct DetailsLayout {
    pub kept: Vec<usize>,
    pub cuts: Vec<usize>,
    pub scroll: usize,
    pub total: usize,
    pub max: usize,
}

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

    /// Lay out the actual rendered items, including titles and multi-row boxes.
    pub(super) fn layout(
        &mut self,
        heights: &[usize],
        bodies: &[std::ops::Range<usize>],
        allocations: Option<&[usize]>,
        excluded: std::ops::Range<usize>,
        available: usize,
    ) -> DetailsLayout {
        let mut keep = vec![true; heights.len()];
        keep[excluded].fill(false);
        let max = heights
            .iter()
            .zip(&keep)
            .filter(|(_, kept)| **kept)
            .map(|(height, _)| height)
            .sum::<usize>()
            .saturating_sub(available);
        let mut cuts = Vec::new();
        if let Some(allocations) = allocations {
            let wants = bodies
                .iter()
                .map(|body| {
                    assert!(
                        heights[body.clone()].iter().all(|&height| height == 1),
                        "foldable bodies must contain rendered text rows"
                    );
                    body.len()
                })
                .collect::<Vec<_>>();
            self.budget(&wants, allocations);
            for (index, body) in bodies.iter().enumerate() {
                let rows = allocations[index] + self.reveal[index];
                keep[body.start + rows..body.end].fill(false);
                if rows > 0 && self.reveal[index] < self.hidden[index] {
                    cuts.push(body.start + rows - 1);
                }
            }
        } else {
            self.hidden.clear();
            self.fallback_max = Some(max);
            self.offset = self.offset.min(max);
        }
        let mut offsets = vec![0];
        for (&height, &kept) in heights.iter().zip(&keep) {
            offsets.push(offsets.last().unwrap() + if kept { height } else { 0 });
        }
        self.bodies = bodies
            .iter()
            .map(|body| offsets[body.start]..offsets[body.end])
            .collect();
        DetailsLayout {
            kept: keep
                .iter()
                .enumerate()
                .filter_map(|(index, &kept)| kept.then_some(index))
                .collect(),
            cuts,
            scroll: self.position(),
            total: *offsets.last().unwrap(),
            max,
        }
    }

    /// A gesture reaches its boundary before a later gesture can leave the window.
    pub(super) fn scroll_rows(&mut self, forward: bool, row: u16, rows: usize) -> bool {
        let mut consumed = false;
        for _ in 0..rows {
            if !self.scroll_window(forward, row) {
                break;
            }
            consumed = true;
        }
        consumed
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
