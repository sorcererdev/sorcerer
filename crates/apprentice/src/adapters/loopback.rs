//! The loopback port: how the agent's helper processes (its MCP server and its hooks, children
//! of the agent and not of the server) reach the server that runs the task. A TCP port on
//! `127.0.0.1` chosen by the system, one JSON line in and one out per connection, and a token of
//! the server's own that every request must carry. Loopback TCP is one code path on Windows,
//! macOS and Linux, where named pipes and Unix sockets would be two.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::domain::loopback_message::{LoopbackReply, LoopbackRequest};
use crate::ports::agent::WayBack;

/// What answers a request whose token is right.
pub type Handler = Arc<dyn Fn(LoopbackRequest) -> LoopbackReply + Send + Sync>;

/// An open loopback port. Dropping it stops answering.
pub struct Loopback {
    way_back: WayBack,
    closed: Arc<AtomicBool>,
}

impl Loopback {
    /// Opens a port on `127.0.0.1` and answers each connection on a thread of its own: a
    /// request with the wrong token is refused unanswered by the handler.
    pub fn open(handler: Handler) -> io::Result<Loopback> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let way_back = WayBack {
            port: listener.local_addr()?.port(),
            token: new_token(),
        };
        let closed = Arc::new(AtomicBool::new(false));
        let (token, stop) = (way_back.token.clone(), closed.clone());
        std::thread::spawn(move || accept_until_closed(&listener, &token, &handler, &stop));
        Ok(Loopback { way_back, closed })
    }

    /// The port and token the helper processes are given.
    pub fn way_back(&self) -> WayBack {
        self.way_back.clone()
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        // wakes the accepting thread, which then sees it is closed
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.way_back.port));
    }
}

fn accept_until_closed(
    listener: &TcpListener,
    token: &str,
    handler: &Handler,
    closed: &AtomicBool,
) {
    for stream in listener.incoming() {
        if closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let (token, handler) = (token.to_string(), handler.clone());
        std::thread::spawn(move || {
            let _ = answer_one(stream, &token, &handler);
        });
    }
}

/// Reads one request line, answers it, closes.
fn answer_one(stream: TcpStream, token: &str, handler: &Handler) -> io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let reply = match serde_json::from_str::<LoopbackRequest>(&line) {
        Ok(request) if request.token == token => handler(request),
        Ok(_) => LoopbackReply::error("wrong token"),
        Err(e) => LoopbackReply::error(format!("not a request: {e}")),
    };
    let mut out = stream;
    writeln!(out, "{}", serde_json::to_string(&reply).unwrap_or_default())?;
    out.flush()
}

/// Sends one request to the server and reads its answer. A question to the person may take
/// minutes, so there is no read timeout; connecting waits at most five seconds.
pub fn ask_loopback(port: u16, request: &LoopbackRequest) -> io::Result<LoopbackReply> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    writeln!(stream, "{}", serde_json::to_string(request)?)?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(io::Error::other)
}

/// 128 random bits as hex, from the standard library's per-process random hasher keys.
fn new_token() -> String {
    let random = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(std::process::id().into());
        hasher.finish()
    };
    format!("{:016x}{:016x}", random(), random())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loopback_message::RequestBody;

    #[test]
    fn a_request_with_the_token_is_answered_and_one_without_is_refused() {
        let handler: Handler =
            Arc::new(|request| LoopbackReply::text(format!("hi {}", request.task)));
        let loopback = Loopback::open(handler).unwrap();
        let way = loopback.way_back();
        let request = |token: &str| LoopbackRequest {
            token: token.into(),
            task: "t1".into(),
            body: RequestBody::Edits {
                event: "post-tool-use".into(),
            },
        };
        assert_eq!(
            ask_loopback(way.port, &request(&way.token)).unwrap(),
            LoopbackReply::text("hi t1")
        );
        assert_eq!(
            ask_loopback(way.port, &request("guess")).unwrap(),
            LoopbackReply::error("wrong token")
        );
        assert_eq!(way.token.len(), 32);
    }
}
