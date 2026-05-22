// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{thread, time};

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

#[test]
fn reports_invalid_t32_sys_dir() {
    let mut ls = utils::start_ls_and_capture_stderr(
        &[
            "--clientProcessId=0",
            "--t32SystemDir=tests/samples/invalid",
        ],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    utils::stop_ls(&mut ls, Some(&mut stdin), None);

    thread::sleep(time::Duration::from_millis(100));

    let output = ls.wait_with_output().expect("Failed to capture output");
    dbg!(str::from_utf8(&output.stdout).unwrap());

    assert!(str::from_utf8(&output.stderr).unwrap().contains("WARNING:"));
    assert!(
        str::from_utf8(&output.stderr)
            .unwrap()
            .contains("does not exist.")
    );
    assert!(
        str::from_utf8(&output.stderr)
            .unwrap()
            .contains("--t32SystemDir")
    );
}

#[test]
fn reports_invalid_t32_temp_dir() {
    let mut ls = utils::start_ls_and_capture_stderr(
        &["--clientProcessId=0", "--t32TempDir=tests/samples/invalid"],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    utils::stop_ls(&mut ls, Some(&mut stdin), None);

    thread::sleep(time::Duration::from_millis(100));

    let output = ls.wait_with_output().expect("Failed to capture output");

    assert!(str::from_utf8(&output.stderr).unwrap().contains("WARNING:"));
    assert!(
        str::from_utf8(&output.stderr)
            .unwrap()
            .contains("does not exist.")
    );
    assert!(
        str::from_utf8(&output.stderr)
            .unwrap()
            .contains("--t32TempDir")
    );
}
