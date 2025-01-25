// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{io::Write, process};

use serde_json::json;

mod utils;

#[test]
fn supports_lifecycle_initialize_req() {
    let pid = process::id();

    let mut ls = utils::start_ls(&[&format!("--clientProcessId={}", pid.to_string())], true);
    let mut stdin = ls.stdin.take().unwrap();

    utils::stop_ls(&mut ls, Some(&mut stdin), None);
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(1));

    let mut ls = utils::start_ls(&[&format!("--clientProcessId={}", pid.to_string())], true);
    let mut stdin = ls.stdin.take().unwrap();

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_lifecycle_shutdown_req() {
    let mut ls = utils::start_ls(
        &[&format!("--clientProcessId={}", process::id().to_string())],
        true,
    );
    let mut stdin = ls.stdin.take().unwrap();

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert!(std::str::from_utf8(&output.stdout)
        .unwrap()
        .contains(r#"{"jsonrpc":"2.0","id":2,"result":null}"#));
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_lifecycle_exit_notification() {
    let mut ls = utils::start_ls(
        &[&format!("--clientProcessId={}", process::id().to_string())],
        true,
    );
    let mut stdin = ls.stdin.take().unwrap();

    utils::stop_ls(&mut ls, Some(&mut stdin), None);
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn exits_on_missing_parent_process() {
    let mut ls = utils::start_ls(&[&format!("--clientProcessId={}", 1)], true);

    utils::stop_ls(&mut ls, None, None);
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(69));
}

#[test]
fn exits_on_wrong_parent_pid() {
    let pid = process::id();

    let mut ls = utils::start_ls(&[&format!("--clientProcessId={}", pid)], false);

    let init = utils::make_initialize_request(1, 2);

    let mut stdin = ls.stdin.take().unwrap();
    stdin.write_all(init.as_bytes()).unwrap();

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(
        output.status.code(),
        Some(t32_language_server::ReturnCode::ProtcolErr as i32)
    );

    assert!(std::str::from_utf8(&output.stdout)
        .unwrap()
        .contains("Error: Process ID of the parent process 2 is different"));

    let mut ls = utils::start_ls(&[&format!("--clientProcessId={}", pid)], false);

    let content = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": isize::MAX,
            "capabilities": {}
        }
    });
    let init = utils::build_msg(&content.to_string());

    let mut stdin = ls.stdin.take().unwrap();
    stdin.write_all(init.as_bytes()).unwrap();

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(
        output.status.code(),
        Some(t32_language_server::ReturnCode::ProtcolErr as i32)
    );

    assert!(std::str::from_utf8(&output.stdout)
        .unwrap()
        .contains("Error: Process ID of the parent process"));
    assert!(std::str::from_utf8(&output.stdout)
        .unwrap()
        .contains("invalid"));
}
