//! Delve's JSON-RPC API (`RPCServer.*`, JSON-RPC 1.0 over a TCP stream),
//! used only for primitives Delve's DAP server lacks.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use base64::Engine;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcError {
    Io(String),
    Protocol(String),
    /// Delve returned an error for the call.
    Call {
        method: String,
        message: String,
    },
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Io(m) => write!(f, "delve rpc i/o: {m}"),
            RpcError::Protocol(m) => write!(f, "delve rpc protocol: {m}"),
            RpcError::Call { method, message } => write!(f, "delve rpc {method}: {message}"),
        }
    }
}

impl std::error::Error for RpcError {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Function {
    pub name: String,
    /// Entry address: the first instruction, before the prologue.
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Location {
    pub pc: u64,
    pub file: String,
    pub line: u32,
    pub function: Option<Function>,
}

/// One disassembled instruction with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub loc: Location,
    pub bytes: Vec<u8>,
}

pub struct DelveRpc {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    id: u64,
}

impl DelveRpc {
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(DelveRpc {
            reader: BufReader::new(stream),
            writer,
            id: 0,
        })
    }

    /// Call `RPCServer.<method>` with one parameter object; returns `result`.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.id += 1;
        let request =
            json!({"method": format!("RPCServer.{method}"), "params": [params], "id": self.id});
        let io = |e: std::io::Error| RpcError::Io(e.to_string());
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .map_err(io)?;
        let mut line = String::new();
        if self.reader.read_line(&mut line).map_err(io)? == 0 {
            return Err(RpcError::Io("connection closed".into()));
        }
        let response: Value =
            serde_json::from_str(&line).map_err(|e| RpcError::Protocol(e.to_string()))?;
        if response["id"].as_u64() != Some(self.id) {
            return Err(RpcError::Protocol(format!(
                "response id {} for request {}",
                response["id"], self.id
            )));
        }
        match &response["error"] {
            Value::Null => Ok(response.get("result").cloned().unwrap_or(Value::Null)),
            Value::String(message) => Err(RpcError::Call {
                method: method.to_string(),
                message: message.clone(),
            }),
            other => Err(RpcError::Call {
                method: method.to_string(),
                message: other.to_string(),
            }),
        }
    }

    fn typed<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
        field: &str,
    ) -> Result<T, RpcError> {
        let result = self.call(method, params)?;
        serde_json::from_value(result.get(field).cloned().unwrap_or(Value::Null))
            .map_err(|e| RpcError::Protocol(format!("{method}.{field}: {e}")))
    }

    /// Resolve `file:line` (or any Delve location expression).
    pub fn find_location(&mut self, loc: &str) -> Result<Vec<Location>, RpcError> {
        self.typed(
            "FindLocation",
            json!({"Scope": SCOPE.clone(), "Loc": loc, "IncludeNonExecutableLines": false}),
            "Locations",
        )
    }

    /// Every instruction of the function containing `pc`.
    pub fn disassemble_function(&mut self, pc: u64) -> Result<Vec<Instruction>, RpcError> {
        #[derive(Deserialize)]
        struct Asm {
            #[serde(rename = "Loc")]
            loc: Location,
            #[serde(rename = "Bytes")]
            bytes: String,
        }
        let asm: Vec<Asm> = self.typed(
            "Disassemble",
            json!({"Scope": SCOPE.clone(), "StartPC": pc, "EndPC": 0, "Flavour": 0}),
            "Disassemble",
        )?;
        asm.into_iter()
            .map(|a| {
                Ok(Instruction {
                    bytes: decode_base64(&a.bytes)?,
                    loc: a.loc,
                })
            })
            .collect()
    }

    pub fn read_memory(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, RpcError> {
        let mem: String = self.typed(
            "ExamineMemory",
            json!({"Address": addr, "Length": len}),
            "Mem",
        )?;
        decode_base64(&mem)
    }

    /// Write bytes through Delve's evaluator (`*(*uint8)(addr) = b`), the
    /// only code-memory write Delve exposes (see docs/m1-delve-findings.md).
    pub fn write_memory(&mut self, addr: u64, bytes: &[u8]) -> Result<(), RpcError> {
        for (i, b) in bytes.iter().enumerate() {
            let symbol = format!("*(*uint8)({:#x})", addr + i as u64);
            self.call("Set", json!({"Scope": {"GoroutineID": -1, "Frame": 0}, "Symbol": symbol, "Value": b.to_string()}))?;
        }
        Ok(())
    }
}

/// Evaluate in the selected goroutine's scope.
static SCOPE: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!({"GoroutineID": -1}));

fn decode_base64(s: &str) -> Result<Vec<u8>, RpcError> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| RpcError::Protocol(format!("base64: {e}")))
}
