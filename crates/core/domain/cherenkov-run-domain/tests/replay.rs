//! Replay fixtures: logs of both aggregates as they were stored under a
//! rules version, and the state each folds to at chosen points, kept as
//! snapshot bytes. They pin behaviour across versions of the rules: the
//! events still fold to the same states, and a snapshot written under
//! another version is passed over for the replay.
//!
//! `tests/fixtures/<major.minor>/` holds, for each recorded stream, `NAME.events`
//! (each stored `EventPage` as a big-endian u32 length and its bytes) and
//! `NAME.states` (each listed state as a big-endian u64 sequence, a u32
//! length and the snapshot bytes). A rules version without its directory
//! fails `test_the_current_rules_version_has_its_fixtures`; `just
//! regen-replay-fixtures` writes it, and refuses to touch one that is there.

use std::path::{Path, PathBuf};

use angzarr_embedded::markup::{DomainMessage, Snapshots, pack_message};
use angzarr_embedded::testing::Harness;
use angzarr_embedded::v1::{
    Command, CommandBook, CommandPage, Cover, EventPage, Fact, FactOrigin, Snapshot, Uuid,
    command_page,
};
use angzarr_embedded::{Domain, Registry, ReplayFixture, assert_replay_fixtures};
use cherenkov_mutation::execution::{RawOutcome, Tier as CoreTier};
use cherenkov_run_domain::mutant::MutantPlan;
use cherenkov_run_domain::{
    DecisionReported, ExecutableMutant, MUTANT_DOMAIN, Mutant, MutantSnapshot, OperatorSet,
    PlanMutant, PlanRun, PlannedMutant, PreparationReported, ProgramRun, ProgramRunReported,
    ProgramRunResult, RUN_DOMAIN, Ran, Run, RunId, RunSnapshot, Selection, Verdict, mutant_root,
    observation, register,
};
use std::time::Duration;

const RUN_STREAM: &str = "run-completed";
const MUTANT_STREAM: &str = "mutant-asked-by-two-runs";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The rules version the crate is at, as its fixtures' directory is named.
fn current() -> String {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    format!("{}.{}", parts.next().unwrap(), parts.next().unwrap())
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    registry
}

fn run_cover() -> Cover {
    Cover {
        domain: RUN_DOMAIN.to_string(),
        root: Some(Uuid::from_bytes([1; 16])),
        edition: None,
    }
}

fn mutant_cover() -> Cover {
    Cover {
        domain: MUTANT_DOMAIN.to_string(),
        root: Some(mutant_root("a").unwrap()),
        edition: None,
    }
}

/// What a stream stored, and its state at chosen points as snapshot bytes.
struct Recorded {
    pages: Vec<EventPage>,
    states: Vec<(u64, Vec<u8>)>,
}

fn snapshot_bytes<S: DomainMessage>(snapshot: &S) -> Vec<u8> {
    prost::Message::encode_to_vec(&snapshot.to_wire())
}

fn from_bytes<S: DomainMessage>(bytes: &[u8]) -> S {
    let wire = <S::Wire as prost::Message>::decode(bytes).unwrap();
    S::from_wire(wire).unwrap()
}

// ---- the scenarios ----

fn planned(id: &str) -> ExecutableMutant {
    ExecutableMutant {
        mutant: Some(PlannedMutant {
            id: id.to_string(),
            file: "src/lib.rs".to_string(),
            line: 3,
            column: 5,
            end_line: 3,
            end_column: 10,
            function: Some("total".to_string()),
            operator: "aor".to_string(),
            description: "in-place".to_string(),
            original: "+".to_string(),
            mutated: Some("-".to_string()),
        }),
        plan: Some(MutantPlan {
            id: id.to_string(),
            ..Default::default()
        }),
    }
}

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: CoreTier::P,
        wall: Duration::from_millis(1_500),
    }
}

fn book(cover: &Cover, body: command_page::Body) -> CommandBook {
    CommandBook {
        cover: Some(cover.clone()),
        page: Some(CommandPage {
            body: Some(body),
            ..CommandPage::default()
        }),
    }
}

fn command<C: DomainMessage>(cover: &Cover, command: &C) -> CommandBook {
    book(
        cover,
        command_page::Body::Command(Command {
            payload: Some(pack_message(command)),
            handling: None,
        }),
    )
}

fn fact<F: DomainMessage>(cover: &Cover, fact: &F, external_id: &str) -> CommandBook {
    book(
        cover,
        command_page::Body::Fact(Fact {
            payload: Some(pack_message(fact)),
            origin: Some(FactOrigin {
                external_id: external_id.to_string(),
                description: "replay fixture".to_string(),
            }),
        }),
    )
}

