//! The answer to one request from the editor, which a product may send after the call that
//! received the request has returned (from a callback, say, once the editor replied to a
//! question of the server's own).

use crossbeam_channel::Sender;
use lsp_server::{ErrorCode, Message, RequestId, Response, ResponseError};
use serde_json::Value;

/// Where the answer to one request goes. Sending it consumes it, so a request is answered once;
/// one dropped without an answer answers with an error, so the editor never waits forever.
///
/// Example: a product lands a change with [`crate::Client::apply_edit`] and, in its reply
/// callback, sends `reply.send(Ok(json!({ "changed": ["a.txt"] })))`.
pub struct Reply {
    id: RequestId,
    sender: Option<Sender<Message>>,
}

impl Reply {
    pub(crate) fn new(id: RequestId, sender: Sender<Message>) -> Reply {
        Reply {
            id,
            sender: Some(sender),
        }
    }

    /// Answers the request.
    pub fn send(mut self, result: Result<Value, ResponseError>) {
        self.answer(result);
    }

    fn answer(&mut self, result: Result<Value, ResponseError>) {
        if let Some(sender) = self.sender.take() {
            let response = Response {
                id: self.id.clone(),
                response_result: result,
            };
            // The editor may be gone; nothing is left to tell.
            let _ = sender.send(Message::Response(response));
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.answer(Err(ResponseError {
            code: ErrorCode::InternalError as i32,
            message: "the server dropped this request without an answer".into(),
            data: None,
        }));
    }
}
