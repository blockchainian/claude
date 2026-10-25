//! File inputs shared by the selected transcript and the usage chart.

use super::*;
use std::collections::{HashMap, HashSet};
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp {
    identity: (u64, u64),
    pub modified: std::time::SystemTime,
    pub length: u64,
}

impl FileStamp {
    pub fn read(path: &std::path::Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            identity: (metadata.dev(), metadata.ino()),
            modified: metadata.modified()?,
            length: metadata.len(),
        })
    }

    pub fn appended_to(&self, previous: &Self) -> bool {
        self.identity == previous.identity && self.length > previous.length
    }
}

#[derive(PartialEq, Eq)]
struct UsageInput {
    session: SessionKey,
    parent: Option<SessionKey>,
    path: Option<PathBuf>,
    status: SessionStatus,
    updated: chrono::DateTime<chrono::Utc>,
}

impl UsageInput {
    fn matches(&self, session: &Session) -> bool {
        self.session == session.id
            && self.parent == session.parent
            && self.path == session.transcript
            && self.status == session.status
            && self.updated == session.updated_at
    }
}

pub(super) struct UsageFile {
    stamp: FileStamp,
    pub series: crate::usage::Series,
    loaded: bool,
}

type UsageBins = std::collections::BTreeMap<i64, crate::usage::Bin>;

#[derive(Default)]
pub(super) struct UsageCache {
    inputs: Vec<UsageInput>,
    pub files: HashMap<PathBuf, UsageFile>,
    pub checked: Option<Instant>,
    frame: Option<(u16, i64)>,
    selected: Option<SessionKey>,
    selected_sessions: HashSet<SessionKey>,
    active: UsageBins,
    #[cfg(test)]
    pub reads: usize,
    #[cfg(test)]
    pub aggregates: usize,
    #[cfg(test)]
    pub selections: usize,
    #[cfg(test)]
    pub stats: usize,
}

impl UsageCache {
    pub fn context_percent(&self, id: &SessionKey) -> Option<u64> {
        let input = self.inputs.iter().find(|input| &input.session == id)?;
        self.files
            .get(input.path.as_ref()?)?
            .series
            .context_percent()
    }

