// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod docsync;
mod lang;
mod runners;
mod workspace;

pub use runners::{Task, TaskDone, TaskSystem};
pub use workspace::{categorize_files, discover_files};

use std::time::{Duration, Instant};

use serde_json::json;

use crate::{
    ReturnCode,
    config::Config,
    ls::language::{
        find_external_macro_definition, find_global_macro_definitions,
    },
    ls::lsp::Message,
    ls::{
        State, Tasks,
        doc::{TextDoc, TextDocData, TextDocs},
        language::{ExtMacroDefOrigin, GotoDefinitionResult},
        mainloop::{trace_doc_cannot_read, trace_doc_change},
        tasks::{
            docsync::{process_doc_change_notif, process_doc_close_notif, process_doc_open_notif},
            lang::{process_find_references_req, process_goto_definition_req, process_goto_definition_result, process_goto_external_macro_def_sync},
            workspace::{process_files_did_rename_notif, process_rename_files_result},
        },
        workspace::{FileIndex, ResolvedRenameFileOperations},
    },
    ls::{
        doc::TextDocStatus,
        log_notif,
        request::{Notification, Request},
        response::{ErrorResponse, GoToDefinitionResponse, LocationResult, NullResponse, Response},
    },
    protocol::{
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, ErrorCodes,
        FileRename, LogTraceParams, ResponseError, SetTraceParams,
    },
    protocol::{LocationLink, NumberOrString, TextDocumentItem, TraceValue, Uri},
    t32::{LANGUAGE_ID, lang_id_supported},
};

#[derive(Debug)]
pub enum OngoingTask {
    DidRenameFiles(Instant),
    GoToDefinitionExtMeta(NumberOrString, Instant),
    GoToExternalMacroDefinition {
        id: NumberOrString,
        completed: u32,
        total: u32,
        depth: u32,
        onset: Instant,
        origin: ExtMacroDefOrigin,
        preliminary: Vec<LocationLink>,
        ops: Option<ExtMacroDefOperations>,
    },
    TextDocUpdate {
        uri: String,
        onset: Instant,
    },
}

pub enum OngoingTaskHandle {
    Identifier(NumberOrString),
    Uri(Uri),
}

#[derive(Debug)]
pub struct ExtMacroDefOperations {
    pub scripts: Vec<Uri>,
    pub callees: Vec<Uri>,
}

#[derive(Clone, Debug)]
pub struct ExtMacroDefLookup {
    pub origin: ExtMacroDefOrigin,
    pub find:
        fn(TextDocData, Vec<Uri>, ExtMacroDefOrigin) -> (Option<GotoDefinitionResult>, Vec<Uri>),
}

#[derive(Clone, Debug)]
pub struct RenameFileOperations {
    pub old: Vec<Uri>,
    pub new: Vec<Uri>,
}

impl OngoingTask {
    fn get_onset(&self) -> &Instant {
        match self {
            OngoingTask::DidRenameFiles(onset) => onset,
            OngoingTask::GoToExternalMacroDefinition { onset, .. }
            | OngoingTask::GoToDefinitionExtMeta(.., onset)
            | OngoingTask::TextDocUpdate { onset, .. } => onset,
        }
    }
}

impl From<Vec<FileRename>> for RenameFileOperations {
    fn from(renamed: Vec<FileRename>) -> Self {
        let mut old: Vec<Uri> = Vec::with_capacity(renamed.len());
        let mut new: Vec<Uri> = Vec::with_capacity(renamed.len());

        for op in renamed {
            old.push(op.old_uri);
            new.push(op.new_uri);
        }
        RenameFileOperations { old, new }
    }
}

pub fn recv_completed_tasks(
    cfg: &Config,
    ts: &mut Tasks,
    docs: &mut TextDocs,
    files: &mut FileIndex,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    let mut completed: Vec<TaskDone> = Vec::new();
    for done in ts.runner.rx.try_iter() {
        completed.push(done);
    }

    for done in ts.completed.iter_mut() {
        completed.push(done.take().expect("No empty slots allowed."));
    }
    ts.completed.clear();

    for done in completed {
        let handle = done.get_task_handle();
        process_completed_task(done, cfg, ts, docs, files, outgoing)?;

        if let Some(handle) = handle {
            mark_ongoing_task_completed(handle, &mut ts.ongoing);
        }
    }
    Ok(())
}