/// Handles each book on a harness of `D`, recording `D`'s state after it.
fn record<D, S>(books: Vec<CommandBook>, snapshot: impl Fn(&D::State) -> S) -> Recorded
where
    D: Domain,
    S: DomainMessage,
    Harness<D>: HarnessOf<D>,
{
    let mut harness = <Harness<D> as HarnessOf<D>>::start();
    let mut states = Vec::new();
    for book in books {
        harness.when_book(book).events_or_panic();
        let last = harness.pages().len() as u64 - 1;
        states.push((last, snapshot_bytes(&snapshot(&harness.state()))));
    }
    Recorded {
        pages: harness.pages(),
        states,
    }
}

/// A harness of `D` over its own stream, and what a handled book must be.
trait HarnessOf<D: Domain> {
    fn start() -> Harness<D>;
}

impl HarnessOf<Run> for Harness<Run> {
    fn start() -> Harness<Run> {
        let mut registry = Registry::new();
        registry.aggregate::<Run>(Run::default).unwrap();
        Harness::new(registry, run_cover())
    }
}

impl HarnessOf<Mutant> for Harness<Mutant> {
    fn start() -> Harness<Mutant> {
        let mut registry = Registry::new();
        registry.aggregate::<Mutant>(Mutant::default).unwrap();
        Harness::new(registry, mutant_cover())
    }
}

trait Applied {
    fn events_or_panic(self);
}

impl<D: Domain> Applied for angzarr_embedded::testing::Then<D> {
    fn events_or_panic(self) {
        self.handled().expect("the scenario's book is handled");
    }
}

fn run_scenario() -> Recorded {
    let cover = run_cover();
    let plan = PlanRun {
        programs: 1,
        root: "/project".to_string(),
        operators: vec![OperatorSet {
            language: "rust".to_string(),
            set: "breadth".to_string(),
        }],
        jobs: 2,
        fast: false,
        mutants: vec![planned("a"), planned("b")],
    };
    let prepared = PreparationReported {
        selection_why: None,
        program: "calc-1".to_string(),
        prepared_ms: 2,
        tests: None,
        selection: Selection::EveryTest,
        failed_tests: Vec::new(),
        selection_reason: Some("no listing".to_string()),
        index: 0,
        baseline: Some(observation::encode(&exited(0))),
        baseline_failures: Vec::new(),
        drives: None,
        baseline_output_tail: None,
    };
    let decided = |id: &str, verdict| DecisionReported {
        mutant: id.to_string(),
        verdict,
        decision: None,
    };
    record::<Run, _>(
        vec![
            command(&cover, &plan),
            fact(&cover, &prepared, "program 0"),
            fact(&cover, &decided("a", Verdict::Killed), "a"),
            fact(&cover, &decided("b", Verdict::Survived), "b"),
        ],
        |run: &Run| run.snapshot().expect("a run offers a snapshot"),
    )
}

fn mutant_scenario() -> Recorded {
    let cover = mutant_cover();
    let plan = |run: u8| PlanMutant {
        run: Some(RunId::from(Uuid::from_bytes([run; 16]))),
        mutant: Some(planned("a")),
        programs: 1,
    };
    let reported = |run: u8, code: i32| ProgramRunReported {
        run: Some(RunId::from(Uuid::from_bytes([run; 16]))),
        ran: Some(ProgramRun {
            program: 0,
            program_name: "calc-1".to_string(),
            took_ms: 0,
            tests: Vec::new(),
            result: ProgramRunResult::Ran(Ran {
                observation: Some(observation::encode(&exited(code))),
                killed_by: Vec::new(),
            }),
        }),
    };
    record::<Mutant, _>(
        vec![
            command(&cover, &plan(1)),
            fact(&cover, &reported(1, 101), "1"),
            command(&cover, &plan(2)),
            fact(&cover, &reported(2, 0), "2"),
        ],
        |mutant: &Mutant| mutant.snapshot().expect("a mutant offers a snapshot"),
    )
}

// ---- the files ----

