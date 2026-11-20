use std::sync::Mutex;

use angzarr_embedded::angzarr_ports::CommitSignal;
use angzarr_embedded::v1::command_page::Body;
use cherenkov_run_domain::{DecisionReported, PlanRun, RUN_DOMAIN, Verdict};

use super::*;

fn cover() -> Cover {
    Cover {
        domain: RUN_DOMAIN.to_string(),
        root: None,
        edition: None,
    }
}

/// An edge that keeps the books it is handed.
#[derive(Default)]
struct Recording {
    books: Mutex<Vec<CommandBook>>,
    commits: CommitSignal,
}

impl Edge for Recording {
    async fn handle(&self, book: CommandBook) -> Result<Reported, EdgeError> {
        self.books.lock().unwrap().push(book);
        Ok(Reported {
            delivery: Delivery::Duplicate,
            events: Vec::new(),
        })
    }

    async fn events(&self, _: &Cover, _: u64) -> Result<Vec<EventPage>, EdgeError> {
        Ok(Vec::new())
    }

    fn next_commit(&self) -> CommitWait {
        self.commits.wait()
    }
}

fn body(book: &CommandBook) -> &Body {
    book.page.as_ref().unwrap().body.as_ref().unwrap()
}

#[test]
fn a_command_book_carries_the_command_to_the_stream() {
    let book = command_book(cover(), &PlanRun::default());
    assert_eq!(book.cover, Some(cover()));
    let Body::Command(command) = body(&book) else {
        panic!("not a command: {book:?}");
    };
    assert!(command.payload.is_some());
}

#[test]
fn a_fact_book_carries_the_fact_under_its_external_id() {
    let fact = DecisionReported {
        mutant: "m".to_string(),
        verdict: Verdict::Killed,
        decision: None,
    };
    let book = fact_book(cover(), &fact, "an id");
    assert_eq!(book.cover, Some(cover()));
    let Body::Fact(fact) = body(&book) else {
        panic!("not a fact: {book:?}");
    };
    assert_eq!(fact.origin.as_ref().unwrap().external_id, "an id");
    assert!(fact.payload.is_some());
}

#[tokio::test]
async fn command_hands_the_edge_a_command_book() {
    let edge = Recording::default();
    edge.command(cover(), &PlanRun::default()).await.unwrap();
    let books = edge.books.lock().unwrap();
    assert_eq!(books.len(), 1);
    assert!(matches!(body(&books[0]), Body::Command(_)));
}

#[tokio::test]
async fn report_hands_the_edge_a_fact_book_and_answers_what_it_did() {
    let edge = Recording::default();
    let fact = DecisionReported {
        mutant: "m".to_string(),
        verdict: Verdict::Survived,
        decision: None,
    };
    let reported = edge.report(cover(), &fact, "x").await.unwrap();
    assert_eq!(reported.delivery, Delivery::Duplicate);
    assert!(matches!(
        body(&edge.books.lock().unwrap()[0]),
        Body::Fact(_)
    ));
}

#[test]
fn only_a_retry_recovery_is_retryable() {
    let failed = |recovery| EdgeError {
        cause: Report::new("x"),
        recovery,
    };
    assert!(failed(Recovery::Retry).retryable());
    for recovery in [Recovery::Business, Recovery::Invalid, Recovery::Bug] {
        assert!(!failed(recovery).retryable(), "{recovery:?}");
    }
}
