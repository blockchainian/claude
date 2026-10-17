use std::{
    io,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(super) struct ModifyOtherKeys(pub(super) bool);
impl crossterm::Command for ModifyOtherKeys {
    fn write_ansi(&self, writer: &mut impl std::fmt::Write) -> std::fmt::Result {
        writer.write_str(if self.0 { "\x1b[>4;2m" } else { "\x1b[>4;0m" })
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "modifyOtherKeys requires ANSI support",
        ))
    }
}

/// tmux only emits parseable modified Enter reports in its CSI-u format.
pub(super) fn enable_tmux_keys(writer: &mut impl io::Write) -> io::Result<()> {
    if std::env::var_os("TMUX").is_none() && std::env::var_os("TMUX_PANE").is_none() {
        return Ok(());
    }
    let mut command = Command::new("tmux");
    command.args(["display-message", "-p"]);
    if let Some(pane) = std::env::var_os("TMUX_PANE") {
        command.arg("-t").arg(pane);
    }
    command
        .arg("#{extended-keys-format}")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Older tmux versions lack this option. Preserve their normal key mode.
    let Ok(mut child) = command.spawn() else {
        return Ok(());
    };
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                if let Ok(output) = child.wait_with_output()
                    && output.status.success()
                    && output.stdout == b"csi-u\n"
                {
                    crossterm::execute!(writer, ModifyOtherKeys(true))?;
                }
                return Ok(());
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
        }
    }
}
