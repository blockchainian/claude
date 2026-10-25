//! Merged session snapshots and filesystem notifications.

mod refresh;
mod watcher;
pub use refresh::{Refresh, RefreshRequest};
mod weekly_usage;

use crate::model::{Session, SessionStatus};
use crate::providers::{
    Capabilities, Provider, ProviderId, SessionAction, SessionKey, Snapshot, Supervision,
    SupervisionState,
};
use std::collections::{HashMap, HashSet};
use std::{
    io,
    path::{Path, PathBuf},
};

pub use watcher::{OutputWatcher, watch};

pub struct Hub {
    providers: Vec<Box<dyn Provider>>,
    pages: usize,
    weekly_usage: weekly_usage::WeeklyUsageCache,
}

impl Hub {
    pub fn new(providers: Vec<Box<dyn Provider>>) -> Self {
        Self {
            providers,
            pages: 1,
            weekly_usage: weekly_usage::WeeklyUsageCache::default(),
        }
    }

    pub fn provider(&self, id: ProviderId) -> &dyn Provider {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .expect("Session provider is not registered")
            .as_ref()
    }

    /// Detaches a held session through its supervisor, otherwise archives it through its provider.
    pub fn archive(
        &self,
        session: &Session,
        supervision: Option<&Supervision>,
    ) -> io::Result<SessionAction> {
        match supervision {
            Some(supervision)
                if matches!(
                    supervision.state,
                    SupervisionState::Active | SupervisionState::Asking(_)
                ) =>
            {
                self.provider(supervision.supervisor.provider)
                    .detach(supervision)
            }
            _ => self.provider(session.id.provider).archive(session),
        }
    }

    pub fn display_names(&self) -> Vec<(ProviderId, &'static str)> {
        let mut names = self
            .providers
            .iter()
            .map(|provider| (provider.id(), provider.display_name()))
            .collect::<Vec<_>>();
        names.sort_by_key(|(id, _)| *id);
        names
    }

    pub fn capabilities(&self) -> HashMap<ProviderId, Capabilities> {
        self.providers
            .iter()
            .map(|provider| (provider.id(), provider.capabilities()))
            .collect()
    }

    pub fn input_choices(&self) -> HashMap<ProviderId, crate::providers::InputChoices> {
        self.providers
            .iter()
            .map(|provider| (provider.id(), provider.input_choices()))
            .collect()
    }

    pub fn set_weekly_usage(&mut self, values: HashMap<ProviderId, crate::providers::WeeklyUsage>) {
        self.weekly_usage.values = values;
    }

