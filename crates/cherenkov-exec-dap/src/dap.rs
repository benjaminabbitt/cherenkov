//! Minimal DAP client: Content-Length framing, request/response
//! correlation, and an event queue for events that arrive in between.
//! Works over TCP (Delve) or any pair of byte streams (GDB on stdio).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DapError {
    Io(String),
    /// Malformed framing or JSON from the adapter.
    Protocol(String),
    /// The adapter answered `success: false`.
    Failed {
        command: String,
        message: String,
    },
    Timeout(String),
    /// The adapter closed the connection.
    Closed,
}

impl std::fmt::Display for DapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DapError::Io(m) => write!(f, "dap i/o: {m}"),
            DapError::Protocol(m) => write!(f, "dap protocol: {m}"),
            DapError::Failed { command, message } => write!(f, "dap {command} failed: {message}"),
            DapError::Timeout(what) => write!(f, "dap timed out waiting for {what}"),
            DapError::Closed => write!(f, "dap connection closed"),
        }
    }
}

impl std::error::Error for DapError {}

/// Frame one message: `Content-Length: <bytes>\r\n\r\n<json>`.
pub fn encode(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Incremental decoder for a stream of framed messages.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next complete message, if one is buffered.
    pub fn next_message(&mut self) -> Result<Option<Value>, DapError> {
        let Some(header_end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            return Ok(None);
        };
        let header = std::str::from_utf8(&self.buf[..header_end])
            .map_err(|e| DapError::Protocol(format!("header is not UTF-8: {e}")))?;
        let len = header
            .split("\r\n")
            .find_map(|line| line.strip_prefix("Content-Length:"))
            .ok_or_else(|| DapError::Protocol(format!("no Content-Length in {header:?}")))?
            .trim()
            .parse::<usize>()
            .map_err(|e| DapError::Protocol(format!("bad Content-Length: {e}")))?;
        let start = header_end + 4;
        if self.buf.len() < start + len {
            return Ok(None);
        }
        let body: Vec<u8> = self.buf.drain(..start + len).skip(start).collect();
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| DapError::Protocol(format!("bad JSON: {e}")))
    }
}

pub struct DapClient {
    writer: Box<dyn Write + Send>,
    /// Chunks from the reader thread; `Err` once the adapter's side closes.
    chunks: Receiver<Result<Vec<u8>, DapError>>,
    decoder: Decoder,
    seq: u64,
    events: VecDeque<Value>,
    /// Queued events looked at by waits and takes, so a test can show that
    /// a wait does not scan the queue.
    inspections: std::cell::Cell<u64>,
    /// Responses that arrived while waiting for another request's.
    responses: HashMap<u64, Value>,
    timeout: Duration,
}

impl DapClient {
    /// A client over a TCP connection (Delve). `timeout` bounds every
    /// wait for a response to [`DapClient::request`].
    pub fn new(stream: TcpStream, timeout: Duration) -> Self {
        match stream.try_clone() {
            Ok(reader) => DapClient::over(reader, stream, timeout),
            // Without a second handle there is nothing to read from; every
            // wait reports the connection closed.
            Err(e) => {
                let (tx, chunks) = mpsc::channel();
                let _ = tx.send(Err(DapError::Io(e.to_string())));
                DapClient::with(Box::new(stream), chunks, timeout)
            }
        }
    }

