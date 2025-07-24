// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::time::{Duration, Instant};

use serde_json::json;
use tree_sitter::Tree;

use crate::{
    ReturnCode,
    config::{Config, Workspace},
    ls::lsp::Message,
    ls::transport::StdioChannel,
    ls::{
        ProcHeartbeat, State, Tasks,
        doc::{TextDoc, TextDocs, read_doc},
        read_msg,
        request::{LogTraceNotification, Notification},
        tasks::{
            Task, TaskDone, TaskSystem, categorize_files, discover_files, recv_completed_tasks,
            schedule_tasks, try_schedule,
        },
        workspace::{FileIndex, WorkspaceMembers},
    },
    protocol::{LogTraceParams, TraceValue, Uri},
    t32::LangExpressions,
};

type FileData = (TextDoc, Tree, LangExpressions);

pub fn handle_requests(channel: &mut StdioChannel, mut cfg: Config) -> Result<(), ReturnCode> {
    let mut tasks = Tasks {
        runner: TaskSystem::build(),
        blocked: Vec::new(),
        ongoing: Vec::new(),
        completed: Vec::new(),
    };

    let mut outgoing: Vec<Option<Message>> = Vec::new();

    let (files, file_data) = if match cfg.workspace {
        Workspace::Root(Some(_)) | Workspace::Folders(Some(_)) => true,
        _ => false,
    } {
        index_workspace(&cfg, channel, &mut tasks, &cfg.workspace, &mut outgoing)?
    } else {
        (FileIndex::new(), Vec::new())
    };
    debug_assert_eq!(tasks.ongoing.len(), 0);

    let mut g = State {
        shutdown_request_recv: false,
        exit_requested: false,
        heartbeat: ProcHeartbeat::build(&cfg),
        docs: TextDocs::from_workspace(files, file_data),
        tasks,
    };

    let mut incoming: Vec<Option<Message>> = Vec::new();

    loop {
        recv_incoming(channel, &mut g.heartbeat, &mut incoming)?;
        recv_completed_tasks(&cfg, &mut g.tasks, &mut g.docs, &mut outgoing)?;

        schedule_tasks(&mut incoming, &mut g, &mut cfg, &mut outgoing)?;

        send_outgoing(channel, &mut outgoing);

        if g.exit_requested {
            return Err(if g.shutdown_request_recv {
                ReturnCode::OkExit
            } else {
                ReturnCode::ErrExit
            });
        }
        incoming.clear();
        outgoing.clear();
    }
}

fn recv_incoming(
    channel: &mut StdioChannel,
    heartbeat: &mut ProcHeartbeat,
    incoming: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    loop {
        match read_msg(channel, heartbeat) {
            Ok(Some(r)) => incoming.push(Some(r)),
            Ok(None) => break,
            Err(rc) => return Err(rc),
        };
    }
    Ok(())
}

fn send_outgoing(channel: &mut StdioChannel, msgs: &mut Vec<Option<Message>>) {
    for msg in msgs {
        let msg = msg.take().expect("No empty slots allowed.");
        channel.send_msg(msg);
    }
}

fn parse_files(
    cfg: &Config,
    channel: &mut StdioChannel,
    tasks: &mut Tasks,
    file_index: &FileIndex,
    workspace: &WorkspaceMembers,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<Vec<FileData>, ReturnCode> {
    let num_files: u32 = match workspace.files.len().try_into() {
        Ok(n) => n,
        Err(_) => u32::MAX,
    };

    for file in workspace.files.iter() {
        try_schedule(
            &mut tasks.runner,
            Task::WorkspaceFileScan(file.clone(), file_index.clone(), read_doc),
            &mut tasks.ongoing,
            &mut tasks.blocked,
        )?;
    }
    let mut results: Vec<FileData> = Vec::with_capacity(num_files as usize);

    let mut completed: u32 = 0;
    while completed < num_files {
        match tasks.runner.rx.recv() {
            Ok(TaskDone::WorkspaceFileScan(res)) => match res {
                Ok((doc, tree, expr)) => {
                    if cfg.trace_level != TraceValue::Off {
                        outgoing.push(Some(trace_doc_change(&doc, &tree)));
                    }
                    results.push((doc, tree, expr));
                }
                Err(uri) => {
                    if cfg.trace_level != TraceValue::Off {
                        outgoing.push(Some(trace_doc_cannot_read(&uri)));
                    }
                }
            },
            Ok(_) => unreachable!("No other task type must be pending."),
            Err(_) => return Err(ReturnCode::UnavailableErr),
        }
        send_outgoing(channel, outgoing);
        outgoing.clear();

        completed += 1;
    }
    tasks.ongoing.clear();

    Ok(results)
}

pub fn index_workspace(
    cfg: &Config,
    channel: &mut StdioChannel,
    tasks: &mut Tasks,
    workspace: &Workspace,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(FileIndex, Vec<FileData>), ReturnCode> {
    debug_assert!(tasks.ongoing.len() <= 0 && tasks.blocked.len() <= 0);

    let start = Instant::now();

    let members = discover_files(tasks, workspace.clone())?;
    if members.missing_roots.len() > 0 {
        outgoing.push(Some(trace_root_invalid(&members.missing_roots)));
    }
    send_outgoing(channel, outgoing);
    outgoing.clear();

    let file_index = categorize_files(tasks, members.files.clone())?;

    let content = parse_files(cfg, channel, tasks, &file_index, &members, outgoing)?;

    if cfg.trace_level != TraceValue::Off {
        outgoing.push(Some(trace_workspace_indexed(
            Instant::now() - start,
            workspace,
        )));
    }
    Ok((file_index, content))
}

pub fn trace_doc_cannot_read(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: File \"{}\" could not be read.", uri),
            verbose: None,
        },
    }))
}

fn trace_workspace_indexed(duration: Duration, workspace: &Workspace) -> Message {
    Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Workspace files indexed in {:.4} seconds.",
                duration.as_secs_f32()
            ),
            verbose: Some(json!(workspace).to_string()),
        },
    }))
}

fn trace_root_invalid(roots: &[Uri]) -> Message {
    Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "WARNING: Workspace root(s) \"{}\"do not exist.",
                roots.join("\", \"")
            ),
            verbose: None,
        },
    }))
}

pub fn trace_doc_change(doc: &TextDoc, tree: &Tree) -> Message {
    Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Text document \"{}\" was updated to version {}.",
                doc.uri, doc.version
            ),
            verbose: Some(
                json!({
                    "text": doc.text,
                    "tree": tree.root_node().to_sexp(),
                })
                .to_string(),
            ),
        },
    }))
}