pub fn schedule_tasks(
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
        process_msg(msg, g, cfg, outgoing)?;
    }

    progress_multi_part_tasks(&g.docs, &mut g.tasks)?;

    Ok(())
}

fn process_completed_task(
    done: TaskDone,
    cfg: &Config,
    ts: &mut Tasks,
    docs: &mut TextDocs,
    files: &mut FileIndex,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    match done {
        TaskDone::DidRenameFiles(
            ResolvedRenameFileOperations {
                renamed,
                missing_dirs,
                missing_files,
            },
            new_files,
        ) => {
            process_rename_files_result(&renamed, new_files, docs, files);

            if cfg.trace_level != TraceValue::Off {
                let onset = get_rename_task_onset(&ts.ongoing);
                outgoing.push(Some(trace_doc_rename(&renamed, Instant::now() - *onset)));

                for dir in missing_dirs {
                    outgoing.push(Some(trace_dir_unknown(&dir)));
                }

                for file in missing_files {
                    outgoing.push(Some(trace_doc_unknown(&file)));
                }
            }
        }
        TaskDone::GoToDefinitionExtMeta(id, goto_def) => {
            if let Some(resp) = process_goto_definition_result(docs, &id, goto_def, ts) {
                if cfg.trace_level != TraceValue::Off {
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                    let OngoingTask::GoToDefinitionExtMeta(_, onset) = &ts.ongoing[idx] else {
                        unreachable!("No other type possible.");
                    };
                    outgoing.push(Some(trace_goto_def(
                        Instant::now() - *onset,
                        resp.result.clone(),
                    )));
                }
                outgoing.push(Some(Message::Response(Response::GoToDefinitionResponse(
                    resp,
                ))));
            }
        }
        TaskDone::GoToExternalMacroDefinitionExtMeta(id, links) => {
            let result = if links.is_empty() {
                None
            } else {
                Some(LocationResult::ExtMeta(links))
            };

            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                let OngoingTask::GoToExternalMacroDefinition { onset, .. } = &ts.ongoing[idx]
                else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_goto_def(
                    Instant::now() - *onset,
                    result.clone(),
                )));
            }

            outgoing.push(Some(Message::Response(Response::GoToDefinitionResponse(
                GoToDefinitionResponse { id, result },
            ))));
        }
        TaskDone::GoToExternalMacroDefinitionSync(id, defs, script, callers) => {
            process_goto_external_macro_def_sync(id, script, defs, callers, &mut ts.ongoing);
        }
        TaskDone::TextDocNew(doc, tree, globals) => {
            if cfg.trace_level != TraceValue::Off {
                let onset = get_task_onset_by_doc(&doc.uri, &ts.ongoing);
                outgoing.push(Some(trace_doc_change(&doc, &tree, Instant::now() - *onset)));
            }
            docs.add(doc, tree, globals, TextDocStatus::Open);
        }
        TaskDone::TextDocEdit(doc, tree, globals) => {
            if cfg.trace_level != TraceValue::Off {
                let onset = get_task_onset_by_doc(&doc.uri, &ts.ongoing);
                outgoing.push(Some(trace_doc_change(&doc, &tree, Instant::now() - *onset)));
            }
            docs.update(doc, tree, globals);
        }
        TaskDone::WorkspaceFileScan(res) => match res {
            Ok((doc, tree, globals)) => {
                if cfg.trace_level != TraceValue::Off {
                    let onset = get_task_onset_by_doc(&doc.uri, &ts.ongoing);
                    outgoing.push(Some(trace_doc_change(&doc, &tree, Instant::now() - *onset)));
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
    Ok(())
}

fn mark_ongoing_task_completed(handle: OngoingTaskHandle, ongoing: &mut Vec<OngoingTask>) {
    let idx = match handle {
        OngoingTaskHandle::Identifier(id) => find_ongoing_task_by_id(&id, ongoing),
        OngoingTaskHandle::Uri(uri) => find_ongoing_task_by_doc(&uri, ongoing),
    };
    ongoing.remove(idx);
}

pub fn try_schedule(
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
    add_task_status_tracking(&job, ongoing);

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
        add_task_status_tracking(job, ongoing);
    }
    blocked.retain(|t| task_blocked(t, &ongoing));

    Ok(())
}

/// Some requests like document updates can only be processed one at a time.
/// This functions checks whether there is an ongoing task that would block
/// the scheduling of the new one.
/// Document lookup operations like *Go to Definition* should use the latest
/// document version, so we delay the corresponding task until the document
/// update has been completed.
/// Rename operations delay all file updates, because they may target a file
/// which is in the process of being renamed.
fn task_blocked(job: &Task, ongoing: &[OngoingTask]) -> bool {
    match job {
        Task::GoToDefinitionExtMeta(
            _,
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::TextDocEdit(TextDoc { uri, .. }, ..)
        | Task::TextDocNew(TextDocumentItem { uri, .. }, ..) => ongoing.iter().any(|o| match o {
            OngoingTask::DidRenameFiles(..) => true,
            OngoingTask::TextDocUpdate { uri: file, .. } => file == uri,
            _ => false,
        }),
        Task::DidRenameFiles(RenameFileOperations { old, .. }, ..) => {
            ongoing.iter().any(|o| match o {
                OngoingTask::DidRenameFiles(..) => true,
                OngoingTask::TextDocUpdate { uri: file, .. } => old.contains(file),
                _ => false,
            })
        }
        Task::WorkspaceFileScan(url, ..) => ongoing.iter().any(|o| match o {
            OngoingTask::DidRenameFiles(..) => true,
            OngoingTask::TextDocUpdate { uri: file, .. } => file == url.as_str(),
            _ => false,
        }),
        _ => false,
    }
}

/// Document updates need to be processed in the order in which they were
/// received. Hence, we need to monitor for which documents we are currently
/// processing an update.
fn add_task_status_tracking(job: &Task, ongoing: &mut Vec<OngoingTask>) {
    let t = match job {
        Task::DidRenameFiles { .. } => OngoingTask::DidRenameFiles(Instant::now()),
        Task::GoToDefinitionExtMeta(id, ..) => {
            OngoingTask::GoToDefinitionExtMeta(id.clone(), Instant::now())
        }
        Task::TextDocNew(TextDocumentItem { uri, .. }, ..)
        | Task::TextDocEdit(TextDoc { uri, .. }, ..) => OngoingTask::TextDocUpdate {
            uri: uri.clone(),
            onset: Instant::now(),
        },
        Task::WorkspaceFileScan(url, ..) => OngoingTask::TextDocUpdate {
            uri: url.to_string(),
            onset: Instant::now(),
        },
        _ => return,
    };
    ongoing.push(t);
}

fn process_msg(
    msg: Message,
    g: &mut State,
    cfg: &mut Config,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
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
        Message::Notification(Notification::DidCloseTextDocumentNotification {
            params: DidCloseTextDocumentParams { text_document },
        }) => {
            process_doc_close_notif(&text_document.uri, &mut g.docs, outgoing);
        }
        Message::Notification(Notification::DidOpenTextDocumentNotification {
            params: DidOpenTextDocumentParams { text_document },
        }) => {
            if lang_id_supported(&text_document.language_id) {
                process_doc_open_notif(text_document, g.files.clone(), &mut g.tasks)?;
            } else {
                outgoing.push(Some(error_lang_id_unsupported(&text_document.language_id)));
            }
        }
        Message::Notification(Notification::DidChangeTextDocumentNotification { params }) => {
            process_doc_change_notif(params, &g.docs, g.files.clone(), &mut g.tasks, outgoing)?;
        }
        Message::Notification(Notification::DidRenameFilesNotification { params }) => {
            process_files_did_rename_notif(&mut g.tasks, params.files, g.files.clone())?;
        }
        Message::Notification(Notification::SetTraceNotification {
            params: SetTraceParams { value },
        }) => {
            cfg.trace_level = value;
        }
        Message::Request(Request::FindReferences { id: _id, params: _params }) => {
            process_find_references_req()?;
        }
        Message::Request(Request::GoToDefinition { id, params }) => {
            process_goto_definition_req(id, params, cfg.trace_level, &mut g.docs, &mut g.tasks, outgoing)?;
        }
        Message::Request(Request::ShutdownRequest { id }) => {
            g.shutdown_request_recv = true;
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id: id },
            ))));
        }
        Message::Notification(Notification::ExitNotification { .. }) => {
            g.exit_requested = true;
        }
        _ => (),
    }
    Ok(())
}

