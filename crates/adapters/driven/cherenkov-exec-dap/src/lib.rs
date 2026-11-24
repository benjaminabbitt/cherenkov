//! The debugger-driven executor. DAP is the protocol for every driver; the
//! Delve driver augments it with Delve's JSON-RPC API only for what DAP
//! lacks (code memory, disassembly with line info, setting the PC); the GDB
//! driver uses DAP plus GDB's repl.
//! Nothing outside this crate knows JSON-RPC exists.

pub mod dap;
mod diagnostics;
pub mod delve;
pub mod gdb;
pub mod gotest;
pub mod isa;
pub mod layouts;
pub mod libtest;
pub mod lines;
pub mod load;
pub mod native;
pub mod routing;
pub mod rpc;
pub mod shapes;
pub mod stubs;
pub mod support_lib;
pub mod tierp;
pub mod toolchain;
pub mod viability;
pub mod x86;

pub use diagnostics::EN_US;
