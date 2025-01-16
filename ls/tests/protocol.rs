// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    io::Write,
    process::{self, Child, Command, Stdio},
};

use serde_json::json;
use t32_language_server;

fn build_msg(content: &str) -> String {
    format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        content.len(),
        content
    )
}

fn make_initialize_request(id: isize, pid: u32) -> String {
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

fn make_exit_notification(id: isize) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "exit",
    });
    build_msg(&content.to_string())
}

fn start_ls(args: &[&str]) -> Child {
    let mut params = vec!["run", "--quiet", "--"];
    params.extend_from_slice(&args);

    Command::new("cargo")
        .args(params)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must be able to start language server.")
}

#[test]
fn supports_lifecycle_initialize_req() {
    let pid = process::id();

    let mut ls = start_ls(&[&format!("--clientProcessId={}", pid.to_string())]);

    let init = make_initialize_request(1, pid);
    let exit = make_exit_notification(2);

    let mut stdin = ls.stdin.take().unwrap();
    stdin.write_all(init.as_bytes()).unwrap();
    stdin.write_all(exit.as_bytes()).unwrap();

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_lifecycle_exit_notification() {
    let mut ls = start_ls(&[&format!("--clientProcessId={}", process::id().to_string())]);

    let exit = make_exit_notification(1);

    let mut stdin = ls.stdin.take().unwrap();
    stdin.write_all(exit.as_bytes()).unwrap();

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn exits_on_missing_parent_process() {
    let ls = start_ls(&[&format!("--clientProcessId={}", 1)]);

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(1));
}
