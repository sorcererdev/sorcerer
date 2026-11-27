//! An agent that plays a script: events it reports, and calls it makes back to the server
//! through the real loopback port (as its MCP server and hooks would), recording each reply.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use apprentice::adapters::loopback::ask_loopback;
use apprentice::domain::agent_event::{AgentEvent, Finished};
use apprentice::domain::loopback_message::{LoopbackReply, LoopbackRequest, RequestBody};
use apprentice::ports::agent::{Agent, NextEvent, Running, TaskRequest};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde_json::{Value, json};

/// One step of the script.
#[derive(Clone)]
pub enum Step {
    Event(AgentEvent),
    /// A medium tool's call, by name without the prefix.
    Tool(&'static str, Value),
    /// The hook after a tool call.
    Hook,
    /// A permission question about a command.
    Bash(&'static str),
    /// Waits for the step's milliseconds.
    Sleep(u64),
    /// The end of the turn, at this cost.
    Finish(f64),
    /// Runs until stopped.
    Hang,
}

pub fn tool_use(name: &str, input: Value) -> Step {
    Step::Event(AgentEvent::ToolUse {
        name: name.to_string(),
        input,
        parent: None,
    })
}

#[derive(Default)]
pub struct ScriptedAgent {
    pub script: Mutex<Vec<Step>>,
    /// The replies to each call, in order.
    pub replies: Arc<Mutex<Vec<LoopbackReply>>>,
    pub requests: Mutex<Vec<TaskRequest>>,
    /// Whether `start` fails.
    pub fails: bool,
    /// Whether the script plays before `start` returns, as an agent that calls back at once
    /// can on a loaded machine.
    pub plays_before_start_returns: bool,
}

impl ScriptedAgent {
    pub fn playing(script: Vec<Step>) -> ScriptedAgent {
        ScriptedAgent {
            script: Mutex::new(script),
            ..ScriptedAgent::default()
        }
    }

    pub fn replies(&self) -> Vec<LoopbackReply> {
        self.replies.lock().unwrap().clone()
    }
}

impl Agent for ScriptedAgent {
    fn start(&self, request: &TaskRequest) -> sorcerer_core::Result<Box<dyn Running>> {
        if self.fails {
            return Err(sorcerer_core::Error::Config {
                file: "claude".into(),
                why: "no claude".into(),
            });
        }
        self.requests.lock().unwrap().push(request.clone());
        let script = std::mem::take(&mut *self.script.lock().unwrap());
        let (sender, events) = crossbeam_channel::unbounded();
        let stopped = Arc::new(AtomicBool::new(false));
        let (request, replies, stop) = (request.clone(), self.replies.clone(), stopped.clone());
        let play = move || {
            let call = |body: RequestBody| {
                let asked = LoopbackRequest {
                    token: request.way_back.token.clone(),
                    task: request.task.clone(),
                    body,
                };
                let reply = ask_loopback(request.way_back.port, &asked).unwrap();
                replies.lock().unwrap().push(reply);
            };
            for step in script {
                match step {
                    Step::Event(event) => {
                        let _ = sender.send(event);
                    }
                    Step::Tool(name, arguments) => call(RequestBody::Tool {
                        name: name.to_string(),
                        arguments,
                    }),
                    Step::Hook => call(RequestBody::Edits {
                        event: "post-tool-use".into(),
                    }),
                    Step::Bash(command) => call(RequestBody::Permission {
                        tool: "Bash".into(),
                        input: json!({ "command": command }),
                    }),
                    Step::Sleep(ms) => std::thread::sleep(Duration::from_millis(ms)),
                    Step::Finish(cost) => {
                        let _ = sender.send(AgentEvent::Finished(Finished {
                            cost_usd: Some(cost),
                            session_id: Some("s1".into()),
                            is_error: false,
                            text: "done".into(),
                            turns: 3,
                        }));
                    }
                    Step::Hang => {
                        while !stop.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
            }
        };
        if self.plays_before_start_returns {
            play();
        } else {
            std::thread::spawn(play);
        }
        Ok(Box::new(ScriptedRunning { events, stopped }))
    }
}

struct ScriptedRunning {
    events: Receiver<AgentEvent>,
    stopped: Arc<AtomicBool>,
}

impl Running for ScriptedRunning {
    fn next_event(&mut self, wait: Duration) -> NextEvent {
        match self.events.recv_timeout(wait) {
            Ok(event) => NextEvent::Event(event),
            Err(RecvTimeoutError::Timeout) => NextEvent::Waiting,
            Err(RecvTimeoutError::Disconnected) => NextEvent::Ended,
        }
    }

    fn close_input(&mut self) {}

    fn stop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}
