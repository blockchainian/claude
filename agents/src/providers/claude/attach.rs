//! Sends input through a temporary, screen-checked Claude attach client.

use crate::providers::{ScreenInput, SessionInput};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const DETACH_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn send(
    job_id: &str,
    cwd: &Path,
    settings_path: &Path,
    target: Option<&str>,
    input: SessionInput,
) -> io::Result<()> {
    validate_job_id(job_id)?;
    match &input {
        SessionInput::Prompt(text) => validate_text(text)?,
        SessionInput::PromptWithImages { text, images } => {
            validate_text(text)?;
            if images.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "No image attachments",
                ));
            }
            for path in images {
                image_path(path)?;
            }
        }
        SessionInput::Rename(_) | SessionInput::Answer { .. } => {
            return Err(io::ErrorKind::Unsupported.into());
        }
        SessionInput::SetModel(value) | SessionInput::SetEffort(value) => {
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid Claude setting",
                ));
            }
            validate_text(value)?;
        }
        SessionInput::Interrupt => {}
    }
    let restore = matches!(
        input,
        SessionInput::SetModel(_) | SessionInput::SetEffort(_)
    );
    let operation = || {
        let mut terminal = Terminal::spawn(job_id, cwd)?;
        let result = (|| {
            let original_state = terminal.current_view()?;
            original_state.target(original_state.viewed.as_deref())?;
            let original = original_state.viewed;
            let result = terminal.navigate(target).and_then(|()| {
                terminal.target = Some(target.map(str::to_owned));
                terminal.deliver(input)
            });
            let restored = terminal.navigate(original.as_deref());
            match (result, restored) {
                (Err(error), Err(restore)) => Err(io::Error::other(format!(
                    "{error}; restore view failed: {restore}"
                ))),
                (Err(error), _) => Err(error),
                (Ok(()), result) => result,
            }
        })();
        let detach = terminal.detach();
        result.and(detach)
    };
    if restore {
        super::settings::with_restored_defaults(settings_path, operation)
    } else {
        operation()
    }
}

/// Captures a confirmed input screen without sending any session input.
#[cfg(test)]
pub(super) fn inspect(job_id: &str, cwd: &Path) -> io::Result<String> {
    let mut terminal = Terminal::spawn(job_id, cwd)?;
    let result = terminal
        .wait_at("inspect-ready", |screen| {
            ready(screen) && view_state(screen).is_some()
        })
        .map(|()| terminal.screen());
    let detached = terminal.detach();
    match result {
        Ok(screen) => detached.map(|()| screen),
        Err(error) => Err(error),
    }
}

/// Selects a view through the same screen-checked navigation used for delivery.
#[cfg(test)]
pub(super) fn select_view(job_id: &str, cwd: &Path, target: Option<&str>) -> io::Result<()> {
    let mut terminal = Terminal::spawn(job_id, cwd)?;
    let result = terminal.navigate(target);
    let detached = terminal.detach();
    result.and(detached)
}

/// Leaves `text` in Claude's input without submitting it.
#[cfg(test)]
pub(super) fn type_input(job_id: &str, cwd: &Path, text: &str) -> io::Result<()> {
    let mut terminal = Terminal::spawn(job_id, cwd)?;
    let result = terminal.wait_at("type-ready", ready).and_then(|()| {
        terminal.paste(text)?;
        terminal.wait_at("typed", left_input)
    });
    let detached = terminal.detach();
    result.and(detached)
}

/// Clears text left in Claude's input, such as the prompt Claude restores after an interrupt,
/// with Ctrl+U only: Esc or Ctrl+C would end the running turn and any pending AskUserQuestion.
pub(super) fn clear_left_input(terminal: &mut dyn ScreenInput) -> io::Result<()> {
    let mut screen = terminal.wait_for("initial-screen", &|screen| {
        ready(screen) || left_input(screen)
    })?;
    while left_input(&screen) {
        let before = editor_text(&screen);
        // Ctrl+U deletes the text before the cursor on its line, then the line break.
        terminal.write(b"\x15")?;
        // Claude redraws the box in steps, so a frame without the box is not a result.
        screen = terminal.wait_for("input-cleared", &|screen| {
            input_box(screen).is_some() && editor_text(screen) != before
        })?;
    }
    Ok(())
}

