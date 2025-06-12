// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::json;
use tree_sitter::Tree;
use url::Url;

use crate::{
    ReturnCode,
    config::{Config, Workspace},
    ls::language::find_definition,
    ls::lsp::Message,
    ls::transport::StdioChannel,
    ls::{
        ProcHeartbeat, State, Tasks,
        doc::{TextDoc, TextDocStatus, TextDocs, import_doc, read_doc, update_doc},
        log_notif, read_msg,
        request::{
            DidChangeTextDocumentNotification, DidCloseTextDocumentNotification,
            DidOpenTextDocumentNotification, GoToDefinitionRequest, LogTraceNotification,
            Notification, Request, SetTraceNotification,
        },
        response::{ErrorResponse, GoToDefinitionResponse, LocationResult, NullResponse, Response},
        tasks::{OngoingTask, Task, TaskDone, TaskSystem},
        workspace::{FileIndex, WorkspaceMembers, index_files, locate_files},
    },
    protocol::{
        DefinitionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, ErrorCodes, LogTraceParams, NumberOrString, ResponseError,
        SetTraceParams, TextDocumentItem, TraceValue, Uri,
    },
    t32::{LANGUAGE_ID, LangExpressions, SUFFIXES, lang_id_supported},
};

type FileData = (TextDoc, Tree, LangExpressions);

