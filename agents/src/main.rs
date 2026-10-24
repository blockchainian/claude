mod activity;
mod app;
mod ask;
mod clipboard;
mod event;
mod images;
mod jsonl;
mod model;
mod processes;
mod providers;
mod pty;
mod refresh;
mod repository;
mod session_terminal;
mod sessions;
mod state;
mod tasks;
mod terminal;
mod theme;
mod ui;
mod usage;

/// A real project folder outside any repository, for tests whose task directory must exist.
#[cfg(test)]
fn test_project() -> std::path::PathBuf {
    let project = std::env::temp_dir().join("agents-tests/project");
    std::fs::create_dir_all(&project).unwrap();
    project
}

fn main() {
    let result = providers::configured()
        .and_then(|(hub, preferences_dir)| app::App::new(hub, preferences_dir).run());
    if let Err(error) = result {
        eprintln!("agents: {error}");
        std::process::exit(1);
    }
}