pub(super) fn validate_job_id(job_id: &str) -> io::Result<()> {
    if job_id.is_empty()
        || job_id.starts_with('-')
        || Path::new(job_id).file_name() != Some(std::ffi::OsStr::new(job_id))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Claude job id",
        ));
    }
    Ok(())
}

fn input_lock_path(config_dir: &Path, job_id: &str) -> std::path::PathBuf {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    config_dir.hash(&mut hash);
    std::env::temp_dir()
        .join("agents-attach-locks")
        .join(format!("{:016x}", hash.finish()))
        .join(format!("{job_id}.lock"))
}

pub(super) fn validate_text(text: &str) -> io::Result<()> {
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Input contains terminal control characters",
        ));
    }
    Ok(())
}

/// A separate bracketed paste makes Claude load the file as an image attachment.
fn image_path(path: &Path) -> io::Result<String> {
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Image path is not valid UTF-8")
    })?;
    if text.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid image path",
        ));
    }
    let path = path.canonicalize()?;
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Image path is not a file",
        ));
    }
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Image path is not valid UTF-8")
    })?;
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

fn editor_text(screen: &str) -> String {
    let Some((row, _)) = prompt(screen) else {
        return String::new();
    };
    screen
        .lines()
        .skip(row)
        .take_while(|line| !line.trim_start().starts_with('─'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn image_visible(screen: &str, count: usize) -> bool {
    editor_text(screen)
        .split("[Image #")
        .skip(1)
        .filter(|suffix| {
            suffix.split_once(']').is_some_and(|(number, _)| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .count()
        == count
}

#[derive(Default)]
struct SynchronizedOutput {
    active: bool,
}

impl vt100::Callbacks for SynchronizedOutput {
    fn unhandled_csi(
        &mut self,
        _: &mut vt100::Screen,
        first: Option<u8>,
        second: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        if first == Some(b'?') && second.is_none() && params.iter().any(|param| *param == [2026]) {
            match command {
                'h' => self.active = true,
                'l' => self.active = false,
                _ => {}
            }
        }
    }
}

/// Defers screen checks until Claude finishes its synchronized redraw.
struct OutputScreen {
    parser: vt100::Parser<SynchronizedOutput>,
}

impl OutputScreen {
    fn new() -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(50, 200, 0, SynchronizedOutput::default()),
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    fn complete(&self) -> Option<String> {
        (!self.parser.callbacks().active).then(|| self.contents())
    }
}

struct Terminal {
    pty: crate::pty::Pty,
    parser: OutputScreen,
    // Attach clients share Claude's cursor and editor. Hold this through detach so
    // page inspection cannot navigate while another client is answering.
    _input_lock: File,
    target: Option<Option<String>>,
    #[cfg(test)]
    raw: Vec<u8>,
}

impl Terminal {
    fn spawn(job_id: &str, cwd: &Path) -> io::Result<Self> {
        validate_job_id(job_id)?;
        let lock_path = input_lock_path(&super::config_dir(), job_id);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(lock_path.parent().expect("input lock directory"))?;
        let input_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)?;
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            match input_lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Claude input is busy in another dashboard",
                    ));
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        let mut command = Command::new("claude");
        command.args(["attach", job_id]).current_dir(cwd);
        let pty = crate::pty::Pty::spawn(command, 50, 200)?;
        Ok(Self {
            pty,
            parser: OutputScreen::new(),
            _input_lock: input_lock,
            target: None,
            #[cfg(test)]
            raw: Vec::new(),
        })
    }

    fn screen(&self) -> String {
        self.parser.contents()
    }

    fn unexpected(&self, reason: &str) -> io::Error {
        screen_error(reason, &self.screen())
    }

    fn diagnostic(&self, stage: &str, error: io::Error) -> io::Error {
        #[cfg(test)]
        {
            let directory = std::env::temp_dir().join("agents-stage3-live");
            let name = format!("driver-{}-{stage}", self.pty.pid());
            let saved = std::fs::create_dir_all(&directory)
                .and_then(|()| {
                    std::fs::write(directory.join(format!("{name}.screen.txt")), self.screen())
                })
                .and_then(|()| std::fs::write(directory.join(format!("{name}.raw")), &self.raw));
            if let Err(diagnostic_error) = saved {
                return io::Error::new(
                    error.kind(),
                    format!("{stage}: {error}; could not save diagnostics: {diagnostic_error}"),
                );
            }
        }
        io::Error::new(error.kind(), format!("{stage}: {error}"))
    }

    fn wait_at(&mut self, stage: &str, predicate: impl Fn(&str) -> bool) -> io::Result<()> {
        self.wait(predicate)
            .map_err(|error| self.diagnostic(stage, error))
    }

    fn wait(&mut self, predicate: impl Fn(&str) -> bool) -> io::Result<()> {
        self.wait_allowing(predicate, None)
    }

    fn wait_allowing(
        &mut self,
        predicate: impl Fn(&str) -> bool,
        allowed_dialog: Option<&str>,
    ) -> io::Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(screen) = self.parser.complete() {
                if predicate(&screen) {
                    return Ok(());
                }
                if let Some(dialog) = known_dialog(&screen)
                    && !allowed_dialog.is_some_and(|marker| dialog.text.contains(marker))
                {
                    return Err(dialog_error(&dialog));
                }
            }
            if self.pty.exited()? {
                return Err(self.unexpected("Claude attach exited"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.unexpected("Claude attach timed out"));
            }
            self.read(remaining.min(Duration::from_millis(100)))?;
        }
    }

    fn read(&mut self, timeout: Duration) -> io::Result<()> {
        let mut bytes = Vec::new();
        if !self.pty.read(timeout, &mut bytes)? {
            return Err(self.unexpected("Claude attach closed its terminal"));
        }
        #[cfg(test)]
        self.raw.extend_from_slice(&bytes);
        self.parser.process(&bytes);
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pty.write(bytes)
    }

    fn confirm_target(&mut self) -> io::Result<()> {
        self.read(Duration::ZERO)?;
        self.wait_at("target-screen-complete", |screen| {
            view_state(screen).is_some()
        })?;
        if let Some(target) = &self.target
            && !view_state(&self.screen())
                .is_some_and(|state| state.focus == Focus::Input && state.viewed == *target)
        {
            return Err(self.unexpected("Claude input target or focus changed"));
        }
        Ok(())
    }

    fn paste(&mut self, text: &str) -> io::Result<()> {
        self.confirm_target()?;
        self.write(b"\x1b[200~")?;
        self.write(text.as_bytes())?;
        self.write(b"\x1b[201~")
    }

    fn submit(&mut self, text: &str) -> io::Result<String> {
        let before = editor_text(&self.screen());
        self.paste(text)?;
        // Observe the paste before Enter, so the unchanged empty prompt cannot acknowledge it.
        self.wait_at("paste-visible", |screen| {
            paste_visible(screen) && editor_text(screen) != before
        })?;
        let screen = self.screen();
        let pasted_input = prompt(&screen)
            .ok_or_else(|| self.unexpected("Claude input disappeared before submission"))?
            .1
            .to_owned();
        self.confirm_target()?;
        self.write(b"\r")?;
        Ok(pasted_input)
    }

    fn deliver(&mut self, input: SessionInput) -> io::Result<()> {
        clear_left_input(self)?;
        self.wait_at("initial-ready", ready)?;
        match input {
            SessionInput::Prompt(text) => {
                if text.is_empty() {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "Empty prompt"));
                }
                let pasted_input = self.submit(&text)?;
                self.wait_at("submission-accepted", |screen| {
                    prompt_accepted(screen, &pasted_input)
                })
            }
            SessionInput::PromptWithImages { text, images } => {
                for (index, path) in images.iter().enumerate() {
                    self.paste(&image_path(path)?)?;
                    self.wait_at("image-attached", |screen| image_visible(screen, index + 1))?;
                }
                let pasted_input = if text.is_empty() {
                    let pasted = prompt(&self.screen())
                        .ok_or_else(|| self.unexpected("Claude image input disappeared"))?
                        .1
                        .to_owned();
                    self.confirm_target()?;
                    self.write(b"\r")?;
                    pasted
                } else {
                    self.submit(&text)?
                };
                self.wait_at("image-submission-accepted", |screen| {
                    prompt_accepted(screen, &pasted_input)
                })
            }
            SessionInput::Interrupt => self.write(b"\x1b"),
            SessionInput::Rename(_) | SessionInput::Answer { .. } => {
                Err(io::ErrorKind::Unsupported.into())
            }
            SessionInput::SetModel(model) => {
                let choices = super::models::input_choices(&super::config_dir(), None);
                let label = choices
                    .model(&model)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "Unknown Claude model")
                    })?
                    .label
                    .clone();
                self.submit(&format!("/model {model}"))?;
                self.wait_at("model-confirmation", |screen| {
                    dialog_shows(screen, "Switch model?") || model_selected(screen, &label)
                })?;
                if !dialog_shows(&self.screen(), "Switch model?") {
                    return Ok(());
                }
                self.write(b"\r")?;
                // The parsed confirmation remains visible until Claude redraws after Enter.
                self.wait_allowing(
                    |screen| {
                        !dialog_shows(screen, "Switch model?") && model_selected(screen, &label)
                    },
                    Some("Switch model?"),
                )
                .map_err(|error| self.diagnostic("model-confirmation-cleared", error))
            }
            SessionInput::SetEffort(effort) => {
                const CONFIRMATION: &str = "Your next response will be slower and use more tokens";
                self.submit(&format!("/effort {effort}"))?;
                self.wait_at("effort-confirmation", |screen| {
                    actual_empty(screen)
                        || (dialog_shows(screen, CONFIRMATION)
                            && dialog_shows(screen, &format!("Yes, switch to {effort}")))
                })?;
                if !dialog_shows(&self.screen(), CONFIRMATION) {
                    return Ok(());
                }
                self.write(b"\r")?;
                self.wait_allowing(actual_empty, Some(CONFIRMATION))
                    .map_err(|error| self.diagnostic("effort-confirmation-cleared", error))
            }
        }
    }

    fn detach(&mut self) -> io::Result<()> {
        self.pty.close(Some(b"\x1a"), DETACH_TIMEOUT)
    }
}

