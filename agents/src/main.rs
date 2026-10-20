mod activity;
mod app;
mod ask;
mod clipboard;
mod codex_await_output;
mod images;
mod jsonl;
mod log;
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
mod terminal_background;
mod theme;
mod ui;
mod usage;

fn main() {
    let result = providers::configured()
        .and_then(|(hub, preferences_dir)| app::App::new(hub, preferences_dir).run());
    if let Err(error) = result {
        eprintln!("agents: {error}");
        std::process::exit(1);
    }
}
