//! JSON over HTTP for the providers: retries when the provider is busy, a timeout,
//! and cancel honoured within a tenth of a second.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use sorcerer_core::Cancel;

use crate::{Error, Result};

/// How many times a request is sent when the provider keeps saying it is busy.
const ATTEMPTS: u32 = 3;

/// How often a request in flight checks whether the user cancelled.
const CANCEL_CHECK: Duration = Duration::from_millis(100);

/// How a JSON POST is sent: the provider's name (it appears in error messages), the URL,
/// the extra headers, how long one request may take, and the wait before the first retry
/// (1 second in use, zero in tests).
pub(crate) struct Post<'a> {
    pub provider: &'a str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
    pub backoff: Duration,
}

/// A provider's reply parsed as JSON, and how many seconds it took, retries included.
pub(crate) struct JsonReply {
    pub json: serde_json::Value,
    pub seconds: f64,
}

/// Sends the body as a JSON POST and returns the reply's JSON.
///
/// A provider that says it is busy (status 429, rate-limited, or 529, overloaded) is asked
/// again, up to 3 times in all, waiting the backoff before the second try and twice the
/// backoff before the third. Any other status outside 200–299 is an error that quotes the
/// first 500 characters of the reply.
///
/// The request runs on its own thread, so a cancel stops the wait within a tenth of a
/// second; the abandoned request finishes or times out on its own.
///
/// Example: with a 1-second backoff, Anthropic answers 529, then 529, then 200: the call
/// returns the third reply after about 3 seconds of waiting.
pub(crate) fn post_json(post: &Post, body: &serde_json::Value, cancel: &Cancel) -> Result<JsonReply> {
    let start = Instant::now();
    let text = send_until_not_busy(post, body, cancel, 1)?;
    let json = serde_json::from_str(&text).map_err(|e| Error::Reply {
        provider: post.provider.into(),
        why: format!("not JSON: {e}"),
    })?;
    Ok(JsonReply {
        json,
        seconds: start.elapsed().as_secs_f64(),
    })
}

/// The status line and body text of one HTTP reply.
struct HttpReply {
    status: u16,
    text: String,
}

impl HttpReply {
    fn provider_is_busy(&self) -> bool {
        self.status == 429 || self.status == 529
    }

    /// The body when the status is a success (200–299), else the error that quotes it.
    fn success_text(self, provider: &str) -> Result<String> {
        if (200..300).contains(&self.status) {
            Ok(self.text)
        } else {
            Err(Error::Http {
                provider: provider.into(),
                status: self.status,
                body: self.text.chars().take(500).collect(),
            })
        }
    }
}

fn send_until_not_busy(
    post: &Post,
    body: &serde_json::Value,
    cancel: &Cancel,
    attempt: u32,
) -> Result<String> {
    let reply = send(post, body, cancel)?;
    if reply.provider_is_busy() && attempt < ATTEMPTS {
        wait(post.backoff * attempt, cancel)?;
        return send_until_not_busy(post, body, cancel, attempt + 1);
    }
    reply.success_text(post.provider)
}

/// Sleeps for the given time in short naps, stopping early with an error on cancel.
fn wait(how_long: Duration, cancel: &Cancel) -> Result<()> {
    let end = Instant::now() + how_long;
    while Instant::now() < end {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(20).min(end - Instant::now()));
    }
    Ok(())
}

fn send(post: &Post, body: &serde_json::Value, cancel: &Cancel) -> Result<HttpReply> {
    let request = build_request(post, body)?;
    let answer = send_on_its_own_thread(request);
    await_answer(post, &answer, cancel)
}

fn build_request(
    post: &Post,
    body: &serde_json::Value,
) -> Result<reqwest::blocking::RequestBuilder> {
    let client = reqwest::blocking::Client::builder()
        .timeout(post.timeout)
        .build()
        .map_err(|e| Error::Transport {
            provider: post.provider.into(),
            why: e.to_string(),
        })?;
    let request = client
        .post(&post.url)
        .header("content-type", "application/json");
    let request = post
        .headers
        .iter()
        .fold(request, |request, (name, value)| request.header(name, value));
    Ok(request.body(body.to_string()))
}

type Answer = mpsc::Receiver<reqwest::Result<HttpReply>>;

fn send_on_its_own_thread(request: reqwest::blocking::RequestBuilder) -> Answer {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reply = request.send().and_then(|response| {
            let status = response.status().as_u16();
            response.text().map(|text| HttpReply { status, text })
        });
        let _ = tx.send(reply);
    });
    rx
}

/// Waits for the request thread's answer, checking for cancel every tenth of a second.
fn await_answer(post: &Post, answer: &Answer, cancel: &Cancel) -> Result<HttpReply> {
    loop {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match answer.recv_timeout(CANCEL_CHECK) {
            Ok(Ok(reply)) => return Ok(reply),
            Ok(Err(e)) if e.is_timeout() => {
                return Err(Error::Timeout {
                    provider: post.provider.into(),
                    seconds: post.timeout.as_secs(),
                });
            }
            Ok(Err(e)) => {
                return Err(Error::Transport {
                    provider: post.provider.into(),
                    why: e.to_string(),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::Transport {
                    provider: post.provider.into(),
                    why: "the request thread died".into(),
                });
            }
        }
    }
}