impl ScreenInput for Terminal {
    fn wait_for(&mut self, stage: &str, ready: &dyn Fn(&str) -> bool) -> io::Result<String> {
        self.wait_at(stage, ready)?;
        Ok(self.screen())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pty.write(bytes)
    }
}

fn screen_error(reason: &str, screen: &str) -> io::Error {
    let mut lines: Vec<String> = screen
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.chars().all(|ch| ch == '─'))
        .rev()
        .take(4)
        .map(|line| line.chars().take(160).collect())
        .collect();
    lines.reverse();
    let summary = if lines.is_empty() {
        "empty screen".into()
    } else {
        lines.join(" | ")
    };
    io::Error::other(format!("{reason}; screen: {summary}"))
}

fn prompt(screen: &str) -> Option<(usize, &str)> {
    let lines: Vec<_> = screen.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(row, line)| {
            let text = line.trim_start().strip_prefix('❯')?;
            let editor = row
                .checked_sub(1)
                .and_then(|above| lines.get(above))
                .is_some_and(|line| line.trim_start().starts_with('─'));
            (editor || !text.trim_start().starts_with(['⏺', '◯'])).then_some((row, text))
        })
        .next_back()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptState<'a> {
    Empty,
    Queued,
    Editing(&'a str),
}

fn prompt_state(screen: &str) -> Option<PromptState<'_>> {
    prompt(screen).map(|(_, text)| match text.trim() {
        "" => PromptState::Empty,
        // Claude renders this placeholder over an empty editor after accepting a busy prompt.
        "Press up to edit queued messages" => PromptState::Queued,
        text if text.starts_with("Message @") && text.ends_with('…') => PromptState::Empty,
        text => PromptState::Editing(text),
    })
}