fn progress_multi_part_tasks(docs: &TextDocs, ts: &mut Tasks) -> Result<(), ReturnCode> {
    let mut tasks: Vec<Task> = Vec::new();

    for job in ts.ongoing.iter_mut() {
        match job {
            OngoingTask::GoToExternalMacroDefinition {
                id,
                completed,
                total,
                origin,
                ops,
                preliminary,
                ..
            } => {
                if *total <= 0 {
                    let globals = docs.get_all_global_macros();
                    if globals.is_some() {
                        preliminary.append(&mut find_global_macro_definitions(
                            docs,
                            globals.unwrap(),
                            origin.clone(),
                        ));
                    }

                    ts.completed
                        .push(Some(TaskDone::GoToExternalMacroDefinitionExtMeta(
                            id.clone(),
                            preliminary.clone(),
                        )));
                } else if *completed <= 0 {
                    if let Some(operations) = ops {
                        debug_assert!(operations.scripts.len() == operations.callees.len());

                        for (script, callee) in
                            operations.scripts.iter().zip(operations.callees.iter())
                        {
                            let (doc, tree, t32) = docs.get_doc_data(script).unwrap();
                            let callers = match docs.get_callers(script) {
                                Some(files) => files.clone(),
                                None => Vec::new(),
                            };

                            tasks.push(Task::GoToExternalMacroDefinition {
                                id: id.clone(),
                                textdoc: TextDocData {
                                    doc: doc.clone(),
                                    tree: tree.clone(),
                                    t32: t32.clone(),
                                },
                                callers: callers,
                                lookup: ExtMacroDefLookup {
                                    origin: ExtMacroDefOrigin {
                                        uri: callee.clone(),
                                        ..origin.clone()
                                    },
                                    find: find_external_macro_definition,
                                },
                            });
                        }
                    }
                    *ops = None;
                }
            }
            _ => (),
        }
    }

    for job in tasks {
        try_schedule(&mut ts.runner, job, &mut ts.ongoing, &mut ts.blocked)?;
    }
    Ok(())
}

