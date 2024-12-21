// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io;

use t32_language_server;

#[test]
fn prints_help() {
    let args = vec![String::from("t32-language-server"), String::from("--help")];

    let mut stdout = Vec::new();
    let streams = t32_language_server::Stdio {
        reader: io::stdin().lock(),
        writer: &mut stdout,
        error: &mut io::stderr(),
    };
    let rc = t32_language_server::run(args, streams);
    let output = String::from_utf8(stdout).expect("Invalid UTF-8");

    assert_eq!(rc, t32_language_server::ReturnCode::OkExit);
    assert!(output.starts_with("Usage: t32-language-server"));
}

#[test]
fn reports_missing_ppid() {
    let args = vec![String::from("t32-language-server")];

    let mut stderr = Vec::new();
    let streams = t32_language_server::Stdio {
        reader: io::stdin().lock(),
        writer: &mut io::stdout().lock(),
        error: &mut stderr,
    };
    let rc = t32_language_server::run(args, streams);
    let error = String::from_utf8(stderr).expect("Invalid UTF-8");

    assert_eq!(rc, t32_language_server::ReturnCode::UsageErr);
    assert_eq!(
        format!("{error}"),
        "Error: Missing argument \"--clientProcessId=PID\"\n"
    );
}
