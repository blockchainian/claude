use super::*;
use std::os::unix::fs::MetadataExt;

/// Archives a thread loaded by this terminal only if its rollout contains no further turn.
pub(super) fn cleanup(home: &Path, session: &Session) -> io::Result<Option<SessionAction>> {
    if session.status != SessionStatus::Inactive || session.process.is_some() {
        return Ok(None);
    }
    let path = session
        .transcript
        .as_deref()
        .ok_or_else(|| io::Error::other("Session has no transcript"))?;
    // Keep the file open: unarchiving moves it out of archived_sessions.
    let mut file = File::open(path)?;
    let offset = crate::jsonl::complete_end(&mut file)?;
    let mut boundary = crate::jsonl::Boundary::default();
    boundary.capture(&mut file, offset)?;
    let home = home.to_owned();
    let id = session.id.id.clone();
    Ok(Some(Box::new(move || {
        let metadata = file.metadata()?;
        if metadata.nlink() == 0 || metadata.len() < offset || !boundary.matches(&mut file)? {
            return Ok(());
        }
        let mut started = false;
        let mut invalid = None;
        let end =
            crate::jsonl::read_lines(&mut file, offset, |line| {
                match serde_json::from_slice::<serde_json::Value>(line) {
                    Ok(record) => {
                        started |= record["type"] == "event_msg"
                            && record["payload"]["type"] == "task_started";
                    }
                    Err(error) => invalid = Some(error),
                }
            })?;
        if let Some(error) = invalid {
            return Err(io::Error::new(io::ErrorKind::InvalidData, error));
        }
        if started || end != file.metadata()?.len() {
            return Ok(());
        }
        let mut client = app_server::Client::connect(&home)?;
        // Revalidate before archive, which would otherwise interrupt a running turn.
        if app_server::thread::running_turn(&mut client, &id)?.is_none() {
            app_server::thread::archive(&mut client, &id)?;
            supervision::remove_thread_record(&supervision::state_dir()?, &id)?;
        }
        Ok(())
    })))
}
