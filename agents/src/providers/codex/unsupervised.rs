use super::app_server::{Client, thread};
use crate::providers::claude::codex_manager::supervision;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Keeps every unsupervised Codex thread subscribed on one daemon connection,
/// so the daemon never unloads it while `agents` runs.
pub(super) struct UnsupervisedThreads {
    stopped: Arc<AtomicBool>,
}

impl UnsupervisedThreads {
    /// Subscribes to every unsupervised thread on a connection owned by a new OS thread.
    pub(super) fn subscribe(home: PathBuf, state: PathBuf) -> io::Result<Self> {
        Self::start(home, state, Duration::from_secs(5))
    }

    fn start(home: PathBuf, state: PathBuf, interval: Duration) -> io::Result<Self> {
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        std::thread::Builder::new()
            .name("codex-unsupervised".into())
            .spawn(move || run(&home, &state, &stop, interval))?;
        Ok(Self { stopped })
    }
}

/// Stops the OS thread at its next wake; the closed connection lets the threads unload.
impl Drop for UnsupervisedThreads {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

fn run(home: &Path, state: &Path, stopped: &AtomicBool, interval: Duration) {
    const RECONNECT_DELAYS: [u64; 5] = [1, 2, 5, 10, 30];
    let mut client = None;
    let mut subscribed = HashSet::new();
    let mut failures = 0;
    while !stopped.load(Ordering::Relaxed) {
        let deadline = Instant::now() + interval;
        if client.is_none() {
            match home
                .join("app-server-control/app-server-control.sock")
                .try_exists()
            {
                Ok(false) => {
                    std::thread::sleep(interval);
                    continue;
                }
                Err(error) => {
                    eprintln!("agents: Codex subscriptions: {error}");
                    std::thread::sleep(interval);
                    continue;
                }
                Ok(true) => {}
            }
            match Client::connect(home) {
                Ok(connected) => {
                    client = Some(connected);
                    failures = 0;
                }
                Err(error) => {
                    eprintln!("agents: Codex subscriptions: {error}");
                    std::thread::sleep(Duration::from_secs(RECONNECT_DELAYS[failures]));
                    failures = (failures + 1).min(RECONNECT_DELAYS.len() - 1);
                    continue;
                }
            }
        }
        let connected = client.as_mut().expect("connected above");
        if let Err(error) = reconcile(home, state, connected, &mut subscribed) {
            eprintln!("agents: Codex subscriptions: {error}");
        }
        if connected.discard_until(deadline).is_err() {
            client = None;
            subscribed.clear();
        }
    }
}

fn reconcile(
    home: &Path,
    state: &Path,
    client: &mut Client,
    subscribed: &mut HashSet<String>,
) -> io::Result<()> {
    let ids = supervision::unsupervised_threads(state)?;
    let mut wanted = HashSet::new();
    if !ids.is_empty() {
        let db = Connection::open_with_flags(
            home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(io::Error::other)?;
        let mut query = db
            .prepare("SELECT archived FROM threads WHERE id = ?")
            .map_err(io::Error::other)?;
        for id in ids {
            let archived: Option<bool> = query
                .query_row([&id], |row| row.get(0))
                .optional()
                .map_err(io::Error::other)?;
            if archived == Some(false) {
                wanted.insert(id);
            } else {
                supervision::remove_thread_record(state, &id)?;
            }
        }
    }
    for id in &wanted {
        if subscribed.contains(id) {
            continue;
        }
        match thread::resume(client, id) {
            Ok(()) => {
                subscribed.insert(id.clone());
            }
            Err(error) => eprintln!("agents: Codex subscribe {id}: {error}"),
        }
    }
    for id in subscribed.difference(&wanted) {
        if let Err(error) = thread::unsubscribe(client, id) {
            eprintln!("agents: Codex unsubscribe {id}: {error}");
        }
    }
    subscribed.retain(|id| wanted.contains(id));
    Ok(())
}

#[cfg(test)]
mod tests;
