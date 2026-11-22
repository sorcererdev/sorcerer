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
    /// How the program ended, once [`Running::try_wait`] or [`Running::stop`] has seen it end.
    ended: Option<ExitStatus>,
}

/// Starts the command in a group of its own. The command's pipes, folder and environment are
/// kept as set. Example: `start(Command::new("claude"))` runs `claude`, and a later
/// [`Running::stop`] ends `claude` and every program it started.
pub fn start(command: Command) -> io::Result<Running> {
    let mut wrapped = CommandWrap::from(command);
    in_a_group_of_its_own(&mut wrapped);
    Ok(Running {
        child: wrapped.spawn()?,
        ended: None,
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

    /// How the program exited, or `None` while it is still running. Does not wait. Once it
    /// has answered with the exit, it answers the same again without asking the system.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.ended.is_none() {
            self.ended = self.child.try_wait()?;
        }
        Ok(self.ended)
    }

    /// The operating system's id for the program (the first process, not those it started).
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Ends the program and everything it started, and waits until they are gone. Prints
    /// nothing: in an editor's server, standard output is the protocol.
    ///
    /// A program already seen to end (by [`Running::try_wait`] or an earlier `stop`) is left
    /// alone: on Unix its group id may since belong to another program, and on Windows waiting
    /// on its job again could wait forever.
    pub fn stop(&mut self) {
        if self.ended.is_some() {
            return;
        }
        let _ = self.child.start_kill();
        if let Ok(status) = self.child.wait() {
            self.ended = Some(status);
        }
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
    fn stopping_twice_or_after_the_exit_returns_at_once() {
        let mut stopped = start(long_sleep()).unwrap();
        stopped.stop();
        let first = stopped.try_wait().unwrap();
        assert!(first.is_some());
        stopped.stop();
        assert_eq!(stopped.try_wait().unwrap(), first);

        let mut exited = start(quick_exit()).unwrap();
        while exited.try_wait().unwrap().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let status = exited.try_wait().unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            exited.stop();
            let _ = done.send(exited.try_wait().unwrap());
        });
        let after_stop = finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("stop after the exit returns at once");
        assert_eq!(after_stop, status);
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

    /// The chain npm's `claude.cmd` makes: `cmd.exe` runs a program that starts a long-running
    /// child. Stopping the first program must end the child too.
    #[cfg(windows)]
    #[test]
    fn stopping_a_cmd_script_also_stops_the_child_it_started() {
        let folder = tempfile::TempDir::new().unwrap();
        let pid_file = folder.path().join("child.pid");
        let script = folder.path().join("starts a child.cmd");
        std::fs::write(
            &script,
            "@echo off\r\npowershell -NoProfile -Command \"$child = Start-Process ping \
             -ArgumentList '-n','60','127.0.0.1' -PassThru -WindowStyle Hidden; \
             Set-Content -Path $env:CHILD_PID_FILE -Value $child.Id; \
             Wait-Process -Id $child.Id\"\r\n",
        )
        .unwrap();
        let mut command = Command::new(&script);
        command.env("CHILD_PID_FILE", &pid_file);
        command.stdout(std::process::Stdio::null());
        let mut running = start(command).unwrap();

        let child_pid = wait_for_pid_in(&pid_file);
        assert!(is_running(child_pid));
        running.stop();
        let gone = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            !is_running(child_pid)
        });
        assert!(gone, "the child {child_pid} outlived the stop");
    }

    /// The process id the script writes once its child runs, waiting up to 30 seconds.
    #[cfg(windows)]
    fn wait_for_pid_in(pid_file: &std::path::Path) -> u32 {
        for _ in 0..300 {
            if let Ok(pid) = std::fs::read_to_string(pid_file)
                .unwrap_or_default()
                .trim()
                .parse()
            {
                return pid;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("the script never recorded its child's pid");
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