fn get_task_onset_by_doc<'a>(doc: &str, ongoing: &'a [OngoingTask]) -> &'a Instant {
    let idx = find_ongoing_task_by_doc(doc, ongoing);
    ongoing[idx].get_onset()
}

fn get_rename_task_onset<'a>(ongoing: &'a [OngoingTask]) -> &'a Instant {
    let idx = find_ongoing_rename_task(ongoing);
    ongoing[idx].get_onset()
}

fn find_ongoing_task_by_id(identifier: &NumberOrString, ongoing: &[OngoingTask]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            OngoingTask::GoToDefinitionExtMeta(id, ..)
            | OngoingTask::GoToExternalMacroDefinition { id, .. } => id == identifier,
            _ => unreachable!("No other tasks can by selected by id."),
        })
        .expect("Must be a registered task.")
}

fn find_ongoing_task_by_doc(doc: &str, ongoing: &[OngoingTask]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            OngoingTask::TextDocUpdate { uri, .. } => uri == doc,
            _ => unreachable!("No other tasks can by selected by document."),
        })
        .expect("Must be a registered task.")
}

fn find_ongoing_rename_task<'a>(ongoing: &'a [OngoingTask]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            OngoingTask::DidRenameFiles(..) => true,
            _ => false,
        })
        .expect("Must be a registered task.")
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

fn trace_dir_unknown(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: Directory \"{}\" is not known.", uri),
            verbose: None,
        },
    })
}

fn trace_doc_rename(renamed: &RenameFileOperations, duration: Duration) -> Message {
    let mut changes: Vec<FileRename> = Vec::with_capacity(renamed.old.len());
    for (old, new) in renamed.old.iter().zip(renamed.new.iter()) {
        changes.push(FileRename {
            old_uri: old.clone(),
            new_uri: new.clone(),
        });
    }

    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Files were renamed in {:.4} seconds.",
                duration.as_secs_f32()
            ),
            verbose: Some(json!(changes).to_string()),
        },
    })
}

fn trace_doc_unknown(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: File \"{}\" is not known.", uri),
            verbose: None,
        },
    })
}

fn trace_goto_def(duration: Duration, defs: Option<LocationResult>) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Go to definition request completed in {:.4} seconds.",
                duration.as_secs_f32()
            ),
            verbose: if let Some(loc) = defs {
                Some(json!(loc).to_string())
            } else {
                None
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        ls::{FileIndex, doc::import_doc},
        t32::LANGUAGE_ID,
    };

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
