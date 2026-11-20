//! DAP framing and the client, against a fake adapter on loopback.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cherenkov_exec_dap::dap::{DapClient, DapError, Decoder, encode};
use serde_json::{Value, json};

#[test]
fn test_encode_counts_content_length_in_bytes() {
    let framed = encode(&json!({"a": "\u{e9}"}));
    assert_eq!(
        framed,
        b"Content-Length: 10\r\n\r\n{\"a\":\"\xc3\xa9\"}".to_vec()
    );
}

#[test]
fn test_decoder_returns_messages_split_across_and_packed_into_reads() {
    let mut d = Decoder::default();
    let two = [encode(&json!({"n": 1})), encode(&json!({"n": 2}))].concat();
    let (a, b) = two.split_at(7);
    d.push(a);
    assert_eq!(d.next_message(), Ok(None));
    d.push(b);
    assert_eq!(d.next_message(), Ok(Some(json!({"n": 1}))));
    assert_eq!(d.next_message(), Ok(Some(json!({"n": 2}))));
    assert_eq!(d.next_message(), Ok(None));
}

#[test]
fn test_decoder_ignores_other_headers() {
    let mut d = Decoder::default();
    d.push(b"Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{}");
    assert_eq!(d.next_message(), Ok(Some(json!({}))));
}

#[test]
fn test_decoder_rejects_missing_length_and_bad_json() {
    let mut d = Decoder::default();
    d.push(b"X-Other: 1\r\n\r\n{}");
    assert!(matches!(d.next_message(), Err(DapError::Protocol(_))));
    let mut d = Decoder::default();
    d.push(b"Content-Length: 3\r\n\r\n{x}");
    assert!(matches!(d.next_message(), Err(DapError::Protocol(_))));
}

/// Per request, in order: the messages the fake adapter replies with.
type Script = Vec<Box<dyn Fn(&Value) -> Vec<Value> + Send>>;

/// A fake adapter: reads one request at a time and answers with `script`,
/// then closes the connection.
fn fake_adapter(script: Script) -> (TcpStream, JoinHandle<Vec<Value>>) {
    fake_adapter_holding(script, Duration::ZERO)
}

/// As [`fake_adapter`], but keeps the connection open and silent for
/// `hold` after the script ends.
fn fake_adapter_holding(script: Script, hold: Duration) -> (TcpStream, JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut seen = Vec::new();
        for step in script {
            let request = read_framed(&mut reader);
            for reply in step(&request) {
                writer.write_all(&encode(&reply)).unwrap();
            }
            seen.push(request);
        }
        thread::sleep(hold);
        seen
    });
    (TcpStream::connect(addr).unwrap(), handle)
}