    pub fn update(
        &mut self,
        sessions: &[Session],
        selected: Option<&SessionKey>,
        width: u16,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<(UsageBins, UsageBins)> {
        let inputs_changed = self.inputs.len() != sessions.len()
            || !self
                .inputs
                .iter()
                .zip(sessions)
                .all(|(input, session)| input.matches(session));
        let frame = (width, crate::usage::bin_of(now));
        let expanded = self
            .frame
            .is_some_and(|previous| width > previous.0 || frame.1 < previous.1);
        let selection_changed = self.selected.as_ref() != selected;
        let check = inputs_changed
            || expanded
            || self
                .checked
                .is_none_or(|checked| checked.elapsed() >= POLLING_INTERVAL);
        let mut aggregate_changed = inputs_changed || self.frame != Some(frame);
        let mut changed = aggregate_changed || selection_changed;
        if !check && !changed {
            return None;
        }
        self.selected = selected.cloned();
        self.frame = Some(frame);
        if inputs_changed {
            self.inputs = sessions
                .iter()
                .map(|session| UsageInput {
                    session: session.id.clone(),
                    parent: session.parent.clone(),
                    path: session.transcript.clone(),
                    status: session.status,
                    updated: session.updated_at,
                })
                .collect();
        }
        if inputs_changed || selection_changed {
            #[cfg(test)]
            {
                self.selections += 1;
            }
            let mut children: HashMap<&SessionKey, Vec<&SessionKey>> = HashMap::new();
            for input in &self.inputs {
                if let Some(parent) = &input.parent {
                    children.entry(parent).or_default().push(&input.session);
                }
            }
            self.selected_sessions.clear();
            let mut pending: Vec<_> = selected.into_iter().collect();
            while let Some(session) = pending.pop() {
                if self.selected_sessions.insert(session.clone())
                    && let Some(descendants) = children.get(session)
                {
                    pending.extend(descendants.iter().copied());
                }
            }
        }
        self.files.retain(|path, _| {
            sessions.iter().any(|session| {
                (session.status != SessionStatus::Inactive || Some(&session.id) == selected)
                    && session.transcript.as_ref() == Some(path)
            })
        });
        if check {
            self.checked = Some(Instant::now());
        }
        let first = frame.1 - frame.1.rem_euclid(2) + 2
            - crate::ui::timeline_seconds(width) / crate::usage::BIN_SECONDS;
        let since = chrono::DateTime::from_timestamp(first * crate::usage::BIN_SECONDS, 0)
            .expect("usage window timestamp");
        for session in sessions.iter().filter(|session| {
            session.status != SessionStatus::Inactive || Some(&session.id) == selected
        }) {
            let Some(path) = &session.transcript else {
                continue;
            };
            if !check && (!selection_changed || self.files.contains_key(path)) {
                continue;
            }
            #[cfg(test)]
            {
                self.stats += 1;
            }
            let Ok(stamp) = FileStamp::read(path) else {
                let removed = self.files.remove(path).is_some();
                changed |= removed;
                aggregate_changed |= removed;
                continue;
            };
            let file = self.files.entry(path.clone()).or_insert_with(|| UsageFile {
                stamp: stamp.clone(),
                series: Default::default(),
                loaded: false,
            });
            if !expanded && file.stamp == stamp && file.loaded {
                continue;
            }
            if expanded || !stamp.appended_to(&file.stamp) {
                file.series = Default::default();
            }
            #[cfg(test)]
            {
                self.reads += 1;
            }
            match file.series.update_window(path, session.id.provider, since) {
                Ok(updated) => {
                    changed |= updated;
                    aggregate_changed |= updated;
                    file.stamp = stamp;
                    file.loaded = true;
                }
                Err(_) => {
                    self.files.remove(path);
                    changed = true;
                    aggregate_changed = true;
                }
            }
        }
        if !changed {
            return None;
        }
        if aggregate_changed {
            #[cfg(test)]
            {
                self.aggregates += 1;
            }
            self.active.clear();
            for session in sessions
                .iter()
                .filter(|session| session.status != SessionStatus::Inactive)
            {
                let Some(file) = session
                    .transcript
                    .as_ref()
                    .and_then(|path| self.files.get_mut(path))
                else {
                    continue;
                };
                file.series.bins.retain(|bin, _| *bin >= first);
                if first > frame.1 {
                    continue;
                }
                for (&bin, usage) in file.series.bins.range(first..=frame.1) {
                    let sum = self.active.entry(bin).or_default();
                    sum.input += usage.input;
                    sum.output += usage.output;
                }
            }
        }
        // These maps contain at most the visible dot columns, independent of transcript size.
        let mut total = self.active.clone();
        let mut others = self.active.clone();
        for session in sessions.iter().filter(|session| {
            self.selected_sessions.contains(&session.id)
                && (session.status != SessionStatus::Inactive || Some(&session.id) == selected)
        }) {
            let Some(file) = session
                .transcript
                .as_ref()
                .and_then(|path| self.files.get_mut(path))
            else {
                continue;
            };
            file.series.bins.retain(|bin, _| *bin >= first);
            if first <= frame.1 {
                for (&bin, usage) in file.series.bins.range(first..=frame.1) {
                    if session.status == SessionStatus::Inactive {
                        let sum = total.entry(bin).or_default();
                        sum.input += usage.input;
                        sum.output += usage.output;
                    } else if let Some(sum) = others.get_mut(&bin) {
                        sum.input -= usage.input;
                        sum.output -= usage.output;
                    }
                }
            }
        }
        others.retain(|_, bin| bin.input != 0 || bin.output != 0);
        Some((total, others))
    }
}