fn ready(screen: &str) -> bool {
    matches!(
        prompt_state(screen),
        Some(PromptState::Empty | PromptState::Queued)
    )
}

/// The state of Claude's own input box, which opens below a rule; a dialog's `❯` selection
/// cursor and a transcript prompt are not the box.
fn input_box(screen: &str) -> Option<PromptState<'_>> {
    let (row, _) = prompt(screen)?;
    row.checked_sub(1)
        .and_then(|above| screen.lines().nth(above))
        .is_some_and(|line| line.trim_start().starts_with('─'))
        .then(|| prompt_state(screen))
        .flatten()
}

fn left_input(screen: &str) -> bool {
    matches!(input_box(screen), Some(PromptState::Editing(_)))
}

fn model_selected(screen: &str, label: &str) -> bool {
    if input_box(screen) != Some(PromptState::Empty) {
        return false;
    }
    let header = screen.lines().take(5).any(|line| {
        line.split_once(" · ").is_some_and(|(model, _)| {
            let model = model.trim_start_matches(|ch: char| !ch.is_alphanumeric());
            model == label
                || model
                    .rsplit_once(" with ")
                    .is_some_and(|(model, effort)| model == label && effort.ends_with(" effort"))
        })
    });
    // Long transcripts scroll the header away; their status line is below the editor.
    let row = prompt(screen).expect("empty input box has a prompt").0;
    let footer = screen.lines().skip(row + 2).any(|line| {
        let model = line.trim().split(" · ").next().unwrap_or("");
        model == label
            || model
                .rsplit_once(' ')
                .is_some_and(|(model, _effort)| model == label)
    });
    header || footer
}

