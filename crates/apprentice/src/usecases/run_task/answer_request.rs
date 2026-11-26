//! A helper process's request, answered for the live task it names.

use crate::domain::loopback_message::{LoopbackReply, RequestBody};
use crate::domain::mediums::{PERMISSION_TOOL, TOOL_PREFIX};

use super::feed::edits_for_agent;
use super::permissions::answer_permission;
use super::task_deps::{LiveTask, TaskDeps};
use super::tool_calls::answer_tool_call;

/// Answers a request from one of the task's helper processes: the person's edits (a hook), a
/// medium's call or a permission question (the MCP server). A task that is over answers
/// nothing more.
pub fn answer_request(live: &LiveTask, body: &RequestBody, deps: &TaskDeps) -> LoopbackReply {
    if !live.is_live() {
        return LoopbackReply::error("the task is over");
    }
    match body {
        RequestBody::Edits { event } => edits_for_agent(live, event, deps),
        RequestBody::Tool { name, arguments } => {
            let name = name.strip_prefix(TOOL_PREFIX).unwrap_or(name);
            if name == PERMISSION_TOOL {
                let tool = arguments["tool_name"].as_str().unwrap_or_default();
                return answer_permission(live, tool, &arguments["input"], deps);
            }
            answer_tool_call(live, name, arguments, deps)
        }
        RequestBody::Permission { tool, input } => answer_permission(live, tool, input, deps),
    }
}
