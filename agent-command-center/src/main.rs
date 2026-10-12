mod app;
mod model;
mod repository;
mod sessions;
mod state;
mod terminal_background;
mod theme;
mod ui;

fn main() {
    let config_dir = sessions::config_dir();
    if !app::config_dir_exists(&config_dir) {
        eprintln!(
            "claude-agents: no Claude Code config directory at {}",
            config_dir.display()
        );
        std::process::exit(1);
    }
    if let Err(error) = app::App::new(config_dir).run() {
        eprintln!("claude-agents: {error}");
        std::process::exit(1);
    }
}
