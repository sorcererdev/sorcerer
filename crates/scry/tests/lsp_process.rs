//! `scry lsp` as a process: it ends when the editor dies with a question unanswered.
mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sorcerer_core::adapters::platform::dirs::user_dir_vars;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_lsp::uri_of;
use support::{git_project, write};

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
fn the_process_ends_when_the_editor_hangs_up_with_the_trust_question_unanswered() {
    let dir = git_project();
    let home = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".sorcerer/config.toml",
        "[model.providers.claude]\nrunner = \"/bin/false\"\n",
    );
    write(
        dir.path(),
        "orders.py",
        "def cancel(order):\n    return 1\n",
    );
    let root_path = canonical(dir.path()).unwrap();
    let uri = uri_of(&root_path.join("orders.py")).as_str().to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_scry"))
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
    let root = uri_of(&root_path).as_str().to_string();
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":root,"capabilities":{}}}),
    );
    read_message(&mut out);
    let note =
        |method: &str, params: Value| json!({"jsonrpc":"2.0","method":method,"params":params});
    send(&mut stdin, &note("initialized", json!({})));
    let doc = json!({"textDocument":{"uri":uri,"languageId":"python","version":1,"text":""}});
    send(&mut stdin, &note("textDocument/didOpen", doc.clone()));
    send(&mut stdin, &note("textDocument/didSave", doc));
    loop {
        if read_message(&mut out)["method"] == "window/showMessageRequest" {
            break;
        }
    }
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("scry lsp is still running after its editor hung up");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
