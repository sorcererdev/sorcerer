//! Starting a program so that stopping it stops everything it started.
//!
//! A runner such as `claude` starts programs of its own: on Windows `claude.cmd` is npm's shim,
//! which runs `cmd.exe`, which runs `node`. Killing only the first process would leave the rest
//! running, still holding the output pipe open. So each program is started in a group of its
//! own (a process group on Unix, a job object on Windows), and stopping it ends the whole
//! group.

use std::io;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};

use process_wrap::std::{ChildWrapper, CommandWrap};

/// A program started by [`start`], with everything it starts in turn.
#[derive(Debug)]
pub struct Running {
    child: Box<dyn ChildWrapper>,
}

/// Starts the command in a group of its own. The command's pipes, folder and environment are
/// kept as set. Example: `start(Command::new("claude"))` runs `claude`, and a later
/// [`Running::stop`] ends `claude` and every program it started.
pub fn start(command: Command) -> io::Result<Running> {
    let mut wrapped = CommandWrap::from(command);
    in_a_group_of_its_own(&mut wrapped);
    Ok(Running {
        child: wrapped.spawn()?,
    })
}

#[cfg(unix)]
fn in_a_group_of_its_own(command: &mut CommandWrap) {
    command.wrap(process_wrap::std::ProcessGroup::leader());
}

#[cfg(windows)]
fn in_a_group_of_its_own(command: &mut CommandWrap) {
    command.wrap(process_wrap::std::JobObject);
}

impl Running {
    /// The program's input, when it was piped and not taken yet.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin().take()
    }

    /// The program's output, when it was piped and not taken yet.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout().take()
    }

    /// The program's error output, when it was piped and not taken yet.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr().take()
    }

    /// How the program exited, or `None` while it is still running. Does not wait.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// The operating system's id for the program (the first process, not those it started).
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Ends the program and everything it started, and waits until they are gone. Prints
    /// nothing: in an editor's server, standard output is the protocol.
    pub fn stop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait();
    }
}

/// Whether a process with this id is running, for tests that check a stop reached a child.
/// A process that has exited but not been collected yet (a zombie on Unix) is not running.
#[doc(hidden)]
pub fn is_running(pid: u32) -> bool {
    process_listed(pid)
}

/// Asks `ps` for the process's state: no line means no process, `Z` means a zombie.
#[cfg(unix)]
fn process_listed(pid: u32) -> bool {
    Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .map(|out| {
            let state = String::from_utf8_lossy(&out.stdout);
            let state = state.trim();
            !state.is_empty() && !state.starts_with('Z')
        })
        .unwrap_or(false)
}

/// Asks `tasklist` for the process: it prints a row naming the id while the process exists.
#[cfg(windows)]
fn process_listed(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_started_program_is_running_until_stopped() {
        let mut running = start(long_sleep()).unwrap();
        let pid = running.id();
        assert!(is_running(pid));
        running.stop();
        assert!(!is_running(pid));
    }

    #[test]
    fn a_program_that_exits_reports_its_status() {
        let mut running = start(quick_exit()).unwrap();
        let status = loop {
            if let Some(status) = running.try_wait().unwrap() {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(status.success());
    }

    #[cfg(unix)]
    fn long_sleep() -> Command {
        let mut command = Command::new("sleep");
        command.arg("30");
        command
    }

    #[cfg(windows)]
    fn long_sleep() -> Command {
        let mut command = Command::new("ping");
        command.args(["-n", "30", "127.0.0.1"]);
        command.stdout(std::process::Stdio::null());
        command
    }

    #[cfg(unix)]
    fn quick_exit() -> Command {
        Command::new("true")
    }

    #[cfg(windows)]
    fn quick_exit() -> Command {
        let mut command = Command::new("cmd");
        command.args(["/C", "exit 0"]);
        command
    }
}
