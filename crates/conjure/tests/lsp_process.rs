//! `conjure lsp` as a process: the handshake over standard input and output, and the end.
mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sorcerer_core::adapters::platform::dirs::user_dir_vars;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_lsp::uri_of;
use support::git_project;

fn send(stdin: &mut impl Write, message: &Value) {
    let body = message.to_string();
    write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    stdin.flush().unwrap();
}

fn read_message(out: &mut impl BufRead) -> Value {
    let mut length = 0;
    loop {
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        match line.trim().strip_prefix("Content-Length: ") {
            Some(n) => length = n.parse().unwrap(),
            None if line.trim().is_empty() => break,
            None => {}
        }
    }
    let mut body = vec![0; length];
    out.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn conjure_lsp_serves_the_editor_and_exits_0() {
    let dir = git_project();
    let home = tempfile::tempdir().unwrap();
    let root = uri_of(&canonical(dir.path()).unwrap()).as_str().to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_conjure"))
        .arg("lsp")
        .current_dir(dir.path())
        .envs(user_dir_vars(home.path()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":root,"capabilities":{}}}),
    );
    let answer = read_message(&mut out);
    let commands = &answer["result"]["capabilities"]["executeCommandProvider"]["commands"];
    assert!(
        commands
            .as_array()
            .unwrap()
            .contains(&json!("conjure.conjure")),
        "{answer}"
    );
    let note =
        |method: &str, params: Value| json!({"jsonrpc":"2.0","method":method,"params":params});
    send(&mut stdin, &note("initialized", json!({})));
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
    );
    read_message(&mut out);
    send(&mut stdin, &note("exit", Value::Null));
    let status = child.wait().unwrap();
    assert!(status.success(), "{status}");
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    assert!(rest.is_empty(), "{rest}");
}