fn actual_empty(screen: &str) -> bool {
    prompt_state(screen) == Some(PromptState::Empty)
}

fn paste_visible(screen: &str) -> bool {
    matches!(prompt_state(screen), Some(PromptState::Editing(_)))
}

fn prompt_accepted(screen: &str, pasted_input: &str) -> bool {
    match prompt_state(screen) {
        Some(PromptState::Empty) => true,
        Some(PromptState::Queued) => {
            prompt(screen).is_some_and(|(_, input)| input.trim() != pasted_input.trim())
        }
        Some(PromptState::Editing(_)) | None => false,
    }
}

/// The block Claude draws in place of its input box to ask something: a `▔` border line, a title,
/// then the body and options.
struct Dialog<'a> {
    title: &'a str,
    text: String,
}

/// Chat text above the block can mention any dialog string, so only the block counts. A frame
/// with a border but no title below it is a redraw in progress.
fn dialog(screen: &str) -> Option<Dialog<'_>> {
    if input_box(screen).is_some() {
        return None;
    }
    let lines: Vec<&str> = screen.lines().collect();
    let border = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with('▔'))?;
    let block = &lines[border + 1..];
    let title = block
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())?;
    Some(Dialog {
        title,
        text: block.join("\n"),
    })
}

fn dialog_shows(screen: &str, text: &str) -> bool {
    dialog(screen).is_some_and(|dialog| dialog.text.contains(text))
}

fn known_dialog(screen: &str) -> Option<Dialog<'_>> {
    const MARKERS: [&str; 9] = [
        "Do you trust",
        "Workspace not trusted",
        "Do you want to proceed?",
        "Allow once",
        "Yes, allow",
        "Enter to confirm",
        "Switch model?",
        "Sign in",
        "Select login method",
    ];
    dialog(screen).filter(|dialog| MARKERS.iter().any(|marker| dialog.text.contains(marker)))
}

