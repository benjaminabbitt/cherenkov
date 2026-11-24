//! Delve JSON-RPC client against a fake server on loopback.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

use cherenkov_exec_dap::rpc::{DelveRpc, Function, Location, RpcError};
use serde_json::{Value, json};

/// Answers each line-delimited request with `reply(request)`.
fn fake_delve(n: usize, reply: fn(&Value) -> Value) -> (TcpStream, JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut seen = Vec::new();
        for _ in 0..n {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            let mut resp = reply(&req);
            resp["id"] = req["id"].clone();
            writer.write_all(format!("{resp}\n").as_bytes()).unwrap();
            seen.push(req);
        }
        seen
    });
    (TcpStream::connect(addr).unwrap(), handle)
}

#[test]
fn test_call_sends_rpcserver_method_with_one_param_object() {
    let (stream, server) = fake_delve(2, |_| json!({"result": {"ok": true}, "error": null}));
    let mut rpc = DelveRpc::new(stream).unwrap();
    assert_eq!(
        rpc.call("State", json!({"NonBlocking": true})).unwrap(),
        json!({"ok": true})
    );
    rpc.call("State", json!({})).unwrap();
    let seen = server.join().unwrap();
    assert_eq!(seen[0]["method"], "RPCServer.State");
    assert_eq!(seen[0]["params"], json!([{"NonBlocking": true}]));
    assert_eq!(
        (seen[0]["id"].as_u64(), seen[1]["id"].as_u64()),
        (Some(1), Some(2))
    );
}

#[test]
fn test_call_error_is_reported_with_method() {
    let (stream, server) = fake_delve(
        1,
        |_| json!({"result": null, "error": "could not find symbol value for r0"}),
    );
    let mut rpc = DelveRpc::new(stream).unwrap();
    let err = rpc.call("Eval", json!({})).unwrap_err();
    assert_eq!(
        err,
        RpcError::Call {
            method: "Eval".into(),
            message: "could not find symbol value for r0".into()
        }
    );
    server.join().unwrap();
}

#[test]
fn test_find_location_parses_function_entry() {
    let (stream, server) = fake_delve(1, |_| {
        json!({"result": {"Locations": [{"pc": 6103922, "file": "/src/calc.go", "line": 12,
            "function": {"name": "example.com/calc.IsSmall", "value": 6103904, "type": 0, "goType": 0, "optimized": false}}]}})
    });
    let mut rpc = DelveRpc::new(stream).unwrap();
    let locs = rpc.find_location("/src/calc.go:12").unwrap();
    assert_eq!(
        locs,
        [Location {
            pc: 6103922,
            file: "/src/calc.go".into(),
            line: 12,
            function: Some(Function {
                name: "example.com/calc.IsSmall".into(),
                value: 6103904
            }),
        }]
    );
    let seen = server.join().unwrap();
    assert_eq!(seen[0]["params"][0]["Loc"], "/src/calc.go:12");
    assert_eq!(seen[0]["params"][0]["Scope"]["GoroutineID"], -1);
}

#[test]
fn test_disassemble_function_asks_for_whole_function_and_decodes_bytes() {
    let (stream, server) = fake_delve(1, |_| {
        json!({"result": {"Disassemble": [
            {"Loc": {"pc": 4096, "file": "/src/calc.go", "line": 12}, "DestLoc": null, "Text": "JL 0x5d237a", "Bytes": "fAI=", "Breakpoint": false, "AtPC": false}
        ]}})
    });
    let mut rpc = DelveRpc::new(stream).unwrap();
    let insns = rpc.disassemble_function(4096).unwrap();
    assert_eq!(insns.len(), 1);
    assert_eq!(insns[0].bytes, [0x7c, 0x02]);
    assert_eq!((insns[0].loc.pc, insns[0].loc.line), (4096, 12));
    let seen = server.join().unwrap();
    assert_eq!(seen[0]["method"], "RPCServer.Disassemble");
    assert_eq!(
        (
            seen[0]["params"][0]["StartPC"].as_u64(),
            seen[0]["params"][0]["EndPC"].as_u64()
        ),
        (Some(4096), Some(0))
    );
}

#[test]
fn test_read_memory_decodes_base64_mem() {
    let (stream, server) = fake_delve(
        1,
        |_| json!({"result": {"Mem": "fAI=", "IsLittleEndian": true}}),
    );
    let mut rpc = DelveRpc::new(stream).unwrap();
    assert_eq!(rpc.read_memory(0x5d2376, 2).unwrap(), [0x7c, 0x02]);
    let seen = server.join().unwrap();
    assert_eq!(
        seen[0]["params"][0],
        json!({"Address": 0x5d2376, "Length": 2})
    );
}

#[test]
fn test_write_memory_sets_each_byte_through_the_evaluator() {
    let (stream, server) = fake_delve(2, |_| json!({"result": {}, "error": null}));
    let mut rpc = DelveRpc::new(stream).unwrap();
    rpc.write_memory(0x10, &[0x7d, 0xab]).unwrap();
    let seen = server.join().unwrap();
    let sets: Vec<(Value, Value)> = seen
        .iter()
        .map(|r| {
            (
                r["params"][0]["Symbol"].clone(),
                r["params"][0]["Value"].clone(),
            )
        })
        .collect();
    assert_eq!(
        sets,
        [
            (json!("*(*uint8)(0x10)"), json!("125")),
            (json!("*(*uint8)(0x11)"), json!("171"))
        ]
    );
    assert_eq!(seen[0]["method"], "RPCServer.Set");
    assert_eq!(
        seen[0]["params"][0]["Scope"],
        json!({"GoroutineID": -1, "Frame": 0})
    );
}

#[test]
fn test_write_memory_stops_at_the_first_failing_byte() {
    let (stream, server) = fake_delve(
        1,
        |_| json!({"result": null, "error": "could not write memory"}),
    );
    let mut rpc = DelveRpc::new(stream).unwrap();
    let err = rpc.write_memory(0x10, &[1, 2, 3]).unwrap_err();
    assert!(matches!(err, RpcError::Call { ref method, .. } if method == "Set"));
    assert_eq!(server.join().unwrap().len(), 1);
}
