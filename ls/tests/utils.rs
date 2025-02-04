// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    io::Write,
    process::{self, Child, ChildStdin, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::json;

pub fn start_ls(args: &[&str], try_initialize: bool) -> Child {
    let mut params = vec!["run", "--quiet", "--"];
    params.extend_from_slice(&args);

    let mut ls = Command::new("cargo")
        .args(params)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must be able to start language server.");

    if try_initialize {
        if let Some(cin) = &mut ls.stdin {
            let pid = process::id();

            let init = make_initialize_request(1, pid);

            to_stdin(cin, &init);
        }
    }
    ls
}

#[allow(dead_code)]
pub fn stop_ls(proc: &mut Child, stdin: Option<&mut ChildStdin>, try_shutdown: Option<isize>) {
    if let Some(cin) = stdin {
        if let Some(id) = try_shutdown {
            let shutdown = make_shutdown_request(id);

            cin.write_all(shutdown.as_bytes()).unwrap();
            let _ = cin.flush();
        }
        let exit = make_exit_notification();

        to_stdin(cin, &exit);
    }

    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Ok(Some(_)) = proc.try_wait() {
            break;
        }
    }
}

pub fn make_initialize_request(id: isize, pid: u32) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": pid,
            "capabilities": {}
        }
    });
    build_msg(&content.to_string())
}

fn make_exit_notification() -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "exit",
    });
    build_msg(&content.to_string())
}

fn make_shutdown_request(id: isize) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "shutdown",
    });
    build_msg(&content.to_string())
}

pub fn make_did_open_text_doc_notification() -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///c:/project/test.cmm",
                "languageId": "practice",
                "version": 1,
                "text": "This is a test.",
            }
        }
    });
    build_msg(&content.to_string())
}

pub fn build_msg(content: &str) -> String {
    format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        content.len(),
        content
    )
}

pub fn to_stdin(cin: &mut ChildStdin, msg: &str) {
    let _ = cin.write_all(msg.as_bytes());
    let _ = cin.flush();
}
