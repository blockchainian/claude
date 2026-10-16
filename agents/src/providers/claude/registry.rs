use crate::model::SessionStatus;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<i64>,
    pub proc_start: Option<String>,
    #[serde(rename = "version")]
    _version: Option<String>,
    pub kind: Option<String>,
    pub job_id: Option<String>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub updated_at: Option<i64>,
    pub status: Option<String>,
}

pub(super) fn background_jobs(config: &Path) -> HashMap<String, crate::model::BackgroundJob> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct State {
        session_id: String,
        template: String,
        needs: Option<String>,
    }
    let mut jobs = HashMap::new();
    for entry in fs::read_dir(config.join("jobs"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let Ok(bytes) = fs::read(entry.path().join("state.json")) else {
            continue;
        };
        let Ok(state) = serde_json::from_slice::<State>(&bytes) else {
            continue;
        };
        if state.template != "bg" || state.session_id.is_empty() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        jobs.insert(
            state.session_id,
            crate::model::BackgroundJob {
                id,
                needs: state.needs,
            },
        );
    }
    jobs
}

/// Maps the status strings written by Claude Code's session registry.
pub(super) fn from_registry(status: &str) -> Option<SessionStatus> {
    match status {
        "waiting" => Some(SessionStatus::NeedsInput),
        "busy" | "shell" => Some(SessionStatus::Working),
        "idle" => Some(SessionStatus::Ready),
        _ => None,
    }
}

pub(super) fn process_starts(pids: &[u32]) -> HashMap<u32, Vec<DateTime<Utc>>> {
    pids.iter()
        .filter_map(|&pid| crate::processes::start_time(pid).map(|start| (pid, vec![start])))
        .collect()
}

fn alive(entry: &Entry, starts: &HashMap<u32, Vec<DateTime<Utc>>>) -> bool {
    let Some(pid) = entry.pid.filter(|pid| *pid > 0 && *pid <= i32::MAX as u32) else {
        return false;
    };
    // Signal zero checks existence without sending a signal to the process.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
        return false;
    }
    let Some(actual) = starts.get(&pid) else {
        return true;
    };
    matches_start(entry, actual)
}

pub(super) fn matches_start(entry: &Entry, actual: &[DateTime<Utc>]) -> bool {
    let expected = entry.proc_start.as_deref().and_then(|start| {
        NaiveDateTime::parse_from_str(start, "%a %b %e %H:%M:%S %Y")
            .ok()
            .map(|time| time.and_utc())
    });
    expected.is_some_and(|time| {
        actual
            .iter()
            .any(|actual| actual.timestamp() == time.timestamp())
    })
}

pub(super) fn live(config_dir: &Path) -> Vec<Entry> {
    let entries: Vec<Entry> = fs::read_dir(config_dir.join("sessions"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| fs::read(path).ok())
        .filter_map(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
        .filter(|entry| entry.session_id.as_ref().is_some_and(|id| !id.is_empty()))
        .filter(|entry| {
            entry.kind.as_deref().is_none_or(|kind| {
                matches!(kind, "interactive" | "bg" | "daemon" | "daemon-worker")
            })
        })
        .collect();
    let pids: Vec<_> = entries.iter().filter_map(|entry| entry.pid).collect();
    let starts = process_starts(&pids);
    entries
        .into_iter()
        .filter(|entry| alive(entry, &starts))
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn fixture(dir: &Path) -> serde_json::Value {
        let pid = std::process::id();
        let starts = process_starts(&[pid]);
        let start = starts
            .get(&pid)
            .expect("libproc must read the process start time for the liveness test")[0];
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/registry.json")).unwrap();
        value["pid"] = pid.into();
        value["procStart"] = start.format("%a %b %e %H:%M:%S %Y").to_string().into();
        fs::create_dir_all(dir.join("sessions")).unwrap();
        value
    }

    #[test]
    fn stopped_background_job_matches_exact_state_session_id() {
        let directory = tempfile::tempdir().unwrap();
        let jobs = directory.path().join("jobs/job-distinct-from-session");
        fs::create_dir_all(&jobs).unwrap();
        // Safe fields copied from the real Claude Code 2.1.299 e772d00b state payload.
        let state = r#"{"state":"done","template":"bg","sessionId":"e772d00b-c424-4632-a8df-74bc9c7b1629","respawnFlags":[],"backend":"daemon"}"#;
        fs::write(jobs.join("state.json"), state).unwrap();
        let found = background_jobs(directory.path());
        let job = found.get("e772d00b-c424-4632-a8df-74bc9c7b1629").unwrap();
        assert_eq!(job.id, "job-distinct-from-session");
        assert_eq!(job.needs, None);
    }

    #[test]
    fn background_job_reads_needs_from_updated_real_state_payload() {
        let directory = tempfile::tempdir().unwrap();
        let jobs = directory.path().join("jobs/e772d00b");
        fs::create_dir_all(&jobs).unwrap();
        let mut state: serde_json::Value = serde_json::from_str(r#"{"state":"done","template":"bg","sessionId":"e772d00b-c424-4632-a8df-74bc9c7b1629","respawnFlags":[],"backend":"daemon"}"#).unwrap();
        state["needs"] = "send a prompt to start".into();
        fs::write(jobs.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
        let found = background_jobs(directory.path());
        assert_eq!(
            found["e772d00b-c424-4632-a8df-74bc9c7b1629"]
                .needs
                .as_deref(),
            Some("send a prompt to start")
        );
    }

    #[test]
    fn partial_registry_is_skipped_until_a_complete_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let value = fixture(dir.path());
        let bytes = serde_json::to_vec(&value).unwrap();
        let valid_path = dir.path().join("sessions/valid.json");
        let partial_path = dir.path().join("sessions/partial.json");
        fs::write(&valid_path, &bytes).unwrap();
        fs::write(&partial_path, &bytes[..bytes.len() / 2]).unwrap();
        let entries = live(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].session_id.as_deref(),
            value["sessionId"].as_str()
        );

        fs::write(&partial_path, &bytes).unwrap();
        let entries = live(dir.path());
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry.session_id.as_deref() == value["sessionId"].as_str())
        );
    }

    #[test]
    fn registry_checks_process_start_and_dead_pids() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = fixture(dir.path());
        let path = dir
            .path()
            .join(format!("sessions/{}.json", std::process::id()));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let entries = live(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name.as_deref(), Some("translate-papers"));
        value["procStart"] = "Thu Jan  1 00:00:00 1970".into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(live(dir.path()).is_empty());
        value["pid"] = 999999.into();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(live(dir.path()).is_empty());
        assert!(serde_json::from_str::<Entry>("{}").is_ok());
    }
}
