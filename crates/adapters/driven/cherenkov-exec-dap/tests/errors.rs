//! Error messages carry what an operator needs to act on them.

use cherenkov_exec_dap::dap::DapError;
use cherenkov_exec_dap::isa::IsaError;
use cherenkov_exec_dap::rpc::RpcError;

#[test]
fn test_dap_errors_name_the_failure() {
    let cases = [
        (DapError::Io("broken pipe".into()), "dap i/o: broken pipe"),
        (
            DapError::Protocol("bad JSON".into()),
            "dap protocol: bad JSON",
        ),
        (
            DapError::Failed {
                command: "readMemory".into(),
                message: "nope".into(),
            },
            "dap readMemory failed: nope",
        ),
        (
            DapError::Timeout("event exited".into()),
            "dap timed out waiting for event exited",
        ),
        (DapError::Closed, "dap connection closed"),
    ];
    for (err, want) in cases {
        assert_eq!(err.to_string(), want);
    }
}

#[test]
fn test_rpc_errors_name_the_failure() {
    let cases = [
        (RpcError::Io("reset".into()), "delve rpc i/o: reset"),
        (RpcError::Protocol("id".into()), "delve rpc protocol: id"),
        (
            RpcError::Call {
                method: "Set".into(),
                message: "denied".into(),
            },
            "delve rpc Set: denied",
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
