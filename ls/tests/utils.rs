// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    io::Write,
    process::{Child, ChildStdin, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

use serde_json::json;

pub fn start_ls(args: &[&str]) -> Child {
    let mut params = vec!["run", "--quiet", "--"];
    params.extend_from_slice(&args);

    Command::new("cargo")
        .args(params)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must be able to start language server.")
}

pub fn stop_ls(proc: &mut Child, stdin: Option<&mut ChildStdin>, try_shutdown: bool) {
    if let Some(cin) = stdin {
        if try_shutdown {
            let shutdown = make_shutdown_notification(99);

            cin.write_all(shutdown.as_bytes()).unwrap();
            let _ = cin.flush();
        }
        let exit = make_exit_notification(100);

        cin.write_all(exit.as_bytes()).unwrap();
        let _ = cin.flush();
    }

    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Ok(Some(_)) = proc.try_wait() {
            break;
        }
    }
}

pub fn make_exit_notification(id: isize) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "exit",
    });
    build_msg(&content.to_string())
}

pub fn make_shutdown_notification(id: isize) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "shutdown",
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
