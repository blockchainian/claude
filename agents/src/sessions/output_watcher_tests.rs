use super::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

fn append(path: &std::path::Path) {
    writeln!(
        OpenOptions::new().append(true).open(path).unwrap(),
        "later output"
    )
    .unwrap();
}

#[test]
fn output_watcher_notifies_when_an_empty_output_file_is_appended() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("task.output");
    fs::write(&path, "").unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut watcher = OutputWatcher::new(move |event| sender.send(event).unwrap()).unwrap();
    assert!(watcher.set_path(Some(path.clone())).unwrap());
    append(&path);
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
}

#[test]
fn output_watcher_follows_file_creation_and_releases_the_previous_directory() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let first_path = first.path().join("first.output");
    let second_path = second.path().join("second.output");
    let (sender, receiver) = mpsc::channel();
    let mut watcher = OutputWatcher::new(move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    assert!(watcher.set_path(Some(first_path.clone())).unwrap());
    fs::write(&first_path, "created output").unwrap();
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(watcher.set_path(Some(second_path.clone())).unwrap());
    while receiver.try_recv().is_ok() {}
    append(&first_path);
    assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
    fs::write(&second_path, "").unwrap();
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    while receiver.try_recv().is_ok() {}
    append(&second_path);
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(watcher.set_path(None).unwrap());
    while receiver.try_recv().is_ok() {}
    append(&second_path);
    assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn output_watcher_ignores_sibling_output_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected.output");
    let sibling = directory.path().join("sibling.output");
    fs::write(&path, "").unwrap();
    fs::write(&sibling, "").unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut watcher = OutputWatcher::new(move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    watcher.set_path(Some(path.clone())).unwrap();
    assert!(!watcher.set_path(Some(path.clone())).unwrap());
    append(&path);
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    // FSEvents may deliver the selected file's creation after watching starts.
    while receiver.recv_timeout(Duration::from_millis(300)).is_ok() {}
    append(&sibling);
    assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
    append(&path);
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
}

#[test]
fn output_watcher_reports_missing_directories_without_repeating_the_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing/task.output");
    let mut watcher = OutputWatcher::new(|_| {}).unwrap();
    assert!(watcher.set_path(Some(path.clone())).is_err());
    assert!(!watcher.set_path(Some(path)).unwrap());
    assert!(watcher.set_path(None).unwrap());
}

#[test]
fn output_watcher_notifies_each_write_before_the_writer_closes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("running.output");
    let mut writer = OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(&path)
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut watcher = OutputWatcher::new(move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    watcher.set_path(Some(path)).unwrap();
    // Drain creation notifications before testing a process's persistent stdout handle.
    while receiver.recv_timeout(Duration::from_millis(300)).is_ok() {}
    for line in 1..=3 {
        writeln!(writer, "live demo line {line}").unwrap();
        receiver
            .recv_timeout(Duration::from_millis(900))
            .unwrap_or_else(|error| {
                panic!("write {line} was not delivered while stdout stayed open: {error}")
            })
            .unwrap();
        while receiver.recv_timeout(Duration::from_millis(100)).is_ok() {}
    }
}

#[test]
fn output_watcher_follows_a_replaced_file_with_an_open_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("task.output");
    fs::write(&path, "first output").unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut watcher = OutputWatcher::new(move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    watcher.set_path(Some(path.clone())).unwrap();
    let replacement = directory.path().join("replacement");
    let mut writer = OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(&replacement)
        .unwrap();
    fs::rename(&replacement, &path).unwrap();
    receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    while receiver.recv_timeout(Duration::from_millis(300)).is_ok() {}
    writeln!(writer, "output after replacement").unwrap();
    receiver
        .recv_timeout(Duration::from_millis(900))
        .unwrap()
        .unwrap();
}
