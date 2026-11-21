//! Stands in for `claude` in tests, on every platform. It records its arguments, its input,
//! its folder and two variables into the files the test names, waits as long as it is told,
//! then prints a canned reply.
//!
//! The variables it reads (each optional):
//!
//! - `FAKE_CLAUDE_ARGS`, `FAKE_CLAUDE_STDIN`, `FAKE_CLAUDE_ENV`: files to write the arguments
//!   (one per line), the input, and `CLAUDECODE=…`, `MDS=…`, `PWD=…`, `PID=…` (its own process
//!   id) into.
//! - `FAKE_CLAUDE_SLEEP`: seconds to wait before replying, such as `5` or `7.3`.
//! - `FAKE_CLAUDE_REPLY`: what to print instead of the default JSON reply.
//! - `FAKE_CLAUDE_CHILD_PID`: a file; with a sleep, the fake first starts a copy of itself that
//!   waits as long, and writes that copy's process id into the file, so a test can check that
//!   stopping the fake stopped what it started.

use std::io::Read;
use std::time::Duration;

use sorcerer_core::adapters::platform::paths::canonical;

const DEFAULT_REPLY: &str = r#"{"result":"ok","total_cost_usd":0.01,"duration_ms":1500,"is_error":false,"usage":{"input_tokens":3,"output_tokens":4}}"#;

fn main() {
    record_arguments();
    record_input();
    record_environment();
    let sleep = seconds_to_sleep();
    let copy = (!sleep.is_zero())
        .then(start_a_sleeping_copy_when_asked)
        .flatten();
    std::thread::sleep(sleep);
    let reply = std::env::var("FAKE_CLAUDE_REPLY").unwrap_or_else(|_| DEFAULT_REPLY.into());
    print!("{reply}");
    if let Some(mut copy) = copy {
        let _ = copy.kill();
        let _ = copy.wait();
    }
}

fn record_arguments() {
    let args: String = std::env::args().skip(1).map(|arg| arg + "\n").collect();
    write_to("FAKE_CLAUDE_ARGS", &args);
}

fn record_input() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    write_to("FAKE_CLAUDE_STDIN", &input);
}

fn record_environment() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| "unset".into());
    let folder = std::env::current_dir()
        .and_then(|dir| canonical(&dir))
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    let text = format!(
        "CLAUDECODE={}\nMDS={}\nPWD={folder}\nPID={}\n",
        var("CLAUDECODE"),
        var("CLAUDE_CODE_DISABLE_CLAUDE_MDS"),
        std::process::id()
    );
    write_to("FAKE_CLAUDE_ENV", &text);
}

fn seconds_to_sleep() -> Duration {
    std::env::var("FAKE_CLAUDE_SLEEP")
        .ok()
        .and_then(|text| text.parse::<f64>().ok())
        .map_or(Duration::ZERO, Duration::from_secs_f64)
}

/// The copy sleeps as long as this fake, records nothing (so it does not overwrite the test's
/// files) and prints nothing; its output is this fake's, so it holds the output pipe open as
/// a real runner's child would.
fn start_a_sleeping_copy_when_asked() -> Option<std::process::Child> {
    let pid_file = std::env::var("FAKE_CLAUDE_CHILD_PID").ok()?;
    let me = std::env::current_exe().expect("the fake knows where it is");
    let copy = std::process::Command::new(me)
        .env_remove("FAKE_CLAUDE_CHILD_PID")
        .env_remove("FAKE_CLAUDE_ARGS")
        .env_remove("FAKE_CLAUDE_STDIN")
        .env_remove("FAKE_CLAUDE_ENV")
        .env("FAKE_CLAUDE_REPLY", "")
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("the fake starts a copy of itself");
    std::fs::write(pid_file, copy.id().to_string()).expect("the pid file is writable");
    Some(copy)
}

fn write_to(variable: &str, text: &str) {
    if let Ok(file) = std::env::var(variable) {
        std::fs::write(file, text).expect("the fake's record file is writable");
    }
}
