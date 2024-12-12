// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io;

use t32_language_server;

#[test]
fn prints_help() {
    let args = vec![String::from("t32-language-server"), String::from("--help")];

    let mut streams = t32_language_server::Stdio {
        reader: io::stdin().lock(),
        writer: Vec::new(),
        error: io::stderr(),
    };
    let rc = t32_language_server::run(args, &mut streams);
    let output = String::from_utf8(streams.writer).expect("Invalid UTF-8");

    assert_eq!(rc, t32_language_server::ReturnCode::OkExit);
    assert!(output.starts_with("Usage: t32-language-server"));
}

#[test]
fn reports_missing_ppid() {
    let args = vec![String::from("t32-language-server")];

    let mut streams = t32_language_server::Stdio {
        reader: io::stdin().lock(),
        writer: io::stdout().lock(),
        error: Vec::new(),
    };
    let rc = t32_language_server::run(args, &mut streams);
    let error = String::from_utf8(streams.error).expect("Invalid UTF-8");

    assert_eq!(rc, t32_language_server::ReturnCode::UsageErr);
    assert_eq!(format!("{error}"), "Error: Missing argument \"--clientProcessId=PID\"\n");
}
