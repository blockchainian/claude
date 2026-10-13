//! Merged session snapshots and filesystem notifications.

mod watcher;

use crate::model::SessionStatus;
use crate::providers::{Capabilities, Provider, ProviderId, Snapshot};
use std::collections::{HashMap, HashSet};
use std::{io, path::PathBuf};

pub use watcher::watch;

pub struct Hub {
    providers: Vec<Box<dyn Provider>>,
    pages: usize,
}

impl Hub {
    pub fn new(providers: Vec<Box<dyn Provider>>) -> Self {
        Self {
            providers,
            pages: 1,
        }
    }

    pub fn provider(&self, id: ProviderId) -> &dyn Provider {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .expect("Session provider is not registered")
            .as_ref()
    }

    pub fn default_provider(&self) -> ProviderId {
        self.providers
            .first()
            .expect("No providers are registered")
            .id()
    }

    pub fn capabilities(&self) -> HashMap<ProviderId, Capabilities> {
        self.providers
            .iter()
            .map(|provider| (provider.id(), provider.capabilities()))
            .collect()
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

    pub fn refresh(&mut self) -> Snapshot {
        merge(
            self.providers
                .iter_mut()
                .map(|provider| provider.refresh(self.pages)),
        )
    }
}

fn merge(snapshots: impl Iterator<Item = Snapshot>) -> Snapshot {
    let mut sessions = Vec::new();
    let mut has_more = false;
    for snapshot in snapshots {
        sessions.extend(snapshot.sessions);
        has_more |= snapshot.has_more;
    }
    // Remove orphaned descendants too, so every retained parent remains in the snapshot.
    loop {
        let ids: HashSet<_> = sessions.iter().map(|session| session.id.clone()).collect();
        let count = sessions.len();
        sessions.retain(|session| {
            session
                .parent
                .as_ref()
                .is_none_or(|parent| ids.contains(parent))
        });
        if sessions.len() == count {
            break;
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
    }
}

#[cfg(test)]
#[path = "../providers/claude/hub_tests.rs"]
mod tests;
