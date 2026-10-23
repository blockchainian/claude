//! File inputs shared by the selected transcript and the activity timeline.

use super::*;
use std::collections::{BTreeSet, HashMap};
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
struct ActivityInput {
    session: SessionKey,
    path: Option<PathBuf>,
    status: SessionStatus,
    updated: chrono::DateTime<chrono::Utc>,
}

impl ActivityInput {
    fn matches(&self, session: &Session) -> bool {
        self.session == session.id
            && self.path == session.transcript
            && self.status == session.status
            && self.updated == session.updated_at
    }
}

struct ActivityFile {
    stamp: FileStamp,
    turns: crate::activity::Turns,
    since: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Default)]
pub(super) struct ActivityCache {
    inputs: Vec<ActivityInput>,
    files: HashMap<PathBuf, ActivityFile>,
    checked: Option<Instant>,
    frame: Option<(u16, i64)>,
    #[cfg(test)]
    pub stats: usize,
}

impl ActivityCache {
    pub fn update(
        &mut self,
        sessions: &[Session],
        width: u16,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<BTreeSet<i64>> {
        let inputs_changed = self.inputs.len() != sessions.len()
            || !self
                .inputs
                .iter()
                .zip(sessions)
                .all(|(input, session)| input.matches(session));
        let frame = (width, crate::usage::bin_of(now));
        let window_expanded = self
            .frame
            .is_some_and(|previous| width > previous.0 || frame.1 < previous.1);
        // Poll independently of paints; changed session inputs invalidate the cache immediately.
        let check = inputs_changed
            || window_expanded
            || self
                .checked
                .is_none_or(|checked| checked.elapsed() >= POLLING_INTERVAL);
        if !check && self.frame == Some(frame) {
            return None;
        }
        self.frame = Some(frame);
        if inputs_changed {
            self.inputs = sessions
                .iter()
                .map(|session| ActivityInput {
                    session: session.id.clone(),
                    path: session.transcript.clone(),
                    status: session.status,
                    updated: session.updated_at,
                })
                .collect();
            self.files.retain(|path, _| {
                sessions
                    .iter()
                    .any(|session| session.transcript.as_ref() == Some(path))
            });
        }
        if check {
            self.checked = Some(Instant::now());
        }
        let since = now - chrono::Duration::seconds(crate::ui::timeline_seconds(width));
        let mut bins = BTreeSet::new();
        for session in sessions {
            let Some(path) = &session.transcript else {
                continue;
            };
            let works = session.status == SessionStatus::Working;
            if check {
                #[cfg(test)]
                {
                    self.stats += 1;
                }
                let Ok(stamp) = FileStamp::read(path) else {
                    self.files.remove(path);
                    continue;
                };
                if !works && chrono::DateTime::<chrono::Utc>::from(stamp.modified) < since {
                    self.files.remove(path);
                    continue;
                }
                let file = self
                    .files
                    .entry(path.clone())
                    .or_insert_with(|| ActivityFile {
                        stamp: stamp.clone(),
                        turns: Default::default(),
                        since: None,
                    });
                if file.stamp != stamp {
                    if !stamp.appended_to(&file.stamp) {
                        file.turns = Default::default();
                    }
                    file.since = None;
                    file.stamp = stamp;
                }
            }
            let Some(file) = self.files.get_mut(path) else {
                continue;
            };
            if file.since.is_none_or(|covered| since < covered) {
                if file.turns.update(path, session.id.provider, since).is_err() {
                    continue;
                }
                file.since = Some(since);
            }
            for (start, end) in file.turns.spans(now, works) {
                bins.extend(crate::usage::bin_of(start.max(since))..=crate::usage::bin_of(end));
            }
        }
        Some(bins)
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
    inputs: Vec<ActivityInput>,
    pub files: HashMap<PathBuf, UsageFile>,
    pub checked: Option<Instant>,
    frame: Option<(u16, i64)>,
    selected: Option<SessionKey>,
    active: UsageBins,
    #[cfg(test)]
    pub reads: usize,
    #[cfg(test)]
    pub aggregates: usize,
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
                .map(|session| ActivityInput {
                    session: session.id.clone(),
                    path: session.transcript.clone(),
                    status: session.status,
                    updated: session.updated_at,
                })
                .collect();
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
        if let Some(session) = sessions
            .iter()
            .find(|session| Some(&session.id) == selected)
            && let Some(file) = session
                .transcript
                .as_ref()
                .and_then(|path| self.files.get_mut(path))
        {
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
