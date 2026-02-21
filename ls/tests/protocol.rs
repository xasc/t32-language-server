// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{env, process, thread, time};

use serde_json::json;
use url::Url;

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

    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains(r#"{"jsonrpc":"2.0","id":2,"result":null}"#)
    );
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
    utils::to_stdin(&mut stdin, &init);

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(
        output.status.code(),
        Some(t32_language_server::ReturnCode::ProtocolErr as i32)
    );

    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("ERROR: Process ID of the parent process 2 is different")
    );

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
    utils::to_stdin(&mut stdin, &init);

    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(
        output.status.code(),
        Some(t32_language_server::ReturnCode::ProtocolErr as i32)
    );

    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("ERROR: Process ID of the parent process")
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("invalid")
    );
}

#[test]
fn can_index_workspace() {
    let pid = process::id();

    let mut ls = utils::start_ls(
        &[
            &format!("--clientProcessId={}", pid.to_string()),
            &format!("--trace={}", "messages"),
        ],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let init = utils::make_initialize_request_with_multi_root_workspace(1, pid);
    utils::to_stdin(&mut stdin, &init);

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_millis(2000));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("samples/a/a.cmm")
    );

    let mut ls = utils::start_ls(
        &[
            &format!("--clientProcessId={}", pid.to_string()),
            &format!("--trace={}", "messages"),
        ],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let init = utils::make_initialize_request_with_root_uri(2, pid);
    utils::to_stdin(&mut stdin, &init);

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_millis(2000));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(3));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("samples/b/b.cmm")
    );

    let mut ls = utils::start_ls(
        &[
            &format!("--clientProcessId={}", pid.to_string()),
            &format!("--trace={}", "messages"),
        ],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let init = utils::make_initialize_request_with_root_path(3, pid);
    utils::to_stdin(&mut stdin, &init);

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_millis(2000));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(4));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("samples/a/d/d.cmmt")
    );
}

#[test]
fn reports_invalid_workspace_roots() {
    let pid = process::id();

    let mut ls = utils::start_ls(
        &[
            &format!("--clientProcessId={}", pid.to_string()),
            &format!("--trace={}", "messages"),
        ],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let init = utils::make_initialize_request_with_invalid_multi_root_workspace(1, pid);
    utils::to_stdin(&mut stdin, &init);

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_millis(2000));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(1));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("samples/c.cmm")
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("tests/__invalid__")
    );
}

