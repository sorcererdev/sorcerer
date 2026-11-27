//! The loopback port: how the agent's helper processes (its MCP server and its hooks, children
//! of the agent and not of the server) reach the server that runs the task. A TCP port on
//! `127.0.0.1` chosen by the system, one JSON line in and one out per connection, and a token of
//! the server's own that every request must carry. Loopback TCP is one code path on Windows,
//! macOS and Linux, where named pipes and Unix sockets would be two.
//!
//! Any local process can connect, so a peer is bounded: its request line must arrive within a
//! few seconds and be at most 16 MiB, and at most 64 connections are answered at once; a peer
//! past these is closed unanswered.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use sorcerer_core::adapters::platform::local_connect::connect_local;

use crate::domain::loopback_message::{LoopbackReply, LoopbackRequest};
use crate::ports::agent::WayBack;

/// What answers a request whose token is right.
pub type Handler = Arc<dyn Fn(LoopbackRequest) -> LoopbackReply + Send + Sync>;

/// How much a peer may take of the server.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// The request line must arrive within this (the answer may take minutes: a question).
    read_within: Duration,
    /// The longest request line, in bytes (a `write_file` carries a whole file).
    longest_line: u64,
    /// The connections answered at once.
    connections: usize,
}

const LIMITS: Limits = Limits {
    read_within: Duration::from_secs(5),
    longest_line: 16 * 1024 * 1024,
    connections: 64,
};

/// An open loopback port. Dropping it stops answering.
pub struct Loopback {
    way_back: WayBack,
    closed: Arc<AtomicBool>,
}

impl Loopback {
    /// Opens a port on `127.0.0.1` and answers each connection on a thread of its own: a
    /// request with the wrong token is refused unanswered by the handler.
    pub fn open(handler: Handler) -> io::Result<Loopback> {
        Loopback::open_with(handler, LIMITS)
    }

    fn open_with(handler: Handler, limits: Limits) -> io::Result<Loopback> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let way_back = WayBack {
            port: listener.local_addr()?.port(),
            token: new_token()?,
        };
        let closed = Arc::new(AtomicBool::new(false));
        let (token, stop) = (way_back.token.clone(), closed.clone());
        std::thread::spawn(move || accept_until_closed(&listener, &token, &handler, &stop, limits));
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
    limits: Limits,
) {
    let answering = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        if answering.fetch_add(1, Ordering::SeqCst) >= limits.connections {
            answering.fetch_sub(1, Ordering::SeqCst);
            continue; // dropped: closed unanswered
        }
        let (token, handler, answering) = (token.to_string(), handler.clone(), answering.clone());
        std::thread::spawn(move || {
            let _ = answer_one(stream, &token, &handler, limits);
            answering.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Reads one request line (within the time and length limits), answers it, closes.
fn answer_one(stream: TcpStream, token: &str, handler: &Handler, limits: Limits) -> io::Result<()> {
    stream.set_read_timeout(Some(limits.read_within))?;
    let mut line = String::new();
    BufReader::new(stream.try_clone()?.take(limits.longest_line + 1)).read_line(&mut line)?;
    if line.len() as u64 > limits.longest_line {
        return Err(io::Error::other("request too long")); // closed unanswered
    }
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
/// minutes, so there is no read timeout; connecting waits at most five seconds. A port nothing
/// listens on is refused at once, on Windows too (see [`connect_local`]).
pub fn ask_loopback(port: u16, request: &LoopbackRequest) -> io::Result<LoopbackReply> {
    let mut stream = connect_local(port, Duration::from_secs(5))?;
    writeln!(stream, "{}", serde_json::to_string(request)?)?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(io::Error::other)
}

/// 128 random bits from the operating system's random source, as hex.
fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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

    #[test]
    fn tokens_are_random_hex() {
        let (one, two) = (new_token().unwrap(), new_token().unwrap());
        assert_ne!(one, two);
        assert!(one.len() == 32 && one.chars().all(|c| c.is_ascii_hexdigit()));
    }

    fn small_limits() -> Limits {
        Limits {
            read_within: Duration::from_millis(200),
            longest_line: 64,
            connections: 2,
        }
    }

    fn echo() -> Handler {
        Arc::new(|request| LoopbackReply::text(request.task))
    }

    #[test]
    fn a_peer_that_sends_nothing_is_dropped_after_the_read_timeout() {
        let loopback = Loopback::open_with(echo(), small_limits()).unwrap();
        let mut idle = TcpStream::connect((Ipv4Addr::LOCALHOST, loopback.way_back().port)).unwrap();
        idle.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let started = std::time::Instant::now();
        let mut answer = String::new();
        let _ = BufReader::new(&mut idle).read_line(&mut answer);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        assert!(answer.is_empty(), "{answer}");
    }

    #[test]
    fn a_line_longer_than_the_cap_is_closed_unanswered() {
        let asked = Arc::new(AtomicBool::new(false));
        let seen = asked.clone();
        let handler: Handler = Arc::new(move |request| {
            seen.store(true, Ordering::SeqCst);
            LoopbackReply::text(request.task)
        });
        let loopback = Loopback::open_with(handler, small_limits()).unwrap();
        let mut long = TcpStream::connect((Ipv4Addr::LOCALHOST, loopback.way_back().port)).unwrap();
        let _ = writeln!(long, "{}", "x".repeat(1000));
        let mut answer = String::new();
        let _ = BufReader::new(long).read_line(&mut answer);
        assert!(answer.is_empty(), "{answer}");
        assert!(!asked.load(Ordering::SeqCst));
    }

    /// The held connections never send, and the read timeout is long (five seconds), so the
    /// extra one is past the cap for the whole test: it must be closed at once, unanswered,
    /// not left waiting for the read timeout.
    #[test]
    fn connections_past_the_cap_are_closed_unanswered() {
        let limits = Limits {
            read_within: Duration::from_secs(5),
            ..small_limits()
        };
        let loopback = Loopback::open_with(echo(), limits).unwrap();
        let port = loopback.way_back().port;
        let _held: Vec<TcpStream> = (0..2)
            .map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap())
            .collect();
        let mut extra = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        extra
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let started = std::time::Instant::now();
        let mut answer = String::new();
        let _ = BufReader::new(&mut extra).read_line(&mut answer);
        assert!(answer.is_empty(), "{answer}");
        // well under the five-second read timeout, with room for a loaded machine
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }
}
