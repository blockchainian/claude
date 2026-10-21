//! Recursive notifications for provider directories.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;

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
