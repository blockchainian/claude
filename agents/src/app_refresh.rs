use super::*;
use crate::sessions::{Refresh, RefreshRequest};

pub(super) struct SessionRefresh {
    idle: Option<Refresh>,
    pending: Option<RefreshRequest>,
    generation: u64,
    pub stops: Vec<(u64, SessionKey, bool)>,
    pub deletes: Vec<(u64, SessionKey, Option<PathBuf>)>,
}

impl SessionRefresh {
    pub fn new(refresh: Refresh) -> Self {
        Self {
            idle: Some(refresh),
            pending: None,
            generation: 0,
            stops: Vec::new(),
            deletes: Vec::new(),
        }
    }
}

impl App {
    pub(super) fn request_refresh(&mut self) -> u64 {
        self.request_sessions(RefreshRequest::Full)
    }

    pub(super) fn request_sessions(&mut self, request: RefreshRequest) -> u64 {
        self.session_refresh.generation += 1;
        if let Some(pending) = &mut self.session_refresh.pending {
            pending.merge(request);
        } else {
            self.session_refresh.pending = Some(request);
        }
        self.start_session_refresh();
        self.session_refresh.generation
    }

    fn start_session_refresh(&mut self) {
        let Some(mut refresh) = self.session_refresh.idle.take() else {
            return;
        };
        let Some(request) = self.session_refresh.pending.take() else {
            self.session_refresh.idle = Some(refresh);
            return;
        };
        let generation = self.session_refresh.generation;
        let pages = self.hub.pages();
        let sender = self.sender.clone();
        // The worker owns its caches. UI actions never wait on a refresh lock, and
        // shutdown drops the receiver without joining a potentially blocked reader.
        thread::Builder::new()
            .name("agents-refresh".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    refresh.run(pages, request)
                }))
                .map_err(|_| io::Error::other("Session refresh panicked"));
                let _ = sender.send(Input::Sessions {
                    refresh: Box::new(refresh),
                    generation,
                    result,
                });
            })
            .expect("Cannot start session refresh worker");
    }

    pub(super) fn finish_sessions(
        &mut self,
        refresh: Refresh,
        generation: u64,
        result: io::Result<Snapshot>,
    ) -> io::Result<()> {
        self.session_refresh.idle = Some(refresh);
        self.apply_sessions(result?);
        let stops = std::mem::take(&mut self.session_refresh.stops);
        for (requested, id, success) in stops {
            if requested <= generation {
                self.pending_inputs.remove(&id);
                self.view.stop_complete(&id, success);
            } else {
                self.session_refresh.stops.push((requested, id, success));
            }
        }
        let deletes = std::mem::take(&mut self.session_refresh.deletes);
        for (requested, id, worktree) in deletes {
            if requested <= generation {
                self.delete_after_refresh(id, worktree);
            } else {
                self.session_refresh.deletes.push((requested, id, worktree));
            }
        }
        self.start_session_refresh();
        Ok(())
    }

    fn delete_after_refresh(&mut self, id: SessionKey, worktree: Option<PathBuf>) {
        if let Some(session) = self.session(&id) {
            if !session.can_resume() {
                return;
            }
            if session.transcript.is_some() {
                self.delete(session, worktree);
                return;
            }
        }
        self.view.show_info("Nothing to delete".into());
    }

    /// Applies completed real-provider refreshes for synchronous fixture assertions.
    #[cfg(test)]
    pub(super) fn refresh(&mut self) {
        self.request_refresh();
        self.finish_refreshes();
    }

    #[cfg(test)]
    pub(super) fn finish_refreshes(&mut self) {
        let mut schedule = RefreshSchedule::new(Instant::now(), FALLBACK_INTERVAL);
        while self.session_refresh.idle.is_none() {
            let input = self.receiver.recv_timeout(Duration::from_secs(30)).unwrap();
            self.handle_inputs(input, &mut None, &mut schedule).unwrap();
        }
    }

    #[cfg(test)]
    pub(super) fn next_input(&mut self, timeout: Duration) -> Input {
        let deadline = Instant::now() + timeout;
        loop {
            let input = self
                .receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if let Input::Sessions {
                refresh,
                generation,
                result,
            } = input
            {
                self.finish_sessions(*refresh, generation, result).unwrap();
            } else {
                return input;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_burst_runs_once_then_runs_one_merged_request() {
        let home = tempfile::tempdir().unwrap();
        let mut app = App::new(Hub::new(Vec::new()), home.path().into());
        assert_eq!(app.request_refresh(), 1);
        for index in 0..50 {
            app.request_sessions(RefreshRequest::paths(vec![PathBuf::from(format!(
                "{index}.jsonl"
            ))]));
        }
        assert!(app.session_refresh.idle.is_none());
        assert!(
            matches!(&app.session_refresh.pending, Some(RefreshRequest::Paths(paths)) if paths.len() == 50)
        );
        app.request_refresh();
        assert_eq!(app.session_refresh.pending, Some(RefreshRequest::Full));
        let Input::Sessions {
            refresh,
            generation,
            result,
        } = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected the initial real Hub refresh");
        };
        assert_eq!(generation, 1);
        app.finish_sessions(*refresh, generation, result).unwrap();
        assert!(app.session_refresh.pending.is_none());
        assert!(app.session_refresh.idle.is_none());
        let Input::Sessions {
            refresh,
            generation,
            result,
        } = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected one merged Hub refresh");
        };
        assert_eq!(generation, 52);
        app.finish_sessions(*refresh, generation, result).unwrap();
        assert!(app.session_refresh.idle.is_some());
        assert!(app.session_refresh.pending.is_none());
        assert!(app.receiver.try_recv().is_err());
    }
}
