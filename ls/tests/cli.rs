// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use t32_language_server;

mod utils;

#[test]
fn prints_help() {
    let ls = utils::start_ls(&["--help"], false);

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert_eq!(
        output.status.code(),
        Some(t32_language_server::ReturnCode::OkExit as i32)
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .starts_with("Usage: t32ls")
    );
}

#[test]
fn prints_version() {
    let ls = utils::start_ls(&["--version"], false);

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert_eq!(
        output.status.code(),
        Some(t32_language_server::ReturnCode::OkExit as i32)
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .starts_with("t32ls (t32-language-server), version")
    );
}
