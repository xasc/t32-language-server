// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::process::{Command, Stdio};


use t32_language_server;

#[test]
fn prints_help() {
    let args = vec!["run".to_string(), "--quiet".to_string(), "--".to_string(), String::from("--help")];

    let ls = Command::new("cargo")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must not fail.");

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert_eq!(output.status.code(), Some(t32_language_server::ReturnCode::OkExit as i32));
    assert!(std::str::from_utf8(&output.stdout).unwrap().starts_with("Usage: t32-language-server"));
}

#[test]
fn reports_missing_ppid() {
    let args = vec!["run".to_string(), "--quiet".to_string(), "--".to_string()];

    let ls = Command::new("cargo")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must not fail.");

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert_eq!(output.status.code(), Some(t32_language_server::ReturnCode::UsageErr as i32));
    assert_eq!(
        std::str::from_utf8(&output.stdout).unwrap(),
        "Error: Missing argument \"--clientProcessId=PID\"\n"
    );
}