#[test]
fn supports_docsync_did_open_notification() {
    let mut ls = utils::start_ls(
        &[&format!("--clientProcessId={}", process::id().to_string())],
        true,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    let notif = utils::make_did_open_text_doc_notification();
    utils::to_stdin(&mut stdin, &notif);

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_docsync_did_change_notification() {
    let mut ls = utils::start_ls(
        &[&format!("--clientProcessId={}", process::id().to_string())],
        true,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    let notif = utils::make_did_open_text_doc_notification();
    utils::to_stdin(&mut stdin, &notif);

    let notif = utils::make_did_close_text_doc_notification();
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_secs(1));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_docsync_did_close_notification() {
    let mut ls = utils::start_ls(
        &[&format!("--clientProcessId={}", process::id().to_string())],
        true,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    let notif = utils::make_did_open_text_doc_notification();
    utils::to_stdin(&mut stdin, &notif);

    let notif = utils::make_did_change_text_doc_notification();
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_secs(1));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_did_rename_files_notification() {
    let pid = process::id();

    let mut ls = utils::start_ls(
        &[&format!("--clientProcessId={}", process::id().to_string())],
        false,
    );
    let mut stdin = ls.stdin.take().unwrap();

    let init = utils::make_initialize_request_with_root_uri(1, pid);
    utils::to_stdin(&mut stdin, &init);

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    let notif = utils::make_did_rename_files_notification();
    utils::to_stdin(&mut stdin, &notif);

    thread::sleep(time::Duration::from_secs(1));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("File rename request with ID")
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn supports_lang_goto_definition_request() {
    for ((dir, line, character), (start, end)) in [
        (
            env::current_dir()
                .unwrap()
                .join("tests")
                .join("samples")
                .join("a")
                .join("a.cmm"),
            139,
            12,
        ),
        (
            env::current_dir()
                .unwrap()
                .join("tests")
                .join("samples")
                .join("a")
                .join("a.cmm"),
            22,
            14,
        ),
    ]
    .into_iter()
    .zip([((22, 21), (22, 10)), ((18, 12), (18, 25))])
    {
        let mut ls = utils::start_ls_with_workspace(&[
            &format!("--clientProcessId={}", process::id().to_string()),
            &format!("--trace={}", "messages"),
        ]);
        let mut stdin = ls.stdin.take().unwrap();

        let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
        utils::to_stdin(&mut stdin, &notif);

        let uri = Url::from_file_path(dir).expect("Must not fail.");

        let notif = utils::make_goto_definition_request(2, uri, line, character);
        utils::to_stdin(&mut stdin, &notif);
        thread::sleep(time::Duration::from_secs(2));

        utils::stop_ls(&mut ls, Some(&mut stdin), Some(3));
        let output = ls.wait_with_output().expect("Cannot capture output");

        assert_eq!(output.status.code(), Some(0));
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", start.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", start.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", end.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", end.1))
        );
    }
}

#[test]
fn supports_lang_find_references_request_for_subroutines() {
    for ((dir, line, character), (start, end)) in [
        (
            env::current_dir()
                .unwrap()
                .join("tests")
                .join("samples")
                .join("a")
                .join("a.cmm"),
            78,
            8,
        ),
        (
            env::current_dir()
                .unwrap()
                .join("tests")
                .join("samples")
                .join("a")
                .join("a.cmm"),
            125,
            0,
        ),
    ]
    .into_iter()
    .zip([((63, 11), (78, 6)), ((125, 0), (122, 10))])
    {
        let mut ls = utils::start_ls_with_workspace(&[
            &format!("--clientProcessId={}", process::id().to_string()),
            &format!("--trace={}", "messages"),
        ]);
        let mut stdin = ls.stdin.take().unwrap();

        let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
        utils::to_stdin(&mut stdin, &notif);

        let uri = Url::from_file_path(dir).expect("Must not fail.");

        let notif = utils::make_find_references_request(2, uri, line, character);
        utils::to_stdin(&mut stdin, &notif);
        thread::sleep(time::Duration::from_secs(2));

        utils::stop_ls(&mut ls, Some(&mut stdin), Some(3));
        let output = ls.wait_with_output().expect("Cannot capture output");

        assert_eq!(output.status.code(), Some(0));
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", start.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", start.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", end.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", end.1))
        );
    }
}

#[test]
fn supports_lang_find_references_request_for_macros() {
    for ((dir, line, character), (start, end, file)) in [(
        env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm"),
        38,
        10,
    )]
    .into_iter()
    .zip([
        (
            (38, 4),
            (38, 16),
            utils::to_file_uri("tests/samples/a/a.cmm"),
        ),
        (
            (42, 6),
            (42, 18),
            utils::to_file_uri("tests/samples/a/a.cmm"),
        ),
        ((17, 6), (17, 18), utils::to_file_uri("tests/samples/c.cmm")),
    ]) {
        let mut ls = utils::start_ls_with_workspace(&[
            &format!("--clientProcessId={}", process::id().to_string()),
            &format!("--trace={}", "messages"),
        ]);
        let mut stdin = ls.stdin.take().unwrap();

        let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
        utils::to_stdin(&mut stdin, &notif);

        let uri = Url::from_file_path(dir).expect("Must not fail.");

        let notif = utils::make_find_references_request(2, uri, line, character);
        utils::to_stdin(&mut stdin, &notif);
        thread::sleep(time::Duration::from_secs(2));

        utils::stop_ls(&mut ls, Some(&mut stdin), Some(3));
        let output = ls.wait_with_output().expect("Cannot capture output");

        assert_eq!(output.status.code(), Some(0));
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", start.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", start.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", end.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", end.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"uri\":\"{}", file))
        );
    }
}

