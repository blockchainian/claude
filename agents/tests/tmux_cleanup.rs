#[path = "support/tmux.rs"]
mod tmux;

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tmux::Tmux;

#[test]
#[ignore = "subprocess fixture for the cleanup tests"]
fn probe_owner() {
    let name = std::env::var("AGENTS_TMUX_CLEANUP_SOCKET").unwrap();
    let ready = std::env::var_os("AGENTS_TMUX_CLEANUP_READY").unwrap();
    let tmux = Tmux::new(name);
    tmux.run(&["new-session", "-d", "-s", "probe", "sleep 60"]);
    std::fs::write(ready, "ready").unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn tmux_server_exits_after_owner_sigkill() {
    owner_exit(libc::SIGKILL);
}

#[test]
fn tmux_server_exits_after_owner_sigterm() {
    owner_exit(libc::SIGTERM);
}

struct Owner(Child);

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}

fn owner_exit(signal: i32) {
    use std::os::unix::process::ExitStatusExt;

    let name = format!("agents-owner-{signal}-{}", std::process::id());
    // Also clean up after a failed assertion, including the regression's red run.
    let _cleanup = Tmux::new(name.clone());
    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("ready");
    let mut owner = Owner(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "probe_owner", "--ignored", "--nocapture"])
            .env("AGENTS_TMUX_CLEANUP_SOCKET", &name)
            .env("AGENTS_TMUX_CLEANUP_READY", &ready)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(owner.0.try_wait().unwrap().is_none(), "probe owner exited");
        assert!(
            Instant::now() < deadline,
            "probe owner did not become ready"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(server_exists(&name), "probe never started");
    // SAFETY: this is our live, unreaped child; its PID cannot be reused yet.
    assert_eq!(unsafe { libc::kill(owner.0.id() as i32, signal) }, 0);
    let status = owner.0.wait().unwrap();
    assert_eq!(status.signal(), Some(signal));
    let deadline = Instant::now() + Duration::from_secs(6);
    while server_exists(&name) {
        assert!(
            Instant::now() < deadline,
            "tmux server survived owner signal {signal}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn server_exists(name: &str) -> bool {
    Command::new("tmux")
        .args(["-L", name, "has-session", "-t", "probe"])
        .output()
        .unwrap()
        .status
        .success()
}