    /// A client over any byte streams, e.g. a child's stdout and stdin (GDB).
    /// A thread reads `reader` until it closes.
    pub fn over(
        mut reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        timeout: Duration,
    ) -> Self {
        let (tx, chunks) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let item = match reader.read(&mut buf) {
                    Ok(0) => Err(DapError::Closed),
                    Ok(n) => Ok(buf[..n].to_vec()),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => Err(DapError::Closed),
                };
                let done = item.is_err();
                if tx.send(item).is_err() || done {
                    return;
                }
            }
        });
        DapClient::with(Box::new(writer), chunks, timeout)
    }

    fn with(
        writer: Box<dyn Write + Send>,
        chunks: Receiver<Result<Vec<u8>, DapError>>,
        timeout: Duration,
    ) -> Self {
        DapClient {
            writer,
            chunks,
            decoder: Decoder::default(),
            seq: 0,
            events: VecDeque::new(),
            inspections: std::cell::Cell::new(0),
            responses: HashMap::new(),
            timeout,
        }
    }

    /// Send a request without waiting; collect it later with
    /// [`DapClient::response`]. Returns its `seq`.
    pub fn send(&mut self, command: &str, arguments: Value) -> Result<u64, DapError> {
        self.seq += 1;
        let mut message = json!({"seq": self.seq, "type": "request", "command": command});
        if !arguments.is_null() {
            message["arguments"] = arguments;
        }
        self.writer
            .write_all(&encode(&message))
            .map_err(|e| DapError::Io(e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| DapError::Io(e.to_string()))?;
        Ok(self.seq)
    }

    /// Send a request and return its response `body` (`null` when absent).
    /// Events that arrive first are queued for [`DapClient::wait_event`].
    pub fn request(&mut self, command: &str, arguments: Value) -> Result<Value, DapError> {
        let seq = self.send(command, arguments)?;
        self.response(seq, self.timeout)
    }

    /// The response `body` to the request with this `seq`.
    pub fn response(&mut self, seq: u64, timeout: Duration) -> Result<Value, DapError> {
        let deadline = Instant::now() + timeout;
        let message = loop {
            if let Some(m) = self.responses.remove(&seq) {
                break m;
            }
            let message = self.next(deadline, || format!("response to request {seq}"))?;
            match (message["type"].as_str(), message["request_seq"].as_u64()) {
                (Some("response"), Some(s)) if s == seq => break message,
                (Some("response"), Some(other)) => {
                    self.responses.insert(other, message);
                }
                (Some("event"), _) => self.events.push_back(message),
                // Reverse requests and malformed responses are not used by Cherenkov.
                _ => {}
            }
        };
        if message["success"].as_bool() != Some(true) {
            let command = message["command"].as_str().unwrap_or("request").to_string();
            let text = message["message"]
                .as_str()
                .unwrap_or("no message")
                .to_string();
            return Err(DapError::Failed {
                command,
                message: text,
            });
        }
        Ok(message.get("body").cloned().unwrap_or(Value::Null))
    }

    /// The first queued or incoming event named `event`; earlier events of
    /// other names stay queued. Returns the whole event message.
    pub fn wait_event(&mut self, event: &str, timeout: Duration) -> Result<Value, DapError> {
        self.wait_any_event(&[event], timeout)
    }

    /// The first queued or incoming event whose name is one of `events`;
    /// other events stay queued, in order.
    pub fn wait_any_event(
        &mut self,
        events: &[&str],
        timeout: Duration,
    ) -> Result<Value, DapError> {
        let wanted = |m: &Value| m["type"] == "event" && events.iter().any(|e| m["event"] == *e);
        let looked = |m: &Value| {
            self.inspections.set(self.inspections.get() + 1);
            wanted(m)
        };
        if let Some(i) = self.events.iter().position(looked) {
            return Ok(self.events.remove(i).expect("index from position"));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let message = self.next(deadline, || format!("event {}", events.join(" or ")))?;
            if wanted(&message) {
                return Ok(message);
            }
            match (message["type"].as_str(), message["request_seq"].as_u64()) {
                (Some("event"), _) => self.events.push_back(message),
                (Some("response"), Some(seq)) => {
                    self.responses.insert(seq, message);
                }
                _ => {}
            }
        }
    }

    /// Queue whatever arrives until nothing has for `idle` (zero: only what
    /// has already arrived), or until `done` holds for the queue.
    pub fn settle_until(&mut self, idle: Duration, done: impl Fn(&VecDeque<Value>) -> bool) {
        loop {
            if done(&self.events) {
                return;
            }
            let deadline = Instant::now() + idle;
            let Ok(message) = self.next(deadline, || "settle".to_string()) else {
                return;
            };
            match (message["type"].as_str(), message["request_seq"].as_u64()) {
                (Some("event"), _) => self.events.push_back(message),
                (Some("response"), Some(seq)) => {
                    self.responses.insert(seq, message);
                }
                _ => {}
            }
        }
    }

    /// Queue whatever arrives until nothing has for `idle`.
    pub fn settle(&mut self, idle: Duration) {
        loop {
            let deadline = Instant::now() + idle;
            let Ok(message) = self.next(deadline, || "settle".to_string()) else {
                return;
            };
            match (message["type"].as_str(), message["request_seq"].as_u64()) {
                (Some("event"), _) => self.events.push_back(message),
                (Some("response"), Some(seq)) => {
                    self.responses.insert(seq, message);
                }
                _ => {}
            }
        }
    }

    /// How many queued events waits and takes have looked at so far.
    pub fn event_inspections(&self) -> u64 {
        self.inspections.get()
    }

    /// The events received and not yet taken.
    pub fn queued(&self) -> &VecDeque<Value> {
        &self.events
    }

    /// Remove and return every queued event named `event`, in arrival order.
    pub fn take_events(&mut self, event: &str) -> Vec<Value> {
        self.inspections
            .set(self.inspections.get() + self.events.len() as u64);
        let (taken, kept): (VecDeque<Value>, VecDeque<Value>) = std::mem::take(&mut self.events)
            .into_iter()
            .partition(|e| e["event"] == event);
        self.events = kept;
        taken.into()
    }

    /// The next message from the adapter, reading until `deadline`.
    fn next(&mut self, deadline: Instant, what: impl Fn() -> String) -> Result<Value, DapError> {
        loop {
            if let Some(message) = self.decoder.next_message()? {
                return Ok(message);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.chunks.recv_timeout(left) {
                Ok(Ok(chunk)) => self.decoder.push(&chunk),
                Ok(Err(e)) => return Err(e),
                Err(RecvTimeoutError::Timeout) => return Err(DapError::Timeout(what())),
                Err(RecvTimeoutError::Disconnected) => return Err(DapError::Closed),
            }
        }
    }
}