pub fn handle_requests(channel: &mut StdioChannel, mut cfg: Config) -> Result<(), ReturnCode> {
    let mut tasks = Tasks {
        runner: TaskSystem::build(),
        blocked: Vec::new(),
        ongoing: Vec::new(),
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
        recv_completed_tasks(&cfg, &mut g.tasks, &mut g.docs, &mut outgoing);

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

fn schedule_tasks(
    incoming: &mut [Option<Message>],
    g: &mut State,
    cfg: &mut Config,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    try_schedule_blocked(
        &mut g.tasks.runner,
        &mut g.tasks.ongoing,
        &mut g.tasks.blocked,
    )?;

    for msg in incoming {
        let msg = msg.take().expect("No empty slots in list.");

        if cfg.trace_level != TraceValue::Off && msg.is_notification() {
            outgoing.push(Some(Message::Notification(log_notif(
                msg.get_notification(),
            ))));
        }

        match msg {
            // All new requests after a shutdown request was received should
            // be trigger an `InvalidRequest` error.
            m if g.shutdown_request_recv && m.is_request() => {
                outgoing.push(Some(error_shutdown_seq(
                    m.get_request()
                        .get_id()
                        .expect("Every request must have an ID.")
                        .clone(),
                )));
            }
            Message::Notification(Notification::DidCloseTextDocumentNotification(
                DidCloseTextDocumentNotification {
                    params: DidCloseTextDocumentParams { text_document },
                },
            )) => {
                process_doc_close_notif(&text_document.uri, &mut g.docs, outgoing);
            }
            Message::Notification(Notification::DidOpenTextDocumentNotification(
                DidOpenTextDocumentNotification {
                    params: DidOpenTextDocumentParams { text_document },
                },
            )) => {
                if lang_id_supported(&text_document.language_id) {
                    process_doc_open_notif(
                        text_document,
                        g.docs.get_file_idx().clone(),
                        &mut g.tasks,
                    )?;
                } else {
                    outgoing.push(Some(error_lang_id_unsupported(&text_document.language_id)));
                }
            }
            Message::Notification(Notification::DidChangeTextDocumentNotification(
                DidChangeTextDocumentNotification { params },
            )) => {
                process_doc_change_notif(params, &g.docs, &mut g.tasks, outgoing)?;
            }
            Message::Notification(Notification::SetTraceNotification(SetTraceNotification {
                params: SetTraceParams { value },
            })) => {
                cfg.trace_level = value;
            }
            Message::Request(Request::GoToDefinition(GoToDefinitionRequest { id, params })) => {
                process_goto_definition_req(id, params, g, cfg.trace_level, outgoing)?;
            }
            Message::Request(Request::ShutdownRequest(req)) => {
                g.shutdown_request_recv = true;
                outgoing.push(Some(Message::Response(Response::NullResponse(
                    NullResponse { id: req.id.clone() },
                ))));
            }
            Message::Notification(Notification::ExitNotification(_)) => {
                g.exit_requested = true;
            }
            _ => (),
        }
    }
    Ok(())
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

fn try_schedule(
    ts: &mut TaskSystem,
    job: Task,
    ongoing: &mut Vec<OngoingTask>,
    blocked: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    if task_blocked(&job, ongoing) {
        blocked.push(job);
        return Ok(());
    }
    ts.schedule(&job)?;
    add_ongoing_task_if_strictly_ordered(&job, ongoing);

    Ok(())
}

fn try_schedule_blocked(
    ts: &mut TaskSystem,
    ongoing: &mut Vec<OngoingTask>,
    blocked: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    for job in blocked.iter() {
        if task_blocked(job, &ongoing) {
            continue;
        }
        ts.schedule(job)?;
        add_ongoing_task_if_strictly_ordered(job, ongoing);
    }
    blocked.retain(|t| task_blocked(t, &ongoing));

    Ok(())
}

fn recv_completed_tasks(
    cfg: &Config,
    ts: &mut Tasks,
    docs: &mut TextDocs,
    outgoing: &mut Vec<Option<Message>>,
) {
    for done in ts.runner.rx.try_iter() {
        mark_ongoing_task_completed(&done, &mut ts.ongoing);

        match done {
            TaskDone::GoToDefinitionExtMeta(id, loc) => {
                let result = match loc {
                    Some(links) => Some(LocationResult::ExtMeta(links)),
                    None => None,
                };
                outgoing.push(Some(Message::Response(Response::GoToDefinitionResponse(
                    GoToDefinitionResponse { id, result },
                ))));
            }
            TaskDone::TextDocNew(doc, tree, globals) => {
                if cfg.trace_level != TraceValue::Off {
                    outgoing.push(Some(trace_doc_change(&doc, &tree)));
                }
                docs.add(doc, tree, globals, TextDocStatus::Open);
            }
            TaskDone::TextDocEdit(doc, tree, globals) => {
                if cfg.trace_level != TraceValue::Off {
                    outgoing.push(Some(trace_doc_change(&doc, &tree)));
                }
                docs.update(doc, tree, globals);
            }
            TaskDone::WorkspaceFileScan(res) => match res {
                Ok((doc, tree, globals)) => {
                    if cfg.trace_level != TraceValue::Off {
                        outgoing.push(Some(trace_doc_change(&doc, &tree)));
                    }
                    docs.add(doc, tree, globals, TextDocStatus::Closed);
                }
                Err(uri) => {
                    if cfg.trace_level != TraceValue::Off {
                        outgoing.push(Some(trace_doc_cannot_read(&uri)));
                    }
                }
            },
            TaskDone::WorkspaceFileDiscovery(_) | TaskDone::WorkspaceFileIndexNew(_) => {
                unreachable!()
            }
        }
    }
}

fn send_outgoing(channel: &mut StdioChannel, msgs: &mut Vec<Option<Message>>) {
    for msg in msgs {
        let msg = msg.take().expect("No empty slots allowed.");
        channel.send_msg(msg);
    }
}

fn process_doc_open_notif(
    doc: TextDocumentItem,
    files: FileIndex,
    ts: &mut Tasks,
) -> Result<(), ReturnCode> {
    try_schedule(
        &mut ts.runner,
        Task::TextDocNew(doc, files, import_doc),
        &mut ts.ongoing,
        &mut ts.blocked,
    )
}

fn process_doc_change_notif(
    params: DidChangeTextDocumentParams,
    docs: &TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if !docs.is_open(&params.text_document.uri) {
        outgoing.push(Some(error_textdoc_not_open(&params.text_document.uri)));
        return Ok(());
    }

    let (doc, tree, ..) = docs.get_doc_data(&params.text_document.uri).unwrap();

    let doc = TextDoc {
        version: params.text_document.version,
        ..doc.clone()
    };
    try_schedule(
        &mut ts.runner,
        Task::TextDocEdit(
            doc,
            tree.clone(),
            docs.get_file_idx().clone(),
            params.content_changes,
            update_doc,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )
}

fn process_doc_close_notif(uri: &str, docs: &mut TextDocs, outgoing: &mut Vec<Option<Message>>) {
    if !docs.is_open(uri) {
        outgoing.push(Some(error_textdoc_cannot_close(uri)));
        return;
    }
    docs.close(uri);
}

fn process_goto_definition_req(
    id: NumberOrString,
    params: DefinitionParams,
    g: &mut State,
    trace_level: TraceValue,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    let (doc, tree, globals) = match g.docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, globals)) => (doc, tree, globals),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut g.tasks.runner,
        Task::GoToDefinitionExtMeta(
            id,
            doc.clone(),
            tree.clone(),
            globals.clone(),
            params.position,
            find_definition,
        ),
        &mut g.tasks.ongoing,
        &mut g.tasks.blocked,
    )?;
    Ok(())
}

/// Some requests like document updates can only be processed one at a time.
/// This functions checks whether there is an ongoing task that would block
/// the scheduling of the new one.
/// Document lookup operations like *Go to Definition* should use the latest
/// document version, so we delay the corresponding task until the document
/// update has been completed.
fn task_blocked(job: &Task, ongoing: &[OngoingTask]) -> bool {
    match job {
        Task::GoToDefinitionExtMeta(_, TextDoc { uri, .. }, ..)
        | Task::TextDocEdit(TextDoc { uri, .. }, ..)
        | Task::TextDocNew(TextDocumentItem { uri, .. }, ..) => ongoing.iter().any(|o| match o {
            OngoingTask::TextDocUpdate { uri: file } => file == uri,
        }),
        Task::WorkspaceFileScan(url, ..) => ongoing.iter().any(|o| match o {
            OngoingTask::TextDocUpdate { uri: file } => file == url.as_str(),
        }),
        Task::WorkspaceFileDiscovery(..) | Task::WorkspaceFileIndexNew(..) => false,
    }
}

/// Document updates need to be processed in the order in which they were
/// received. Hence, we need to monitor for which documents we are currently
/// processing an update.
fn add_ongoing_task_if_strictly_ordered(job: &Task, ongoing: &mut Vec<OngoingTask>) {
    let t = match job {
        Task::TextDocNew(TextDocumentItem { uri, .. }, ..)
        | Task::TextDocEdit(TextDoc { uri, .. }, ..) => {
            OngoingTask::TextDocUpdate { uri: uri.clone() }
        }
        Task::WorkspaceFileScan(url, ..) => OngoingTask::TextDocUpdate {
            uri: url.to_string(),
        },
        Task::GoToDefinitionExtMeta(..)
        | Task::WorkspaceFileDiscovery(..)
        | Task::WorkspaceFileIndexNew(..) => return,
    };
    ongoing.push(t);
}

fn mark_ongoing_task_completed(job: &TaskDone, ongoing: &mut Vec<OngoingTask>) {
    let idx = match job {
        TaskDone::TextDocEdit(doc, ..)
        | TaskDone::TextDocNew(doc, ..)
        | TaskDone::WorkspaceFileScan(Ok((doc, ..))) => find_ongoing_task(&doc.uri, ongoing),
        TaskDone::WorkspaceFileScan(Err(uri)) => find_ongoing_task(uri, ongoing),
        TaskDone::GoToDefinitionExtMeta(..)
        | TaskDone::WorkspaceFileDiscovery(..)
        | TaskDone::WorkspaceFileIndexNew(..) => return,
    };
    ongoing.remove(idx);
}

fn index_workspace(
    cfg: &Config,
    channel: &mut StdioChannel,
    tasks: &mut Tasks,
    workspace: &Workspace,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(FileIndex, Vec<FileData>), ReturnCode> {
    debug_assert!(tasks.ongoing.len() <= 0 && tasks.blocked.len() <= 0);

    let members = discover_files(tasks, workspace.clone())?;
    if members.missing_roots.len() > 0 {
        outgoing.push(Some(trace_root_invalid(&members.missing_roots)));
    }
    send_outgoing(channel, outgoing);
    outgoing.clear();

    let file_index = categorize_files(tasks, members.files.clone())?;

    let content = parse_files(cfg, channel, tasks, &file_index, &members, outgoing)?;

    Ok((file_index, content))
}

fn discover_files(tasks: &mut Tasks, workspace: Workspace) -> Result<WorkspaceMembers, ReturnCode> {
    let discover = Task::WorkspaceFileDiscovery(workspace.clone(), &SUFFIXES, locate_files);
    try_schedule(
        &mut tasks.runner,
        discover,
        &mut tasks.ongoing,
        &mut tasks.blocked,
    )?;

    let members = match tasks.runner.rx.recv() {
        Ok(TaskDone::WorkspaceFileDiscovery(m)) => Ok(m),
        Ok(_) => unreachable!("No other tasks must be pending."),
        Err(_) => Err(ReturnCode::UnavailableErr),
    };
    tasks.ongoing.clear();

    members
}

fn categorize_files(tasks: &mut Tasks, files: Vec<Url>) -> Result<FileIndex, ReturnCode> {
    let indexer = Task::WorkspaceFileIndexNew(files, index_files);
    try_schedule(
        &mut tasks.runner,
        indexer,
        &mut tasks.ongoing,
        &mut tasks.blocked,
    )?;

    let file_index = match tasks.runner.rx.recv() {
        Ok(TaskDone::WorkspaceFileIndexNew(idx)) => Ok(idx),
        Ok(_) => unreachable!("No other tasks must be pending."),
        Err(_) => Err(ReturnCode::UnavailableErr),
    };
    tasks.ongoing.clear();

    file_index
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

fn find_ongoing_task(doc: &str, ongoing: &[OngoingTask]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            OngoingTask::TextDocUpdate { uri } => uri == doc,
        })
        .expect("Must be a registered task.")
}

fn error_shutdown_seq(id: NumberOrString) -> Message {
    Message::Response(Response::ErrorResponse(ErrorResponse {
        id: Some(id),
        error: ResponseError {
            code: ErrorCodes::InvalidRequest as i64,
            message: "Error: Server has received shutdown request. Cannot handle request."
                .to_string(),
            data: None,
        },
    }))
}

fn error_lang_id_unsupported(lang_id: &str) -> Message {
    Message::Response(Response::ErrorResponse(ErrorResponse {
        id: None,
        error: ResponseError {
            code: ErrorCodes::InvalidParams as i64,
            message: format!(
                "Error: Language ID \"{}\" is not supported for text documents. The only supported language ID is \"{}\".",
                lang_id, LANGUAGE_ID
            ),
            data: None,
        },
    }))
}

fn error_textdoc_not_open(uri: &str) -> Message {
    Message::Response(Response::ErrorResponse(ErrorResponse {
        id: None,
        error: ResponseError {
            code: ErrorCodes::InvalidRequest as i64,
            message: format!(
                "Error: Text document \"{}\" has not been opened, so it cannot be changed.",
                uri
            ),
            data: None,
        },
    }))
}

fn error_textdoc_cannot_close(uri: &str) -> Message {
    Message::Response(Response::ErrorResponse(ErrorResponse {
        id: None,
        error: ResponseError {
            code: ErrorCodes::InvalidRequest as i64,
            message: format!(
                "Error: Text document \"{}\" has not been opened, so it cannot be closed.",
                uri
            ),
            data: None,
        },
    }))
}

fn trace_doc_cannot_read(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: File \"{}\" could not be read.", uri),
            verbose: None,
        },
    }))
}

fn trace_doc_change(doc: &TextDoc, tree: &Tree) -> Message {
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

fn trace_doc_unknown(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: File \"{}\" is not known.", uri),
            verbose: None,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::t32::LANGUAGE_ID;

    #[test]
    fn can_block_tasks_until_ready() {
        let mut ts = TaskSystem::build();

        let job = Task::TextDocNew(
            TextDocumentItem {
                uri: "file:///c:/project/readme.md".to_string(),
                language_id: LANGUAGE_ID.to_string(),
                version: 1,
                text: "This is a test.".to_string(),
            },
            FileIndex::new(),
            import_doc,
        );
        let job_copy = job.clone();

        let mut ongoing = Vec::<OngoingTask>::new();
        let mut blocked = Vec::<Task>::new();

        try_schedule(&mut ts, job, &mut ongoing, &mut blocked).expect("Must not fail.");

        assert!(matches!(ongoing[0], OngoingTask::TextDocUpdate { .. }));

        try_schedule(&mut ts, job_copy, &mut ongoing, &mut blocked).expect("Must not fail.");

        assert_eq!(ongoing.len(), 1);
        assert!(matches!(blocked[0], Task::TextDocNew { .. }));
    }
}
