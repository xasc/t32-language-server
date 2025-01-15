// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    io::Write,
    process::{self, Command, Stdio},
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

#[test]
fn lifecycle_initialize_req() {
    let pid = process::id();

    let mut ls = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            &format!("--clientProcessId={}", pid.to_string()),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must not fail.");

    let content = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": pid,
            "capabilities": {}
        }
    });

    let init = build_msg(&content.to_string());

    let content = json!({
        "jsonrpc": "2.0",
        "id": "2",
        "method": "exit",
    });
    let exit = build_msg(&content.to_string());

    let mut stdin = ls.stdin.take().unwrap();
    stdin.write_all(init.as_bytes()).unwrap();
    stdin.write_all(exit.as_bytes()).unwrap();

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert_eq!(output.status.code(), Some(0));

    let mut ls = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            &format!("--clientProcessId={}", pid.to_string()),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must not fail.");

    let exit = build_msg(&content.to_string());

    let mut stdin = ls.stdin.take().unwrap();
    stdin.write_all(exit.as_bytes()).unwrap();

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert_eq!(output.status.code(), Some(1));
}