fn dialog_error(dialog: &Dialog) -> io::Error {
    let body: Vec<String> = dialog
        .text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != dialog.title)
        .map(|line| line.chars().take(160).collect())
        .collect();
    io::Error::other(format!(
        "Claude attach needs a response; dialog: {} | {}",
        dialog.title,
        body.join(" | ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_lock_is_outside_claude_config_and_is_scoped_by_config_and_job() {
        let first = Path::new("/config/first");
        let path = input_lock_path(first, "job");
        assert!(!path.starts_with(first));
        assert!(path.starts_with(std::env::temp_dir().join("agents-attach-locks")));
        assert_eq!(path, input_lock_path(first, "job"));
        assert_ne!(path, input_lock_path(Path::new("/config/second"), "job"));
        assert_ne!(path, input_lock_path(first, "other"));
    }

    #[test]
    fn captured_model_header_includes_the_selected_effort() {
        let screen =
            include_str!("../../../tests/fixtures/claude-attach/model-effort-header.screen.txt");
        assert!(model_selected(screen, "Sonnet 5"));
        assert!(!model_selected(screen, "Sonnet 5.5"));
        assert!(!model_selected(screen, "Opus 5.5"));
    }

    #[test]
    #[ignore = "starts a real Claude background session and restores global model defaults"]
    fn real_model_command_without_confirmation() {
        struct Cleanup {
            cwd: tempfile::TempDir,
            session: String,
            job: Option<String>,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let mut errors = Vec::new();
                if let Some(job) = &self.job {
                    for command in ["stop", "rm"] {
                        match Command::new("claude")
                            .args([command, job])
                            .current_dir(self.cwd.path())
                            .output()
                        {
                            Ok(output) if output.status.success() => {}
                            Ok(output) => errors.push(format!(
                                "cleanup {command}: {}",
                                String::from_utf8_lossy(&output.stderr)
                            )),
                            Err(error) => errors.push(format!("cleanup {command}: {error}")),
                        }
                    }
                }
                let delete = || -> io::Result<()> {
                    let projects =
                        match std::fs::read_dir(super::super::config_dir().join("projects")) {
                            Ok(projects) => projects,
                            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                            Err(error) => return Err(error),
                        };
                    for project in projects {
                        let transcript = project?.path().join(format!("{}.jsonl", self.session));
                        match super::super::delete(&transcript) {
                            Ok(()) => {}
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error),
                        }
                    }
                    Ok(())
                };
                if let Err(error) = delete() {
                    errors.push(format!("cleanup transcript: {error}"));
                }
                if !errors.is_empty() {
                    let message = errors.join("\n");
                    if std::thread::panicking() {
                        eprintln!("{message}");
                    } else {
                        panic!("{message}");
                    }
                }
            }
        }
        let mut cleanup = Cleanup {
            cwd: tempfile::tempdir().unwrap(),
            session: uuid::Uuid::new_v4().to_string(),
            job: None,
        };
        let cwd = cleanup.cwd.path().to_owned();
        let output = Command::new("claude")
            .args([
                "--bg",
                "--session-id",
                &cleanup.session,
                "--model",
                "haiku",
                "--setting-sources",
                "",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--",
                "Reply exactly READY. Do not use tools.",
            ])
            .current_dir(&cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let job = text
            .lines()
            .find_map(|line| line.split("backgrounded · ").nth(1))
            .expect("background job id")
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned();
        cleanup.job = Some(job.clone());
        let result = std::panic::catch_unwind(|| {
            let settings = super::super::config_dir().join("settings.json");
            super::super::settings::with_restored_defaults(&settings, || {
                let mut terminal = Terminal::spawn(&job, &cwd)?;
                let invalid = terminal
                    .deliver(SessionInput::SetModel("agents-nonexistent-model".into()))
                    .unwrap_err();
                assert_eq!(invalid.kind(), io::ErrorKind::InvalidInput);
                let result = terminal.deliver(SessionInput::SetModel("claude-opus-5-5".into()));
                let directory = cwd.join("scratchpad/model-confirmation");
                std::fs::create_dir_all(&directory)?;
                std::fs::write(directory.join("model.raw"), &terminal.raw)?;
                std::fs::write(directory.join("model.screen.txt"), terminal.screen())?;
                terminal.detach()?;
                drop(terminal);
                result?;
                let screen = inspect(&job, &cwd)?;
                std::fs::write(directory.join("reattached.screen.txt"), &screen)?;
                assert!(screen.contains("Opus 5.5"), "{screen}");
                Ok(())
            })
            .unwrap();
        });
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn captured_long_session_confirms_the_model_below_the_editor() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/model-footer.screen.txt");
        assert!(model_selected(screen, "Opus 5.5"));
        assert!(!model_selected(screen, "Fable 5.1"));
        assert!(!model_selected(screen, "Opus 5"));
    }

    #[test]
    fn captured_model_command_completes_without_a_confirmation_dialog() {
        let raw = include_bytes!("../../../tests/fixtures/claude-attach/model-command.raw");
        let mut screen = OutputScreen::new();
        screen.process(raw);
        let screen = screen.complete().expect("completed model command redraw");
        assert!(screen.contains("Opus 5.5"), "{screen}");
        assert!(!screen.contains("Switch model?"), "{screen}");
        assert!(dialog(&screen).is_none(), "{screen}");
        assert!(model_selected(&screen, "Opus 5.5"), "{screen}");
        assert!(!model_selected(&screen, "Fable 5.1"), "{screen}");
    }

    #[test]
    fn chat_text_with_dialog_markers_is_not_a_dialog() {
        let screen = include_str!(
            "../../../tests/fixtures/claude-attach/chat-text-with-dialog-markers.screen.txt"
        );
        assert!(ready(screen), "{screen}");
        assert!(dialog(screen).is_none(), "{screen}");
        assert!(known_dialog(screen).is_none(), "{screen}");
    }

    #[test]
    fn captured_switch_model_dialog_is_found_by_its_border_and_title() {
        let screen =
            include_str!("../../../tests/fixtures/claude-attach/switch-model-dialog.screen.txt");
        let dialog = dialog(screen).expect("dialog block");
        assert_eq!(dialog.title, "Switch model?");
        assert!(dialog.text.contains("No, go back"), "{}", dialog.text);
        assert!(!dialog.text.contains("Crunched"), "{}", dialog.text);
        assert!(known_dialog(screen).is_some());
        assert!(!ready(screen));
        let error = dialog_error(&known_dialog(screen).unwrap()).to_string();
        assert!(
            error.contains("Switch model? | Your next response"),
            "{error}"
        );
        assert!(error.contains("2. No, go back"), "{error}");
    }

    #[test]
    fn captured_effort_dialog_is_found_even_when_its_border_carries_text() {
        let screen =
            include_str!("../../../tests/fixtures/claude-attach/change-effort-dialog.screen.txt");
        let dialog = dialog(screen).expect("dialog block");
        assert_eq!(dialog.title, "Change effort level?");
        assert!(dialog_shows(screen, "Yes, switch to high"));
        assert!(dialog_shows(
            screen,
            "Your next response will be slower and use more tokens"
        ));
    }

    #[test]
    fn border_without_a_title_is_a_redraw_in_progress() {
        let screen = "Done\n▔▔▔▔▔▔▔▔▔▔▔▔\n\n";
        assert!(dialog(screen).is_none());
    }

    #[test]
    fn dialog_text_mentioned_above_the_dialog_is_not_the_dialog() {
        let screen =
            include_str!("../../../tests/fixtures/claude-attach/switch-model-dialog.screen.txt")
                .replace(
                    "Switch model? Allow once.",
                    "Allow once. Enter to confirm. Sign in",
                );
        assert_eq!(dialog(&screen).unwrap().title, "Switch model?");
        assert!(!dialog_shows(&screen, "Sign in"));
    }

    #[test]
    fn captured_question_redraw_waits_for_synchronized_output_end() {
        let raw = include_bytes!("../../../tests/fixtures/claude-attach/synchronized-question.raw");
        let end = raw
            .windows(8)
            .position(|bytes| bytes == b"\x1b[?2026l")
            .unwrap();
        for chunk_size in [1, 7, 37, 512] {
            let mut screen = OutputScreen::new();
            for bytes in raw[..end + 7].chunks(chunk_size) {
                screen.process(bytes);
            }
            assert!(screen.contents().contains("First"));
            assert!(screen.contents().contains("Enter to select"));
            assert!(
                screen.complete().is_none(),
                "incomplete synchronized redraw accepted with {chunk_size}-byte chunks"
            );
            screen.process(&raw[end + 7..]);
            let complete = screen.complete().expect("completed synchronized redraw");
            assert!(complete.contains("First"));
            assert!(complete.contains("Second"));
        }
    }

    #[test]
    fn captured_queue_placeholder_accepts_another_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        let sent_text = screen
            .lines()
            .find_map(|line| line.strip_prefix("❯ Steer"))
            .unwrap();
        assert!(prompt_accepted(screen, sent_text));
        assert!(ready(screen));
    }

    #[test]
    fn captured_queue_placeholder_is_not_a_visible_paste() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        assert!(!paste_visible(screen));
    }

    #[test]
    fn captured_queue_placeholder_does_not_acknowledge_a_setting_change() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        assert!(!actual_empty(screen));
    }

    #[test]
    fn captured_queue_hint_does_not_acknowledge_unchanged_pasted_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        let (_, unchanged_input) = prompt(screen).unwrap();
        assert!(!prompt_accepted(screen, unchanged_input));
    }

    #[test]
    fn captured_historical_queue_hint_does_not_acknowledge_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        let submitted = screen
            .lines()
            .find(|line| line.starts_with("❯ Steer"))
            .unwrap();
        let with_pending_input = format!("{screen}\n{submitted}");
        assert!(!prompt_accepted(&with_pending_input, submitted));
    }

    #[test]
    fn captured_terminal_output_contains_ready_before_detaching() {
        let raw = include_bytes!("../../../tests/fixtures/claude-attach/attach.raw");
        let mut parser = OutputScreen::new();
        let mut saw_ready = false;
        for chunk in raw.chunks(37) {
            parser.process(chunk);
            saw_ready |= parser.complete().is_some_and(|screen| ready(&screen));
        }
        let screen = parser.complete().expect("completed terminal output");
        assert!(
            saw_ready,
            "No empty input appeared in the real terminal capture: {screen}"
        );
        assert!(screen.contains("Resume this session with:"), "{screen}");
        assert!(!ready(&screen));
    }

    #[test]
    fn captured_interrupted_prompt_is_left_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/interrupted.screen.txt");
        assert!(left_input(screen), "{screen}");
        assert!(!ready(screen));
    }

    #[test]
    fn captured_idle_and_busy_screens_have_no_left_input() {
        for screen in [
            include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt"),
            include_str!("../../../tests/fixtures/claude-attach/busy.screen.txt"),
            include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt"),
        ] {
            assert!(!left_input(screen), "{screen}");
        }
    }

    #[test]
    fn captured_question_cursor_is_not_left_input() {
        let raw = include_bytes!("../../../tests/fixtures/claude-attach/synchronized-question.raw");
        let mut parser = OutputScreen::new();
        parser.process(raw);
        let screen = parser.complete().expect("completed question screen");
        assert!(prompt(&screen).is_some(), "{screen}");
        assert!(!left_input(&screen), "{screen}");
        // A paste and Enter on a dialog would select its option.
        assert!(!ready(&screen), "{screen}");
    }

    #[test]
    fn captured_idle_and_busy_screens_accept_input() {
        for screen in [
            include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt"),
            include_str!("../../../tests/fixtures/claude-attach/busy.screen.txt"),
        ] {
            assert!(ready(screen), "{screen}");
        }
    }

    #[test]
    fn captured_empty_input_without_footer_is_ready() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt");
        let (row, _) = prompt(screen).unwrap();
        let without_footer = screen.lines().take(row + 1).collect::<Vec<_>>().join("\n");
        assert!(ready(&without_footer));
    }

    #[test]
    fn captured_transcript_prompt_is_not_an_input_screen() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt");
        let (row, _) = prompt(screen).unwrap();
        let before_input = screen.lines().take(row).collect::<Vec<_>>().join("\n");
        assert!(!ready(&before_input));
    }

    #[test]
    fn prompt_rejects_terminal_escape_injection() {
        assert!(validate_text("hello\u{1b}[201~\r/model opus").is_err());
        assert!(validate_text("hello\u{1a}").is_err());
    }

    #[test]
    fn prompt_accepts_multiline_unicode_text() {
        assert!(validate_text("第一行\nsecond line\t✓").is_ok());
    }

    #[test]
    fn timeout_reports_captured_editor_and_footer_in_one_line() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt");
        let message = screen_error("Claude attach timed out", screen).to_string();
        assert!(message.starts_with("Claude attach timed out; screen: "));
        assert!(message.contains("❯"), "{message}");
        assert!(message.contains("auto mode on"), "{message}");
        assert!(!message.contains("Claude Code v2.1.299"), "{message}");
        assert_eq!(message.lines().count(), 1);
    }

    #[test]
    fn screen_error_reports_empty_screen() {
        assert_eq!(
            screen_error("Claude attach timed out", "\n  \n").to_string(),
            "Claude attach timed out; screen: empty screen"
        );
    }
}

#[cfg(test)]
#[path = "attach_input_tests.rs"]
mod input_tests;

#[cfg(test)]
#[path = "attach_image_tests.rs"]
mod image_tests;

#[path = "attach_view.rs"]
mod view;
use view::*;
