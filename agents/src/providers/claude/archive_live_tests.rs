use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "archives and stops the real background job named by AGENTS_LIVE_ARCHIVE_JOB"]
fn archive_background_session_after_cloud_disconnect() {
    let job_id = std::env::var("AGENTS_LIVE_ARCHIVE_JOB").expect("explicit test job id required");
    let config = config_dir();
    let deadline = Instant::now() + Duration::from_secs(60);
    let entry = loop {
        if let Some(entry) = registry::live(&config).into_iter().find(|entry| {
            entry.kind.as_deref() == Some("bg")
                && entry.job_id.as_deref() == Some(&job_id)
                && entry
                    .bridge_session_id
                    .as_deref()
                    .is_some_and(|id| !id.is_empty())
        }) {
            break entry;
        }
        assert!(
            Instant::now() < deadline,
            "Remote Control registration timed out"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let pid = entry.pid.unwrap();
    let registry_path = config.join("sessions").join(format!("{pid}.json"));
    let mut provider = ClaudeProvider::new(config);
    let session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|session| session.id.id == entry.session_id.as_deref().unwrap())
        .expect("test session must be visible to agents");
    assert_eq!(session.background_job.as_ref().unwrap().id, job_id);
    println!(
        "before cloud archive: {}",
        fs::read_to_string(&registry_path).unwrap()
    );
    println!("process_exists({pid}) = {:?}", process_exists(pid));
    let mut client = api::Client::connect(&provider.config_dir).unwrap();
    let result = api::session::archive(&mut client, entry.bridge_session_id.as_deref().unwrap());
    println!("api::session::archive = {result:?}");
    result.unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let bytes = fs::read(&registry_path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if value["bridgeSessionId"].is_null() {
            assert!(value.as_object().unwrap().contains_key("bridgeSessionId"));
            println!("after cloud archive: {}", String::from_utf8(bytes).unwrap());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cloud archive did not clear bridgeSessionId"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let running = process_exists(pid).unwrap();
    println!("after cloud archive process_exists({pid}) = {running}");
    assert!(running, "must reproduce the still-running local process");
    let result = provider.archive(&session).unwrap()();
    println!("ClaudeProvider::archive = {result:?}");
    result.unwrap();
    let running = process_exists(pid).unwrap();
    println!("after agents archive process_exists({pid}) = {running}");
    assert!(!running);
    assert!(session.transcript.as_ref().unwrap().exists());
}
