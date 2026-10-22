//! Handles background Claude AskUserQuestion requests until the dashboard answers or attaches.
use crate::ask::{Answer, Ask, AskKind, Question};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    io::{self, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

struct Pending {
    ask: Ask,
    questions: Value,
    stream: TcpStream,
}
pub(super) struct AskUserQuestion {
    entries: Arc<Mutex<HashMap<String, Pending>>>,
    stopping: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    #[cfg(test)]
    pub(super) port: u16,
}
impl AskUserQuestion {
    pub(super) fn bind(port: u16, config: PathBuf) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
        #[cfg(test)]
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let pending = Arc::clone(&entries);
        let stop = Arc::clone(&stopping);
        let worker = thread::Builder::new()
            .name("agents-ask_user_question".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let entries = Arc::clone(&pending);
                            let config = config.clone();
                            let stop = Arc::clone(&stop);
                            if let Err(error) = thread::Builder::new()
                                .name("agents-ask_user_question-request".into())
                                .spawn(move || {
                                    if let Err(error) = receive(stream, &config, &entries, &stop) {
                                        eprintln!("agents: Claude AskUserQuestion: {error}");
                                    }
                                })
                            {
                                eprintln!("agents: Claude AskUserQuestion reader: {error}");
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(error) => {
                            eprintln!("agents: Claude AskUserQuestion listener: {error}");
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            entries,
            stopping,
            worker: Some(worker),
            #[cfg(test)]
            port,
        })
    }
    pub(super) fn pending(&self) -> Vec<Ask> {
        let mut entries = self.entries.lock().expect("AskUserQuestion requests lock");
        entries.retain(|_, entry| {
            let mut byte = [0];
            match entry.stream.peek(&mut byte) {
                Ok(0) => false,
                Ok(_) => true,
                Err(error) => error.kind() == io::ErrorKind::WouldBlock,
            }
        });
        entries.values().map(|entry| entry.ask.clone()).collect()
    }
    pub(super) fn answer(&self, ask: &Ask, answer: &Answer) -> io::Result<()> {
        let answers = encode_answers(ask, answer)?;
        let mut entries = self.entries.lock().expect("AskUserQuestion requests lock");
        let entry = entries
            .get(&ask.id)
            .filter(|entry| entry.ask == *ask)
            .ok_or_else(|| io::Error::other("Claude AskUserQuestion is no longer pending"))?;
        let body = if answer.rejected {
            let reason = "The user dismissed these questions. Write them and their options as plain text, then end your turn. Ask nothing through AskUserQuestion until the user replies.";
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":reason}})
        } else {
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"questions":entry.questions,"answers":answers}}})
        };
        let mut entry = entries.remove(&ask.id).expect("pending AskUserQuestion");
        respond(&mut entry.stream, Some(&body))
    }
    pub(super) fn release(&self, session: &str) -> io::Result<()> {
        let mut entries = self.entries.lock().expect("AskUserQuestion requests lock");
        let ids: Vec<_> = entries
            .iter()
            .filter(|(_, entry)| entry.ask.session.id == session)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(mut entry) = entries.remove(&id) {
                respond(&mut entry.stream, None)?;
            }
        }
        Ok(())
    }
}
impl Drop for AskUserQuestion {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        for (_, entry) in self
            .entries
            .lock()
            .expect("AskUserQuestion requests lock")
            .drain()
        {
            let _ = entry.stream.shutdown(Shutdown::Both);
        }
    }
}
fn receive(
    mut stream: TcpStream,
    config: &std::path::Path,
    entries: &Mutex<HashMap<String, Pending>>,
    stopping: &AtomicBool,
) -> io::Result<()> {
    let (ask, questions) = match read_request(&mut stream).and_then(|value| parse_request(&value)) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("agents: invalid Claude AskUserQuestion: {error}");
            return stream.write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    };
    if !super::registry::background_jobs(config).contains_key(&ask.session.id) {
        return respond(&mut stream, None);
    }
    stream.set_read_timeout(None)?;
    stream.set_nonblocking(true)?;
    let mut entries = entries.lock().expect("AskUserQuestion requests lock");
    if !stopping.load(Ordering::Acquire) {
        entries.insert(
            ask.id.clone(),
            Pending {
                ask,
                questions,
                stream,
            },
        );
    }
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid Claude AskUserQuestion or answer",
    )
}
fn parse_request(value: &Value) -> io::Result<(Ask, Value)> {
    if value["hook_event_name"] != "PreToolUse" || value["tool_name"] != "AskUserQuestion" {
        return Err(invalid());
    }
    let id = value["tool_use_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(invalid)?
        .to_owned();
    let session = value["session_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(invalid)?;
    let source = value["tool_input"]["questions"]
        .as_array()
        .filter(|questions| !questions.is_empty())
        .ok_or_else(invalid)?;
    let questions = source
        .iter()
        .map(|value| {
            let text = value["question"]
                .as_str()
                .filter(|text| !text.is_empty())
                .ok_or_else(invalid)?
                .to_owned();
            Ok(Question {
                id: text.clone(),
                text,
                header: value["header"].as_str().ok_or_else(invalid)?.to_owned(),
                options: serde_json::from_value(value["options"].clone())?,
                multi: value["multiSelect"].as_bool().ok_or_else(invalid)?,
                other: true,
                secret: false,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok((
        Ask {
            id,
            session: format!("claude:{session}").parse().map_err(|_| invalid())?,
            since: Utc::now(),
            kind: AskKind::Questions {
                questions,
                blocking: true,
            },
        },
        Value::Array(source.clone()),
    ))
}
pub(super) fn encode_answers(ask: &Ask, answer: &Answer) -> io::Result<BTreeMap<String, String>> {
    let AskKind::Questions { questions, .. } = &ask.kind;
    if answer
        .answers
        .keys()
        .any(|id| !questions.iter().any(|question| &question.id == id))
    {
        return Err(invalid());
    }
    let mut result = BTreeMap::new();
    for question in questions {
        let Some(answer) = answer.answers.get(&question.id) else {
            continue;
        };
        if !question.multi && answer.answers.len() + usize::from(answer.other.is_some()) > 1 {
            return Err(invalid());
        }
        if answer
            .answers
            .iter()
            .any(|label| !question.options.iter().any(|option| &option.label == label))
        {
            return Err(invalid());
        }
        let mut values = answer.answers.clone();
        if let Some(other) = &answer.other {
            if !question.other || other.is_empty() {
                return Err(invalid());
            }
            values.push(other.clone());
        }
        if !values.is_empty() {
            result.insert(question.text.clone(), values.join(", "));
        }
    }
    Ok(result)
}
fn read_request(stream: &mut TcpStream) -> io::Result<Value> {
    // Accepted sockets inherit the listener's nonblocking flag on macOS.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        head.push(byte[0]);
        if head.len() > 16384 {
            return Err(invalid());
        }
    }
    let head = std::str::from_utf8(&head).map_err(|_| invalid())?;
    let mut lines = head.split("\r\n");
    if lines.next() != Some("POST /ask-user-question HTTP/1.1") {
        return Err(invalid());
    }
    let mut length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(invalid());
                }
                length = Some(value.trim().parse::<usize>().map_err(|_| invalid())?);
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return Err(invalid());
            }
        }
    }
    let length = length
        .filter(|length| *length <= 1024 * 1024)
        .ok_or_else(invalid)?;
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
fn respond(stream: &mut TcpStream, body: Option<&Value>) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let body = body
        .map(serde_json::to_vec)
        .transpose()?
        .unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.shutdown(Shutdown::Both)
}
#[cfg(test)]
#[path = "ask_user_question_live_tests.rs"]
mod live_tests;
#[cfg(test)]
#[path = "ask_user_question_tests.rs"]
mod tests;