fn read_framed(reader: &mut BufReader<TcpStream>) -> Value {
    let mut len = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length: ") {
            len = n.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn response(req: &Value, success: bool, body: Value) -> Value {
    json!({"seq": 100, "type": "response", "request_seq": req["seq"], "command": req["command"], "success": success, "body": body})
}

fn event(name: &str, body: Value) -> Value {
    json!({"seq": 200, "type": "event", "event": name, "body": body})
}

const T: Duration = Duration::from_secs(5);

#[test]
fn test_request_sends_command_with_increasing_seq_and_returns_body() {
    let (stream, adapter) = fake_adapter(vec![
        Box::new(|r| vec![response(r, true, json!({"supportsSetVariable": true}))]),
        Box::new(|r| vec![response(r, true, Value::Null)]),
    ]);
    let mut dap = DapClient::new(stream, T);
    let body = dap
        .request("initialize", json!({"adapterID": "cherenkov"}))
        .unwrap();
    assert_eq!(body, json!({"supportsSetVariable": true}));
    assert_eq!(
        dap.request("configurationDone", Value::Null).unwrap(),
        Value::Null
    );
    let seen = adapter.join().unwrap();
    assert_eq!(seen[0]["type"], "request");
    assert_eq!(seen[0]["command"], "initialize");
    assert_eq!(seen[0]["arguments"]["adapterID"], "cherenkov");
    assert_eq!(
        (seen[0]["seq"].as_u64(), seen[1]["seq"].as_u64()),
        (Some(1), Some(2))
    );
}

#[test]
fn test_events_before_a_response_are_queued_for_wait_event() {
    let (stream, adapter) = fake_adapter(vec![Box::new(|r| {
        vec![
            event("output", json!({"output": "hi"})),
            event("exited", json!({"exitCode": 3})),
            response(r, true, Value::Null),
        ]
    })]);
    let mut dap = DapClient::new(stream, T);
    dap.request("continue", json!({"threadId": 1})).unwrap();
    let exited = dap.wait_event("exited", T).unwrap();
    assert_eq!(exited["body"]["exitCode"], 3);
    assert_eq!(dap.wait_event("output", T).unwrap()["body"]["output"], "hi");
    adapter.join().unwrap();
}

#[test]
fn test_failed_response_is_an_error_with_the_adapter_message() {
    let (stream, adapter) = fake_adapter(vec![Box::new(|r| {
        let mut resp = response(r, false, Value::Null);
        resp["message"] = json!("unknown memoryReference");
        vec![resp]
    })]);
    let mut dap = DapClient::new(stream, T);
    let err = dap.request("readMemory", json!({})).unwrap_err();
    assert_eq!(
        err,
        DapError::Failed {
            command: "readMemory".into(),
            message: "unknown memoryReference".into()
        }
    );
    adapter.join().unwrap();
}

#[test]
fn test_wait_event_times_out() {
    let (stream, adapter) = fake_adapter_holding(
        vec![Box::new(|r| vec![response(r, true, Value::Null)])],
        Duration::from_millis(500),
    );
    let mut dap = DapClient::new(stream, T);
    dap.request("configurationDone", Value::Null).unwrap();
    let err = dap
        .wait_event("exited", Duration::from_millis(100))
        .unwrap_err();
    assert_eq!(err, DapError::Timeout("event exited".into()));
    adapter.join().unwrap();
}

#[test]
fn test_closed_connection_is_reported() {
    // No script: the adapter accepts, then drops its socket.
    let (stream, adapter) = fake_adapter(vec![]);
    let mut dap = DapClient::new(stream, T);
    adapter.join().unwrap();
    let err = dap.request("disconnect", Value::Null).unwrap_err();
    assert_eq!(err, DapError::Closed);
}

#[test]
fn test_wait_any_event_returns_the_first_of_several_names_and_queues_the_rest() {
    let (stream, adapter) = fake_adapter(vec![Box::new(|r| {
        vec![
            event("output", json!({"output": "x"})),
            event("stopped", json!({"reason": "exception"})),
            event("exited", json!({"exitCode": 2})),
            response(r, true, Value::Null),
        ]
    })]);
    let mut dap = DapClient::new(stream, T);
    dap.request("continue", json!({"threadId": 1})).unwrap();
    let first = dap.wait_any_event(&["exited", "stopped"], T).unwrap();
    assert_eq!(first["event"], "stopped");
    assert_eq!(
        dap.wait_any_event(&["exited", "stopped"], T).unwrap()["event"],
        "exited"
    );
    assert_eq!(dap.wait_event("output", T).unwrap()["event"], "output");
    adapter.join().unwrap();
}

#[test]
fn test_response_to_another_request_is_not_taken_as_this_one() {
    let (stream, adapter) = fake_adapter(vec![Box::new(|r| {
        let mut stale = response(r, true, json!({"stale": true}));
        stale["request_seq"] = json!(99);
        vec![stale, response(r, true, json!({"fresh": true}))]
    })]);
    let mut dap = DapClient::new(stream, T);
    assert_eq!(
        dap.request("threads", Value::Null).unwrap(),
        json!({"fresh": true})
    );
    adapter.join().unwrap();
}

#[test]
fn test_client_works_over_pipes_as_gdb_speaks_it() {
    let (from_adapter, mut adapter_out) = std::io::pipe().unwrap();
    let (adapter_in, to_adapter) = std::io::pipe().unwrap();
    let adapter = thread::spawn(move || {
        let mut d = Decoder::default();
        let mut reader = adapter_in;
        let mut buf = [0u8; 4096];
        let request = loop {
            if let Some(m) = d.next_message().unwrap() {
                break m;
            }
            let n = reader.read(&mut buf).unwrap();
            d.push(&buf[..n]);
        };
        adapter_out
            .write_all(&encode(&response(&request, true, json!({"via": "pipe"}))))
            .unwrap();
        request
    });
    let mut dap = DapClient::over(from_adapter, to_adapter, T);
    assert_eq!(
        dap.request("initialize", json!({})).unwrap(),
        json!({"via": "pipe"})
    );
    assert_eq!(adapter.join().unwrap()["command"], "initialize");
}

#[test]
fn test_a_response_deferred_past_later_requests_can_be_collected() {
    // GDB answers `launch` only after `configurationDone`.
    let (stream, adapter) = fake_adapter(vec![
        Box::new(|_launch| vec![]),
        Box::new(|done| {
            let mut launch = response(done, true, json!({"launched": true}));
            launch["request_seq"] = json!(1);
            launch["command"] = json!("launch");
            vec![response(done, true, Value::Null), launch]
        }),
    ]);
    let mut dap = DapClient::new(stream, T);
    let launch = dap.send("launch", json!({"program": "x"})).unwrap();
    assert_eq!(
        dap.request("configurationDone", Value::Null).unwrap(),
        Value::Null
    );
    assert_eq!(dap.response(launch, T).unwrap(), json!({"launched": true}));
    adapter.join().unwrap();
}

#[test]
fn test_collecting_a_failed_deferred_response_is_an_error() {
    let (stream, adapter) = fake_adapter(vec![Box::new(|r| {
        let mut resp = response(r, false, Value::Null);
        resp["message"] = json!("no such program");
        vec![resp]
    })]);
    let mut dap = DapClient::new(stream, T);
    let seq = dap.send("launch", json!({})).unwrap();
    let err = dap.response(seq, T).unwrap_err();
    assert_eq!(
        err,
        DapError::Failed {
            command: "launch".into(),
            message: "no such program".into()
        }
    );
    adapter.join().unwrap();
}

#[test]
fn test_take_events_drains_only_the_named_events_in_order() {
    let (stream, adapter) = fake_adapter(vec![Box::new(|r| {
        vec![
            event("output", json!({"output": "a"})),
            event("stopped", json!({"reason": "entry"})),
            event("output", json!({"output": "b"})),
            response(r, true, Value::Null),
        ]
    })]);
    let mut dap = DapClient::new(stream, T);
    dap.request("continue", Value::Null).unwrap();
    let outputs: Vec<Value> = dap
        .take_events("output")
        .into_iter()
        .map(|e| e["body"]["output"].clone())
        .collect();
    assert_eq!(outputs, [json!("a"), json!("b")]);
    assert!(dap.take_events("output").is_empty());
    assert_eq!(
        dap.wait_event("stopped", T).unwrap()["body"]["reason"],
        "entry"
    );
    adapter.join().unwrap();
}