    #[cfg(test)]
    pub fn weekly_usage_jobs(
        &mut self,
        now: std::time::Instant,
    ) -> Vec<(ProviderId, crate::providers::WeeklyUsageJob)> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let job = provider.weekly_usage()?;
                self.weekly_usage
                    .start(provider.id(), now)
                    .then_some((provider.id(), job))
            })
            .collect()
    }

    pub fn account_usage_jobs(
        &mut self,
        now: std::time::Instant,
    ) -> Vec<(ProviderId, crate::providers::AccountUsageJob)> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let job = provider.account_usage()?;
                self.weekly_usage
                    .start(provider.id(), now)
                    .then_some((provider.id(), job))
            })
            .collect()
    }

    pub fn weekly_usage_complete(
        &mut self,
        provider: ProviderId,
        result: io::Result<Option<crate::providers::WeeklyUsage>>,
    ) {
        self.weekly_usage.complete(provider, result);
    }

    pub fn weekly_usage(&self) -> &HashMap<ProviderId, crate::providers::WeeklyUsage> {
        &self.weekly_usage.values
    }

    pub fn task_defaults(
        &self,
        cwd: &Path,
    ) -> Option<(ProviderId, io::Result<crate::tasks::TaskDefaults>)> {
        self.providers.iter().find_map(|provider| {
            provider
                .task_defaults(cwd)
                .map(|defaults| (provider.id(), defaults))
        })
    }

    pub fn watch_roots(&self) -> io::Result<Vec<PathBuf>> {
        let mut roots = Vec::new();
        for provider in &self.providers {
            roots.extend(provider.watch_roots()?);
        }
        Ok(roots)
    }

    pub fn show_more(&mut self) {
        self.pages = self.pages.saturating_add(1);
    }

    pub fn session_source(&self) -> Refresh {
        Refresh::new(Self::new(
            self.providers.iter().map(|p| p.session_source()).collect(),
        ))
    }

    pub fn pages(&self) -> usize {
        self.pages
    }

    pub fn refresh_paths(&mut self, paths: &[PathBuf], previous: Snapshot) -> Snapshot {
        let Some(affected) = refresh::affected_sessions(paths, &previous.sessions) else {
            return self.refresh();
        };
        self.refresh_selected(Some((&affected, &previous)))
    }

    pub fn refresh(&mut self) -> Snapshot {
        self.refresh_selected(None)
    }

    fn refresh_selected(&mut self, changed: Option<(&HashSet<SessionKey>, &Snapshot)>) -> Snapshot {
        let mut snapshots: Vec<_> = self
            .providers
            .iter_mut()
            .map(|provider| match changed {
                None => provider.discover(self.pages),
                Some((affected, previous)) => {
                    let ids = affected
                        .iter()
                        .filter(|id| id.provider == provider.id())
                        .cloned()
                        .collect::<Vec<_>>();
                    provider.refresh_sessions(self.pages, &ids, &previous.sessions)
                }
            })
            .collect();
        let mut frontier: Vec<_> = snapshots
            .iter()
            .flat_map(|s| &s.sessions)
            .filter(|s| s.process.is_some())
            .map(|s| s.id.clone())
            .collect();
        let mut selected: HashSet<_> = snapshots
            .iter()
            .flat_map(|s| &s.sessions)
            .map(|s| s.id.clone())
            .collect();
        let mut visited = HashSet::new();
        while !frontier.is_empty() {
            frontier.retain(|id| visited.insert(id.clone()));
            if frontier.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for (provider, snapshot) in self.providers.iter_mut().zip(&mut snapshots) {
                let children = provider.children(&frontier);
                snapshot.supervisions.extend(children.supervisions);
                for child in children.sessions {
                    next.push(child.id.clone());
                    if selected.insert(child.id.clone()) {
                        snapshot.sessions.push(child);
                    }
                }
                // Providers may discover children together with their parents.
                next.extend(
                    snapshot
                        .sessions
                        .iter()
                        .filter(|s| s.parent.as_ref().is_some_and(|p| frontier.contains(p)))
                        .map(|s| s.id.clone()),
                );
            }
            let mut loads: HashMap<_, HashSet<_>> = HashMap::new();
            for supervision in snapshots.iter().flat_map(|snapshot| &snapshot.supervisions) {
                if frontier.contains(&supervision.supervisor)
                    && !selected.contains(&supervision.session)
                {
                    loads
                        .entry(supervision.session.provider)
                        .or_default()
                        .insert(supervision.session.clone());
                }
            }
            for (provider, snapshot) in self.providers.iter_mut().zip(&mut snapshots) {
                let Some(ids) = loads.remove(&provider.id()) else {
                    continue;
                };
                let loaded = provider.load(&ids.into_iter().collect::<Vec<_>>());
                snapshot.supervisions.extend(loaded.supervisions);
                for session in loaded.sessions {
                    next.push(session.id.clone());
                    if selected.insert(session.id.clone()) {
                        snapshot.sessions.push(session);
                    }
                }
            }
            frontier = next;
        }
        let supervisions: Vec<_> = snapshots
            .iter()
            .flat_map(|snapshot| snapshot.supervisions.iter().cloned())
            .collect();
        merge(
            self.providers
                .iter_mut()
                .zip(snapshots)
                .map(|(provider, snapshot)| {
                    let Some((affected, previous)) = changed else {
                        return provider.enrich(snapshot);
                    };
                    let mut cached = Vec::new();
                    let mut details = Vec::new();
                    for session in snapshot.sessions {
                        if !affected.contains(&session.id)
                            && let Some(before) = previous
                                .sessions
                                .iter()
                                .find(|before| before.id == session.id)
                        {
                            cached.push(before.clone());
                        } else {
                            details.push(session);
                        }
                    }
                    let mut updated = provider.enrich(Snapshot {
                        sessions: details,
                        has_more: snapshot.has_more,
                        supervisions: snapshot.supervisions,
                    });
                    updated.sessions.extend(cached);
                    updated
                })
                .map(|mut snapshot| {
                    apply_supervisions(&mut snapshot, &supervisions);
                    snapshot
                }),
        )
    }
}

fn apply_supervisions(snapshot: &mut Snapshot, supervisions: &[Supervision]) {
    for supervision in supervisions {
        let Some(session) = snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == supervision.session)
        else {
            continue;
        };
        if session.parent.is_none()
            && let Some(name) = &session.name
        {
            session.title.clone_from(name);
        }
        session.parent = Some(supervision.supervisor.clone());
        if let SupervisionState::Asking(questions) = &supervision.state {
            session.status = SessionStatus::NeedsInput;
            session.pending_questions.clone_from(questions);
        }
    }
}

fn merge(snapshots: impl Iterator<Item = Snapshot>) -> Snapshot {
    let mut sessions = Vec::new();
    let mut has_more = false;
    let mut supervisions = Vec::new();
    for snapshot in snapshots {
        sessions.extend(snapshot.sessions);
        has_more |= snapshot.has_more;
        supervisions.extend(snapshot.supervisions);
    }
    let ids: HashSet<_> = sessions.iter().map(|s| s.id.clone()).collect();
    for session in &mut sessions {
        if session.parent.as_ref().is_some_and(|p| !ids.contains(p)) {
            session.parent = None;
        }
    }
    let (mut parents, mut children): (Vec<_>, Vec<_>) = sessions
        .into_iter()
        .partition(|session| session.parent.is_none());
    parents.sort_by(|a, b| {
        (a.status == SessionStatus::Inactive)
            .cmp(&(b.status == SessionStatus::Inactive))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    let positions: HashMap<_, _> = parents
        .iter()
        .chain(&children)
        .enumerate()
        .map(|(index, session)| (session.id.clone(), index))
        .collect();
    children.sort_by(|a, b| {
        positions[a.parent.as_ref().expect("child has a parent")]
            .cmp(&positions[b.parent.as_ref().expect("child has a parent")])
            .then_with(|| a.id.cmp(&b.id))
    });
    parents.extend(children);
    Snapshot {
        sessions: parents,
        has_more,
        supervisions,
    }
}

#[cfg(test)]
#[path = "../providers/claude/hub_tests.rs"]
mod tests;

#[cfg(test)]
mod supervision_tests;
