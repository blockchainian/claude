mod activity;
mod app;
mod ask;
mod clipboard;
mod images;
mod jsonl;
mod log;
mod model;
mod processes;
mod providers;
mod refresh;
mod repository;
mod sessions;
mod shell_summary;
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
