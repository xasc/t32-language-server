// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    env,
    io::Write,
    path,
    process::{self, Child, ChildStdin, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::json;
use url::Url;

#[allow(dead_code)]
#[derive(PartialEq)]
pub enum TraceValue {
    Messages,
    Verbose,
    Off,
}

#[allow(dead_code)]
pub fn start_ls(args: &[&str], try_initialize: bool) -> Child {
    let mut params = vec!["run", "--quiet", "--"];
    params.extend_from_slice(&args);

    let mut ls = Command::new("cargo")
        .args(params)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must be able to start language server.");

    if try_initialize {
        if let Some(cin) = &mut ls.stdin {
            let pid = process::id();

            let init = make_initialize_request(1, pid);

            to_stdin(cin, &init);
        }
    }
    ls
}

#[allow(dead_code)]
pub fn start_ls_with_workspace(args: &[&str]) -> Child {
    let mut params = vec!["run", "--quiet", "--"];
    params.extend_from_slice(&args);

    let mut ls = Command::new("cargo")
        .args(params)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must be able to start language server.");

    if let Some(cin) = &mut ls.stdin {
        let pid = process::id();

        let init = make_initialize_request_with_multi_root_workspace(1, pid);

        to_stdin(cin, &init);
    }
    ls
}

#[allow(dead_code)]
pub fn stop_ls(proc: &mut Child, stdin: Option<&mut ChildStdin>, try_shutdown: Option<isize>) {
    if let Some(cin) = stdin {
        if let Some(id) = try_shutdown {
            let shutdown = make_shutdown_request(id);

            cin.write_all(shutdown.as_bytes()).unwrap();
            let _ = cin.flush();
        }
        let exit = make_exit_notification();

        to_stdin(cin, &exit);
    }

    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Ok(Some(_)) = proc.try_wait() {
            break;
        }
    }
}

#[allow(dead_code)]
pub fn make_initialize_request(id: isize, pid: u32) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": pid,
            "capabilities": {},
        }
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_initialize_request_with_multi_root_workspace(id: isize, pid: u32) -> String {
    let dir = env::current_dir().unwrap().join("tests").join("samples");
    let workspace: Url = Url::from_directory_path(path::absolute(dir).unwrap()).unwrap();

    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": pid,
            "workspaceFolders": [{
                "uri": workspace.as_str(),
                "name": "workspace",
            }],
            "capabilities": {},
        }
    });

    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_initialize_request_with_invalid_multi_root_workspace(id: isize, pid: u32) -> String {
    let dir = env::current_dir().unwrap().join("tests").join("samples");
    let workspace: Url = Url::from_directory_path(path::absolute(dir.clone()).unwrap()).unwrap();

    let dir_invalid = env::current_dir()
        .unwrap()
        .join("tests")
        .join("__invalid__");
    let workspace_invalid: Url =
        Url::from_directory_path(path::absolute(dir_invalid).unwrap()).unwrap();

    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": pid,
            "workspaceFolders": [
                {
                    "uri": workspace.as_str(),
                    "name": "workspace",
                },
                {
                    "uri": workspace_invalid.as_str(),
                    "name": "invalid",
                },
            ],
            "capabilities": {},
        }
    });

    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_initialize_request_with_root_uri(id: isize, pid: u32) -> String {
    let dir = env::current_dir().unwrap().join("tests").join("samples");
    let workspace: Url = Url::from_directory_path(path::absolute(dir.clone()).unwrap()).unwrap();

    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": pid,
            "rootUri": workspace.as_str(),
            "capabilities": {},
        }
    });

    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_initialize_request_with_root_path(id: isize, pid: u32) -> String {
    let dir = env::current_dir().unwrap().join("tests").join("samples");

    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": pid,
            "rootPath": dir,
            "capabilities": {},
        }
    });

    build_msg(&content.to_string())
}

fn make_exit_notification() -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "exit",
    });
    build_msg(&content.to_string())
}

fn make_shutdown_request(id: isize) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "shutdown",
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_goto_definition_request(id: isize, uri: Url, line: u32, character: u32) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/definition",
        "params": {
            "textDocument": {
                "uri": uri.to_string(),
            },
            "position": {
                "line": line,
                "character": character,
            },
        }
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_did_open_text_doc_notification() -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///c:/project/test.cmm",
                "languageId": "practice",
                "version": 1,
                "text": "PRINT \"Hello, World!\"",
            }
        }
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_did_rename_files_notification() -> String {
    let dir = env::current_dir().unwrap().join("tests").join("samples");

    let content = json!({
        "jsonrpc": "2.0",
        "method": "workspace/didRenameFiles",
        "params": {
            "files": [{
                "oldUri": Url::from_file_path(dir.join("c.cmm")).unwrap().to_string(),
                "newUri": Url::from_file_path(dir.join("c1.cmm")).unwrap().to_string(),
            }]
        }
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_did_change_text_doc_notification() -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "file:///c:/project/test.cmm",
                "version": 2,
            },
            "contentChanges": [{
                "range": {
                    "start": {
                        "line": 0,
                        "character": 12,
                    },
                    "end": {
                        "line": 0,
                        "character": 19,
                    }
                },
                "text": "",
            }],
        }
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_did_close_text_doc_notification() -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {
            "textDocument": {
                "uri": "file:///c:/project/test.cmm",
            }
        }
    });
    build_msg(&content.to_string())
}

#[allow(dead_code)]
pub fn make_set_trace_notification(level: TraceValue) -> String {
    let content = json!({
        "jsonrpc": "2.0",
        "method": "$/setTrace",
        "params": {
            "value": match level {
                TraceValue::Messages => "messages",
                TraceValue::Verbose => "verbose",
                TraceValue::Off => "off",
            }
        }
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

pub fn to_stdin(cin: &mut ChildStdin, msg: &str) {
    let _ = cin.write_all(msg.as_bytes());
    let _ = cin.flush();
}
