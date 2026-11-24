use super::*;

fn wall() -> Duration {
    Duration::new(3, 123_456_789)
}

/// One of every kind, with a wall time that has nanoseconds.
fn observations() -> Vec<RawOutcome> {
    vec![
        RawOutcome::Exited {
            code: 101,
            tier: CoreTier::P,
            wall: wall(),
        },
        RawOutcome::Crashed {
            kind: "panic".to_string(),
            message: "index out of range".to_string(),
            tier: CoreTier::B,
            wall: wall(),
        },
        RawOutcome::TimedOut {
            tier: CoreTier::A,
            wall: wall(),
        },
        RawOutcome::NotReached {
            reason: "not linked".to_string(),
        },
        RawOutcome::Unsupported {
            reason: "inlined".to_string(),
        },
        RawOutcome::Unviable {
            reason: "no default".to_string(),
        },
    ]
}

#[test]
fn test_every_observation_goes_through_proto_and_comes_back_equal() {
    for raw in observations() {
        let wire: cherenkov_run_wire::mutant::Observation = encode(&raw).into();
        let decoded = decode(Observation::try_from(wire).unwrap()).unwrap();
        assert_eq!(decoded, raw);
    }
}

#[test]
fn test_an_exit_with_no_wall_time_is_refused_by_the_field() {
    let mut observation = encode(&observations().remove(0));
    if let ObservationKind::Exited(exited) = &mut observation.kind {
        exited.wall = None;
    }
    assert_eq!(
        decode(observation),
        Err(Unreadable::Missing { field: "wall" })
    );
}
