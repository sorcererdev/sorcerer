//! JSON over HTTP for the providers: retries on 429 and 529, a timeout,
//! and cancel honoured within a tenth of a second.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use sorcerer_core::Cancel;

use crate::{Error, Result};

/// How a JSON POST is sent: the provider's name (for errors), the URL,
/// headers, the timeout, and the base wait before a retry (1 s in use,
/// zero in tests).
pub(crate) struct Post<'a> {
    pub provider: &'a str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
    pub backoff: Duration,
}

/// POSTs BODY and returns the reply's JSON and the seconds it took. 429
/// and 529 are retried twice, after BACKOFF and twice BACKOFF. The request
/// runs on its own thread so CANCEL is honoured within a tenth of a second
/// (the abandoned request finishes or times out on its own).
pub(crate) fn post_json(
    post: &Post,
    body: &serde_json::Value,
    cancel: &Cancel,
) -> Result<(serde_json::Value, f64)> {
    let start = Instant::now();
    for attempt in 0..3u32 {
        if attempt > 0 {
            wait(post.backoff * attempt, cancel)?;
        }
        let (status, text) = send(post, body, cancel)?;
        if (status == 429 || status == 529) && attempt < 2 {
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(Error::Http {
                provider: post.provider.into(),
                status,
                body: text.chars().take(500).collect(),
            });
        }
        let json = serde_json::from_str(&text).map_err(|e| Error::Reply {
            provider: post.provider.into(),
            why: format!("not JSON: {e}"),
        })?;
        return Ok((json, start.elapsed().as_secs_f64()));
    }
    unreachable!("the third attempt returns")
}

fn wait(d: Duration, cancel: &Cancel) -> Result<()> {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(20).min(end - Instant::now()));
    }
    Ok(())
}

fn send(post: &Post, body: &serde_json::Value, cancel: &Cancel) -> Result<(u16, String)> {
    let (tx, rx) = mpsc::channel();
    let client = reqwest::blocking::Client::builder()
        .timeout(post.timeout)
        .build()
        .map_err(|e| Error::Transport {
            provider: post.provider.into(),
            why: e.to_string(),
        })?;
    let mut rb = client
        .post(&post.url)
        .header("content-type", "application/json");
    for (k, v) in &post.headers {
        rb = rb.header(k, v);
    }
    let rb = rb.body(body.to_string());
    std::thread::spawn(move || {
        let r = rb.send().and_then(|resp| {
            let status = resp.status().as_u16();
            resp.text().map(|t| (status, t))
        });
        let _ = tx.send(r);
    });
    loop {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(r)) => return Ok(r),
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
