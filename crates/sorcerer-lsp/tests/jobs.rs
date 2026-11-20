//! `Jobs`: a job runs after its delay, a resubmit restarts the delay, a cancel drops it.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use sorcerer_lsp::Jobs;

const DELAY: Duration = Duration::from_millis(100);
/// Long enough for any job due to have run, on a busy machine too.
const WAIT: Duration = Duration::from_secs(2);
/// How long to watch for a job that must not run.
const QUIET: Duration = Duration::from_millis(200);

fn runs(receiver: &Receiver<&'static str>) -> Vec<&'static str> {
    std::iter::from_fn(|| receiver.recv_timeout(QUIET).ok()).collect()
}

#[test]
fn a_job_runs_after_its_delay() {
    let jobs = Jobs::new();
    let (ran, receiver) = channel();
    let started = std::time::Instant::now();
    jobs.submit("pass", DELAY, move || ran.send("a").unwrap());
    assert_eq!(receiver.recv_timeout(WAIT), Ok("a"));
    assert!(started.elapsed() >= DELAY);
}

#[test]
fn a_job_runs_on_a_thread_of_its_own() {
    let jobs = Jobs::new();
    let (ran, receiver) = channel();
    jobs.submit("pass", Duration::ZERO, move || {
        ran.send(std::thread::current().id()).unwrap();
    });
    let job_thread = receiver.recv_timeout(WAIT).unwrap();
    assert_ne!(job_thread, std::thread::current().id());
}

#[test]
fn resubmitting_restarts_the_delay_and_runs_only_the_last() {
    let jobs = Jobs::new();
    let (ran, receiver) = channel();
    for name in ["first", "second", "third"] {
        let ran = ran.clone();
        jobs.submit("a", DELAY, move || ran.send(name).unwrap());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(receiver.recv_timeout(WAIT), Ok("third"));
    assert_eq!(runs(&receiver), Vec::<&str>::new());
}

#[test]
fn cancel_drops_a_waiting_job() {
    let jobs = Jobs::new();
    let (ran, receiver) = channel();
    jobs.submit("a", DELAY, move || ran.send("a").unwrap());
    jobs.cancel("a");
    assert_eq!(runs(&receiver), Vec::<&str>::new());
}

#[test]
fn keys_do_not_disturb_each_other() {
    let jobs = Jobs::new();
    let (ran, receiver) = channel();
    let other = ran.clone();
    jobs.submit("a", DELAY, move || ran.send("a").unwrap());
    jobs.submit("b", DELAY, move || other.send("b").unwrap());
    jobs.cancel("c");
    let mut got = vec![
        receiver.recv_timeout(WAIT).unwrap(),
        receiver.recv_timeout(WAIT).unwrap(),
    ];
    got.sort_unstable();
    assert_eq!(got, vec!["a", "b"]);
}

#[test]
fn dropping_jobs_drops_what_is_waiting() {
    let jobs = Jobs::new();
    let (ran, receiver) = channel();
    jobs.submit("a", DELAY, move || ran.send("a").unwrap());
    drop(jobs);
    assert_eq!(runs(&receiver), Vec::<&str>::new());
}
