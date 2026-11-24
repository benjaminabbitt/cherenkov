use std::time::Duration;

use super::*;

fn worker() -> Worker {
    Worker::new(Vec::new())
}

#[tokio::test]
async fn test_a_worker_given_is_taken() {
    let pool = Pool::new(Vec::new());
    assert!(pool.try_take().is_none());
    pool.give(worker());
    assert!(pool.try_take().is_some());
    assert!(pool.try_take().is_none(), "the one worker is taken");
}

#[tokio::test]
async fn test_a_take_waits_for_a_worker_to_be_given() {
    let pool = Pool::new(Vec::new());
    let waiting = {
        let pool = pool.clone();
        tokio::spawn(async move { pool.take().await })
    };
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    assert!(!waiting.is_finished(), "no worker is free");
    pool.give(worker());
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("a worker given wakes the take")
        .unwrap();
}

#[tokio::test]
async fn test_a_worker_given_before_the_take_is_not_missed() {
    let pool = Pool::new(vec![worker()]);
    tokio::time::timeout(Duration::from_secs(5), pool.take())
        .await
        .expect("an idle worker is taken at once");
}