#[test]
fn supports_lang_find_references_request_for_file() {
    for ((dir, line, character), (start, end, file)) in [(
        env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("c.cmm"),
        35,
        11,
    )]
    .into_iter()
    .zip([((35, 3), (35, 12), utils::to_file_uri("tests/samples/c.cmm"))])
    {
        let mut ls = utils::start_ls_with_workspace(&[
            &format!("--clientProcessId={}", process::id().to_string()),
            &format!("--trace={}", "messages"),
        ]);
        let mut stdin = ls.stdin.take().unwrap();

        let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
        utils::to_stdin(&mut stdin, &notif);

        let uri = Url::from_file_path(dir).expect("Must not fail.");

        let notif = utils::make_find_references_request(2, uri, line, character);
        utils::to_stdin(&mut stdin, &notif);
        thread::sleep(time::Duration::from_secs(2));

        utils::stop_ls(&mut ls, Some(&mut stdin), Some(3));
        let output = ls.wait_with_output().expect("Cannot capture output");

        assert_eq!(output.status.code(), Some(0));
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", start.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", start.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", end.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", end.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"uri\":\"{}", file))
        );
    }
}

#[test]
fn supports_lang_find_references_request_for_command() {
    for ((dir, line, character), (start, end, file)) in [(
        env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("c.cmm"),
        37,
        5,
    )]
    .into_iter()
    .zip([
        ((37, 0), (38, 0), utils::to_file_uri("tests/samples/c.cmm")),
        (
            (158, 4),
            (159, 0),
            utils::to_file_uri("tests/samples/a/a.cmm"),
        ),
    ]) {
        let mut ls = utils::start_ls_with_workspace(&[
            &format!("--clientProcessId={}", process::id().to_string()),
            &format!("--trace={}", "messages"),
        ]);
        let mut stdin = ls.stdin.take().unwrap();

        let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
        utils::to_stdin(&mut stdin, &notif);

        let uri = Url::from_file_path(dir).expect("Must not fail.");

        let notif = utils::make_find_references_request(2, uri, line, character);
        utils::to_stdin(&mut stdin, &notif);
        thread::sleep(time::Duration::from_secs(2));

        utils::stop_ls(&mut ls, Some(&mut stdin), Some(3));
        let output = ls.wait_with_output().expect("Cannot capture output");

        assert_eq!(output.status.code(), Some(0));
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", start.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", start.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"line\":{}", end.0))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"character\":{}", end.1))
        );
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains(&format!("\"uri\":\"{}", file))
        );
    }
}

#[test]
fn can_enable_logging() {
    let pid = process::id();

    let mut ls = utils::start_ls(&[&format!("--clientProcessId={}", pid.to_string())], true);
    let mut stdin = ls.stdin.take().unwrap();

    let notif = utils::make_set_trace_notification(utils::TraceValue::Messages);
    utils::to_stdin(&mut stdin, &notif);

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("$/logTrace")
    );
}

#[test]
fn can_build_semantic_token_legend() {
    let mut ls = utils::start_ls_with_semantic_tokens(&[
        &format!("--clientProcessId={}", process::id().to_string()),
        &format!("--trace={}", "messages"),
    ]);
    let mut stdin = ls.stdin.take().unwrap();

    thread::sleep(time::Duration::from_millis(2000));

    utils::stop_ls(&mut ls, Some(&mut stdin), Some(2));
    let output = ls.wait_with_output().expect("Cannot capture output");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("typeParameter")
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("macro")
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("number")
    );
    assert!(
        !std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("property")
    );

    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("declaration")
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("abstract")
    );
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("defaultLibrary")
    );
    assert!(
        !std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("async")
    );
}