fn encode_events(pages: &[EventPage]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for page in pages {
        let page = prost::Message::encode_to_vec(page);
        bytes.extend_from_slice(&u32::try_from(page.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(&page);
    }
    bytes
}

fn encode_states(states: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (sequence, state) in states {
        bytes.extend_from_slice(&sequence.to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(state.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(state);
    }
    bytes
}

/// Takes `n` bytes off the front of `bytes`.
fn take<'a>(bytes: &mut &'a [u8], n: usize) -> &'a [u8] {
    let (head, rest) = bytes.split_at(n);
    *bytes = rest;
    head
}

fn take_len(bytes: &mut &[u8]) -> usize {
    u32::from_be_bytes(take(bytes, 4).try_into().unwrap()) as usize
}

fn decode_events(mut bytes: &[u8]) -> Vec<EventPage> {
    let mut pages = Vec::new();
    while !bytes.is_empty() {
        let len = take_len(&mut bytes);
        pages.push(prost::Message::decode(take(&mut bytes, len)).unwrap());
    }
    pages
}

fn decode_states(mut bytes: &[u8]) -> Vec<(u64, Vec<u8>)> {
    let mut states = Vec::new();
    while !bytes.is_empty() {
        let sequence = u64::from_be_bytes(take(&mut bytes, 8).try_into().unwrap());
        let len = take_len(&mut bytes);
        states.push((sequence, take(&mut bytes, len).to_vec()));
    }
    states
}

fn read(dir: &Path, name: &str) -> Recorded {
    let events = std::fs::read(dir.join(format!("{name}.events"))).unwrap();
    let states = std::fs::read(dir.join(format!("{name}.states"))).unwrap();
    Recorded {
        pages: decode_events(&events),
        states: decode_states(&states),
    }
}

/// The version directories recorded, as `major.minor`.
fn versions() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(fixtures()) else {
        return Vec::new();
    };
    let mut versions: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    versions.sort();
    versions
}

// ---- the tests ----

#[test]
fn test_the_current_rules_version_has_its_fixtures() {
    assert!(
        versions().contains(&current()),
        "no tests/fixtures/{}: run `just regen-replay-fixtures`",
        current()
    );
}

fn restored<S: DomainMessage, A: Default + Snapshots<Anchor = S>>(bytes: &[u8]) -> A {
    let mut aggregate = A::default();
    aggregate.restore(from_bytes::<S>(bytes)).unwrap();
    aggregate
}

#[test]
fn test_every_recorded_log_replays_to_the_states_recorded_with_it() {
    for version in versions() {
        let dir = fixtures().join(&version);
        let run = read(&dir, RUN_STREAM);
        assert_replay_fixtures::<Run>(
            &registry(),
            &[ReplayFixture {
                name: &format!("{version}/{RUN_STREAM}"),
                stream: run_cover(),
                log: run.pages,
                states: run
                    .states
                    .iter()
                    .map(|(sequence, bytes)| (*sequence, restored::<RunSnapshot, Run>(bytes)))
                    .collect(),
            }],
        );
        let mutant = read(&dir, MUTANT_STREAM);
        assert_replay_fixtures::<Mutant>(
            &registry(),
            &[ReplayFixture {
                name: &format!("{version}/{MUTANT_STREAM}"),
                stream: mutant_cover(),
                log: mutant.pages,
                states: mutant
                    .states
                    .iter()
                    .map(|(sequence, bytes)| (*sequence, restored::<MutantSnapshot, Mutant>(bytes)))
                    .collect(),
            }],
        );
    }
}

/// The snapshot `bytes` of `domain`'s aggregate, packed as a store keeps it.
fn packed(domain: &str, bytes: &[u8]) -> prost_types::Any {
    if domain == RUN_DOMAIN {
        pack_message(&from_bytes::<RunSnapshot>(bytes))
    } else {
        pack_message(&from_bytes::<MutantSnapshot>(bytes))
    }
}

#[test]
fn test_a_snapshot_is_used_only_under_the_rules_version_it_was_taken_under() {
    let registry = registry();
    let catalog = registry.catalog();
    for version in versions() {
        let dir = fixtures().join(&version);
        for (domain, name) in [(RUN_DOMAIN, RUN_STREAM), (MUTANT_DOMAIN, MUTANT_STREAM)] {
            let (sequence, state) = read(&dir, name).states.pop().expect("a recorded state");
            let snapshot = Snapshot {
                sequence,
                state: Some(packed(domain, &state)),
                rules_version: format!("{version}.0"),
            };
            assert_eq!(
                catalog.snapshot_usable(domain, &snapshot),
                version == current(),
                "{domain} under {version}"
            );
        }
    }
}

#[test]
fn test_the_registry_has_no_event_without_its_upcaster() {
    registry().check().unwrap();
}

/// Writes the fixtures of the current rules version. Never run in passing
/// (`just regen-replay-fixtures`), and it refuses a version whose directory
/// is there: a recorded version is added to, never rewritten.
#[test]
#[ignore = "writes tests/fixtures/<this version>: run through `just regen-replay-fixtures`"]
fn regenerate_the_fixtures_of_the_current_rules_version() {
    let dir = fixtures().join(current());
    assert!(
        !dir.exists(),
        "{} is recorded; a version's fixtures are never rewritten",
        dir.display()
    );
    std::fs::create_dir_all(&dir).unwrap();
    for (name, recorded) in [
        (RUN_STREAM, run_scenario()),
        (MUTANT_STREAM, mutant_scenario()),
    ] {
        std::fs::write(
            dir.join(format!("{name}.events")),
            encode_events(&recorded.pages),
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("{name}.states")),
            encode_states(&recorded.states),
        )
        .unwrap();
    }
}
