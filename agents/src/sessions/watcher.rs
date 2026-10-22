//! Recursive notifications for provider directories.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Watches only the output file currently displayed in event details.
pub struct OutputWatcher {
    watcher: RecommendedWatcher,
    path: Arc<Mutex<Option<PathBuf>>>,
    requested: Option<PathBuf>,
}

impl OutputWatcher {
    pub fn new(
        mut changed: impl FnMut(notify::Result<notify::Event>) + Send + 'static,
    ) -> notify::Result<Self> {
        let path = Arc::new(Mutex::new(None::<PathBuf>));
        let selected = path.clone();
        let watcher =
            notify::recommended_watcher(
                move |result: notify::Result<notify::Event>| match result {
                    Ok(event) => {
                        let path = selected.lock().expect("output watcher path poisoned");
                        if path.as_ref().is_some_and(|path| {
                            !matches!(event.kind, EventKind::Access(_))
                                && (event.need_rescan()
                                    || event.paths.iter().any(|changed| changed == path))
                        }) {
                            changed(Ok(event));
                        }
                    }
                    Err(error) => changed(Err(error)),
                },
            )?;
        Ok(Self {
            watcher,
            path,
            requested: None,
        })
    }

    /// Watches the parent directory so an absent file can be created later.
    pub fn set_path(&mut self, path: Option<PathBuf>) -> notify::Result<bool> {
        if self.requested == path {
            return Ok(false);
        }
        self.requested = path.clone();
        let previous = self
            .path
            .lock()
            .expect("output watcher path poisoned")
            .take();
        if let Some(previous) = previous {
            self.watcher
                .unwatch(previous.parent().expect("output file has parent"))?;
        }
        let path = path
            .map(|path| {
                let parent = path
                    .parent()
                    .ok_or_else(|| notify::Error::generic("Output file has no parent directory"))?;
                let name = path
                    .file_name()
                    .ok_or_else(|| notify::Error::generic("Output file has no file name"))?;
                Ok::<_, notify::Error>(parent.canonicalize()?.join(name))
            })
            .transpose()?;
        if let Some(path) = &path {
            self.watcher.watch(
                path.parent().expect("output file has parent"),
                RecursiveMode::NonRecursive,
            )?;
        }
        *self.path.lock().expect("output watcher path poisoned") = path;
        Ok(true)
    }
}

pub fn watch(
    roots: &[PathBuf],
    mut changed: impl FnMut(notify::Result<notify::Event>) + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) if relevant(&event) => changed(Ok(event)),
            Ok(_) => {}
            Err(error) => changed(Err(error)),
        })?;
    for root in roots {
        if root.is_dir() {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }
    }
    Ok(watcher)
}

fn relevant(event: &notify::Event) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && (event.paths.is_empty()
            || event.paths.iter().any(|path| {
                path.file_name().is_none_or(|name| name != "agents.json")
                    // Manager inbox polling creates and removes these lock directories
                    // even without messages; their contents do not change session state.
                    && !path.components().any(|component| {
                        let name = component.as_os_str().to_string_lossy();
                        name.ends_with(".jsonl.lock") || name.ends_with(".json.lock")
                    })
                    && path
                        .extension()
                        .is_none_or(|extension| extension != "key" && extension != "tmp")
            }))
}

#[cfg(test)]
#[path = "../providers/claude/watcher_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "output_watcher_tests.rs"]
mod output_tests;
