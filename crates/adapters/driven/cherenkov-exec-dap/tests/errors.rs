//! Error messages carry what an operator needs to act on them.

use cherenkov_exec_dap::dap::DapError;
use cherenkov_exec_dap::isa::IsaError;
use cherenkov_exec_dap::rpc::RpcError;

#[test]
fn test_dap_errors_name_the_failure() {
    let cases = [
        (
            DapError::Io("broken pipe".into()),
            "cherenkov-exec-dap-dap-io detail=\"broken pipe\"",
        ),
        (
            DapError::Protocol("bad JSON".into()),
            "cherenkov-exec-dap-dap-protocol detail=\"bad JSON\"",
        ),
        (
            DapError::Failed {
                command: "readMemory".into(),
                message: "nope".into(),
            },
            "cherenkov-exec-dap-dap-failed command=readMemory detail=nope",
        ),
        (
            DapError::Timeout("event exited".into()),
            "cherenkov-exec-dap-dap-timeout what=\"event exited\"",
        ),
        (DapError::Closed, "cherenkov-exec-dap-dap-closed"),
    ];
    for (err, want) in cases {
        assert_eq!(err.to_string(), want);
    }
}

#[test]
fn test_rpc_errors_name_the_failure() {
    let cases = [
        (
            RpcError::Io("reset".into()),
            "cherenkov-exec-dap-rpc-io detail=reset",
        ),
        (
            RpcError::Protocol("id".into()),
            "cherenkov-exec-dap-rpc-protocol detail=id",
        ),
        (
            RpcError::Call {
                method: "Set".into(),
                message: "denied".into(),
            },
            "cherenkov-exec-dap-rpc-call method=Set detail=denied",
        ),
    ];
    for (err, want) in cases {
        assert_eq!(err.to_string(), want);
    }
}

#[test]
fn test_isa_error_names_the_table_it_was_found_in_and_what_is_wrong() {
    assert_eq!(
        IsaError::UnknownToken { name: "x".into() }.to_string(),
        "cherenkov-exec-dap-isa-unknown-token name=x"
    );
}
