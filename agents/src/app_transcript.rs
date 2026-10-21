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
