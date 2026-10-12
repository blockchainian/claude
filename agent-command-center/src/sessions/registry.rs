use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<i64>,
    pub proc_start: Option<String>,
    pub version: Option<String>,
    pub kind: Option<String>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub updated_at: Option<i64>,
    pub status: Option<String>,
}

pub(super) fn process_starts(pids: &[u32]) -> HashMap<u32, Vec<DateTime<Utc>>> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let Ok(output) = Command::new("ps")
        .args(["-o", "pid=,lstart=", "-p"])
        .arg(
            pids.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
        .env("LC_ALL", "C")
        .output()
    else {
        return HashMap::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (pid, start) = line.split_once(char::is_whitespace)?;
            let pid = pid.parse().ok()?;
            let local = NaiveDateTime::parse_from_str(start.trim(), "%a %b %e %H:%M:%S %Y").ok()?;
            let starts = match Local.from_local_datetime(&local) {
                chrono::LocalResult::Single(time) => vec![time.with_timezone(&Utc)],
                chrono::LocalResult::Ambiguous(a, b) => {
                    vec![a.with_timezone(&Utc), b.with_timezone(&Utc)]
                }
                chrono::LocalResult::None => return None,
            };
            Some((pid, starts))
        })
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
            .expect("ps must be available for the liveness test")[0];
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/registry.json")).unwrap();
        value["pid"] = pid.into();
        value["procStart"] = start.format("%a %b %e %H:%M:%S %Y").to_string().into();
        fs::create_dir_all(dir.join("sessions")).unwrap();
        value
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
