// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

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
    ls::{
        State, Tasks,
        doc::{TextDoc, TextDocData, TextDocStatus, TextDocs},
        language::ExtMacroDefOrigin,
        language::MacroPropagation,
        log_notif,
        lsp::Message,
        mainloop::{trace_doc_cannot_read, trace_doc_change},
        request::{Notification, Request},
        response::{
            ErrorResponse, FindReferencesResponse, GoToDefinitionResponse, LocationResult,
            NullResponse, Response,
        },
        tasks::{
            docsync::{process_doc_change_notif, process_doc_close_notif, process_doc_open_notif},
            lang::{
                process_find_references_req, process_find_references_result,
                process_goto_definition_req, process_goto_definition_result,
                progress_find_macro_def_references, progress_find_subscript_macro_refs,
                progress_goto_external_macro_def, recv_find_macro_def_references_sync,
                recv_find_subscript_macro_references_sync, recv_goto_external_macro_def_sync,
            },
            workspace::{process_files_did_rename_notif, process_rename_files_result},
        },
        workspace::{FileIndex, ResolvedRenameFileOperations},
    },
    protocol::{
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, ErrorCodes, FileRename, Location,
        LocationLink, LogTraceParams, NumberOrString, Range, ResponseError, SetTraceParams,
        TextDocumentItem, TraceValue, Uri,
    },
    t32::{LANGUAGE_ID, lang_id_supported},
};

#[derive(Debug)]
pub enum OngoingTask {
    DidRenameFiles(Instant),
    FindMacroReferencesDefinitions {
        id: NumberOrString,
        onset: Instant,
        progress: TaskProgress,
        r#macro: String,
        definitions: Vec<MacroPropagation>,
        results: FileLocationMap,
        undone: Vec<Uri>,
    },
    FindMacroReferencesSubscripts {
        id: NumberOrString,
        onset: Instant,
        progress: TaskProgress,
        r#macro: String,
        visited: Vec<Uri>,
        undone: Vec<Uri>,
        results: FileLocationMap,
    },
    FindReferences(NumberOrString, Instant),
    GoToDefinition(NumberOrString, Instant),
    GoToExternalMacroDef {
        id: NumberOrString,
        onset: Instant,
        progress: TaskProgress,
        origin: ExtMacroDefOrigin,
        visited: FileCallMap,
        undone: ExtMacroDefLookups,
        results: Vec<LocationLink>,
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
pub struct TaskProgress {
    completed: u32,
    pub total: u32,
    cycles: u32,
    max_cycles: u32,
}

/// To find macro defintions in other files, we need to check for the presence
/// of script calls. `LOCAL` and `GLOBAL` macro definitions remain valid in
/// subscripts.
#[derive(Debug)]
pub struct ExtMacroDefLookups {
    pub files: Vec<Uri>,
    pub callees: Vec<Uri>,
}

#[derive(Debug)]
pub struct FileCallMap {
    files: Vec<(Uri, u32)>,
    calls: Vec<Vec<Uri>>,
}
#[derive(Clone, Debug)]
pub struct FileLocationMap {
    files: Vec<(Uri, u32)>,
    locations: Vec<Vec<Range>>,
}

#[derive(Clone, Debug)]
pub struct RenameFileOperations {
    pub old: Vec<Uri>,
    pub new: Vec<Uri>,
}

impl FileLocationMap {
    pub fn new() -> Self {
        FileLocationMap {
            files: Vec::new(),
            locations: Vec::new(),
        }
    }

    pub fn insert(&mut self, file: &Uri, loc: Range) {
        debug_assert_eq!(self.files.len(), self.locations.len());
        match self.files.binary_search_by(|(f, _)| f.cmp(file)) {
            Ok(ii) => match self.locations[ii].binary_search_by(|r| r.cmp(&loc)) {
                Ok(_) => return,
                Err(idx) => self.locations[ii].insert(idx, loc),
            },
            Err(ii) => {
                self.files
                    .insert(ii, (file.clone(), self.locations.len() as u32));
                self.locations.push(vec![loc]);
            }
        }
        debug_assert_eq!(self.files.len(), self.locations.len());
    }

    pub fn to_locations(mut self) -> Vec<Location> {
        debug_assert_eq!(self.files.len(), self.locations.len());

        let mut locs: Vec<Location> = Vec::with_capacity(self.files.len());
        for (file, ii) in self.files {
            let mut slot: Vec<Range> = Vec::new();
            slot.append(&mut self.locations[ii as usize]);

            for span in slot {
                locs.push(Location {
                    uri: file.clone(),
                    range: span,
                });
            }
        }
        locs
    }
}

impl OngoingTask {
    fn get_id(&self) -> &NumberOrString {
        match self {
            OngoingTask::FindReferences(id, ..)
            | OngoingTask::GoToExternalMacroDef { id, .. }
            | OngoingTask::GoToDefinition(id, ..) => id,
            _ => unreachable!("Other types have not ID field."),
        }
    }

    fn get_onset(&self) -> &Instant {
        match self {
            OngoingTask::DidRenameFiles(onset)
            | OngoingTask::FindMacroReferencesDefinitions { onset, .. }
            | OngoingTask::FindMacroReferencesSubscripts { onset, .. }
            | OngoingTask::FindReferences(.., onset)
            | OngoingTask::GoToExternalMacroDef { onset, .. }
            | OngoingTask::GoToDefinition(.., onset)
            | OngoingTask::TextDocUpdate { onset, .. } => onset,
        }
    }

    fn aborted(&self) -> bool {
        match self {
            OngoingTask::GoToExternalMacroDef { progress, .. } => progress.aborted(),
            _ => false,
        }
    }
}

impl TaskProgress {
    pub fn new(total: u32) -> Self {
        TaskProgress {
            completed: 0,
            cycles: 0,
            total,
            max_cycles: u32::MAX,
        }
    }

    #[expect(unused)]
    pub fn with_limit(progress: Self, max_cycles: u32) -> Self {
        TaskProgress {
            max_cycles,
            ..progress
        }
    }

    pub fn finished(&self) -> bool {
        self.total <= 0
    }

    /// Next task step ready for execution.
    pub fn ready(&self) -> bool {
        self.total > 0 && self.completed <= 0
    }

    pub fn aborted(&self) -> bool {
        self.total <= 0 && self.completed <= 0
    }

    pub fn advance(&mut self) {
        self.completed += 1;
        if self.completed >= self.total {
            self.cycles += 1;
            self.completed = 0;

            if self.cycles >= self.max_cycles {
                self.total = 0;
            }
        }
    }

    pub fn abort(&mut self) {
        self.total = 0
    }
}

impl ExtMacroDefLookups {
    pub fn is_empty(&self) -> bool {
        debug_assert_eq!(self.files.len(), self.callees.len());
        self.files.is_empty()
    }

    pub fn clear(&mut self) {
        debug_assert_eq!(self.files.len(), self.callees.len());

        self.files.clear();
        self.callees.clear();
    }
}

impl FileCallMap {
    pub fn new() -> Self {
        FileCallMap {
            files: Vec::new(),
            calls: Vec::new(),
        }
    }

    pub fn get(&self, file: &Uri) -> Option<&Vec<Uri>> {
        debug_assert_eq!(self.files.len(), self.calls.len());

        if let Some((_, idx)) = self.files.iter().find(|(uri, _)| *uri == *file) {
            Some(&self.calls[*idx as usize])
        } else {
            None
        }
    }

    pub fn insert(&mut self, file: Uri, call: Uri) {
        debug_assert_eq!(self.files.len(), self.calls.len());

        if let Ok(idx) = self.files.binary_search_by(|(a, _)| a.cmp(&file)) {
            if !self.calls[self.files[idx].1 as usize].contains(&call) {
                self.calls[self.files[idx].1 as usize].push(call);
            }
        } else {
            let idx = self.files.len() as u32;

            self.files.push((file, idx));
            self.calls.push(vec![call]);

            self.files.sort_by(|(a, _), (b, _)| a.cmp(b));
        }
        debug_assert_eq!(self.files.len(), self.calls.len());
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

    progress_multi_part_tasks(cfg, &g.docs, &mut g.tasks, outgoing)?;

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
        TaskDone::FindMacroReferences(id, locations) => {
            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);

                let onset = match &ts.ongoing[idx] {
                    Some(OngoingTask::FindMacroReferencesSubscripts { onset, .. }) => onset,
                    Some(OngoingTask::FindMacroReferencesDefinitions { onset, .. }) => onset,
                    _ => unreachable!("No other type possible."),
                };

                outgoing.push(Some(trace_find_refs(
                    Instant::now() - *onset,
                    locations.clone(),
                )));
            }

            outgoing.push(Some(Message::Response(Response::FindReferencesResponse(
                FindReferencesResponse {
                    id,
                    result: locations,
                },
            ))));
        }
        TaskDone::FindMacroReferencesSyncDefinitions(id, result) => {
            let mut uri: Option<Uri> = None;
            if cfg.trace_level != TraceValue::Off {
                uri = Some(result.uri.clone());
            }

            recv_find_macro_def_references_sync(&id, result, &mut ts.ongoing);
            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                let Some(OngoingTask::FindMacroReferencesDefinitions { onset, .. }) =
                    &ts.ongoing[idx]
                else {
                    unreachable!("No other type possible.");
                };
                let Some(uri) = uri else { unreachable!() };

                outgoing.push(Some(trace_find_subscript_macro_refs_sync(
                    Instant::now() - *onset,
                    id,
                    uri,
                )));
            }
        }
        TaskDone::FindMacroReferencesSyncSubscripts(id, result) => {
            let mut uri: Option<Uri> = None;
            if cfg.trace_level != TraceValue::Off {
                uri = Some(result.uri.clone());
            }

            recv_find_subscript_macro_references_sync(&id, result, &mut ts.ongoing);
            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                let Some(OngoingTask::FindMacroReferencesSubscripts { onset, .. }) =
                    &ts.ongoing[idx]
                else {
                    unreachable!("No other type possible.");
                };
                let Some(uri) = uri else { unreachable!() };

                outgoing.push(Some(trace_find_macro_definition_refs_sync(
                    Instant::now() - *onset,
                    id,
                    uri,
                )));
            }
        }
        TaskDone::FindReferences(id, result) => {
            if let Some(resp) = process_find_references_result(docs, &id, result, ts) {
                if cfg.trace_level != TraceValue::Off {
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                    let Some(OngoingTask::FindReferences(_, onset)) = &ts.ongoing[idx] else {
                        unreachable!("No other type possible.");
                    };
                    outgoing.push(Some(trace_find_refs(
                        Instant::now() - *onset,
                        resp.result.clone(),
                    )));
                }
                outgoing.push(Some(Message::Response(Response::FindReferencesResponse(
                    resp,
                ))));
            }
        }
        TaskDone::GoToDefinition(id, goto_def) => {
            if let Some(resp) = process_goto_definition_result(docs, &id, goto_def, ts) {
                if cfg.trace_level != TraceValue::Off {
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                    let Some(OngoingTask::GoToDefinition(_, onset)) = &ts.ongoing[idx] else {
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
        TaskDone::GoToExternalMacroDef(id, links) => {
            let result = if links.is_empty() {
                None
            } else {
                Some(LocationResult::ExtMeta(links))
            };

            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                let Some(OngoingTask::GoToExternalMacroDef { onset, .. }) = &ts.ongoing[idx] else {
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
        TaskDone::GoToExternalMacroDefSync(id, defs, script, callers) => {
            recv_goto_external_macro_def_sync(&id, &script, defs, callers, &mut ts.ongoing);
            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
                let Some(OngoingTask::GoToExternalMacroDef { onset, .. }) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_goto_ext_def_sync(
                    Instant::now() - *onset,
                    id,
                    script,
                )));
            }
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

fn mark_ongoing_task_completed(handle: OngoingTaskHandle, ongoing: &mut Vec<Option<OngoingTask>>) {
    let idx = match handle {
        OngoingTaskHandle::Identifier(id) => find_ongoing_task_by_id(&id, ongoing),
        OngoingTaskHandle::Uri(uri) => find_ongoing_task_by_doc(&uri, ongoing),
    };
    ongoing.remove(idx);
}

pub fn try_schedule(
    ts: &mut TaskSystem,
    job: Task,
    ongoing: &mut Vec<Option<OngoingTask>>,
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
    ongoing: &mut Vec<Option<OngoingTask>>,
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
fn task_blocked(job: &Task, ongoing: &[Option<OngoingTask>]) -> bool {
    match job {
        Task::DidRenameFiles(RenameFileOperations { old, .. }, ..) => {
            ongoing.iter().any(|o| match o {
                Some(OngoingTask::DidRenameFiles(..)) => true,
                Some(OngoingTask::TextDocUpdate { uri: file, .. }) => old.contains(&file),
                None => unreachable!("Not empty slots allowed."),
                _ => false,
            })
        }
        Task::FindReferences(
            _,
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::GoToDefinition(
            _,
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::TextDocEdit(
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::TextDocNew(TextDocumentItem { uri, .. }, ..) => ongoing.iter().any(|o| match o {
            Some(OngoingTask::DidRenameFiles(..)) => true,
            Some(OngoingTask::TextDocUpdate { uri: file, .. }) => file == uri,
            None => unreachable!("Not empty slots allowed."),
            _ => false,
        }),
        Task::WorkspaceFileScan(url, ..) => ongoing.iter().any(|o| match o {
            Some(OngoingTask::DidRenameFiles(..)) => true,
            Some(OngoingTask::TextDocUpdate { uri: file, .. }) => file == url.as_str(),
            None => unreachable!("Not empty slots allowed."),
            _ => false,
        }),
        _ => false,
    }
}

/// Document updates need to be processed in the order in which they were
/// received. Hence, we need to monitor for which documents we are currently
/// processing an update.
fn add_task_status_tracking(job: &Task, ongoing: &mut Vec<Option<OngoingTask>>) {
    let t = match job {
        Task::DidRenameFiles { .. } => OngoingTask::DidRenameFiles(Instant::now()),
        Task::FindReferences(id, ..) => OngoingTask::FindReferences(id.clone(), Instant::now()),
        Task::GoToDefinition(id, ..) => OngoingTask::GoToDefinition(id.clone(), Instant::now()),
        Task::TextDocNew(TextDocumentItem { uri, .. }, ..)
        | Task::TextDocEdit(
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        ) => OngoingTask::TextDocUpdate {
            uri: uri.clone(),
            onset: Instant::now(),
        },
        Task::WorkspaceFileScan(url, ..) => OngoingTask::TextDocUpdate {
            uri: url.to_string(),
            onset: Instant::now(),
        },
        _ => return,
    };
    ongoing.push(Some(t));
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
        Message::Request(Request::FindReferences { id, params }) => {
            process_find_references_req(
                id,
                params,
                cfg.trace_level,
                &mut g.docs,
                &mut g.tasks,
                outgoing,
            )?;
        }
        Message::Request(Request::GoToDefinition { id, params }) => {
            process_goto_definition_req(
                id,
                params,
                cfg.trace_level,
                &mut g.docs,
                &mut g.tasks,
                outgoing,
            )?;
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

fn progress_multi_part_tasks(
    cfg: &Config,
    docs: &TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    let mut tasks: Vec<Task> = Vec::new();

    for job in ts.ongoing.iter_mut() {
        let Some(task) = job else {
            unreachable!("No empty slots allowed.")
        };

        if cfg.trace_level != TraceValue::Off && task.aborted() {
            outgoing.push(Some(trace_task_aborted(
                Instant::now() - *task.get_onset(),
                task.get_id(),
            )));
        }

        match task {
            OngoingTask::GoToExternalMacroDef { .. } => {
                progress_goto_external_macro_def(docs, job, &mut tasks, &mut ts.completed)?
            }
            OngoingTask::FindMacroReferencesDefinitions { .. } => {
                progress_find_macro_def_references(docs, job, &mut tasks)?
            }
            OngoingTask::FindMacroReferencesSubscripts { .. } => {
                progress_find_subscript_macro_refs(docs, job, &mut tasks, &mut ts.completed)?
            }
            _ => (),
        }
    }

    for job in tasks {
        try_schedule(&mut ts.runner, job, &mut ts.ongoing, &mut ts.blocked)?;
    }
    Ok(())
}

fn get_task_onset_by_doc<'a>(doc: &str, ongoing: &'a [Option<OngoingTask>]) -> &'a Instant {
    let idx = find_ongoing_task_by_doc(doc, ongoing);
    let Some(task) = &ongoing[idx] else {
        unreachable!("No empty slots allowed.")
    };
    task.get_onset()
}

fn get_rename_task_onset<'a>(ongoing: &'a [Option<OngoingTask>]) -> &'a Instant {
    let idx = find_ongoing_rename_task(ongoing);
    let Some(task) = &ongoing[idx] else {
        unreachable!("No empty slots allowed.")
    };
    task.get_onset()
}

fn find_ongoing_task_by_id(identifier: &NumberOrString, ongoing: &[Option<OngoingTask>]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            Some(OngoingTask::FindReferences(id, ..))
            | Some(OngoingTask::FindMacroReferencesDefinitions { id, .. })
            | Some(OngoingTask::FindMacroReferencesSubscripts { id, .. })
            | Some(OngoingTask::GoToDefinition(id, ..))
            | Some(OngoingTask::GoToExternalMacroDef { id, .. }) => id == identifier,
            None => unreachable!("Not empty slots allowed."),
            _ => unreachable!("No other tasks can by selected by id."),
        })
        .expect("Must be a registered task.")
}

fn find_ongoing_task_by_doc(doc: &str, ongoing: &[Option<OngoingTask>]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            Some(OngoingTask::TextDocUpdate { uri, .. }) => uri == doc,
            None => unreachable!("Not empty slots allowed."),
            _ => unreachable!("No other tasks can by selected by document."),
        })
        .expect("Must be a registered task.")
}

fn find_ongoing_rename_task<'a>(ongoing: &'a [Option<OngoingTask>]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            Some(OngoingTask::DidRenameFiles(..)) => true,
            Some(_) => false,
            None => unreachable!("Not empty slots allowed."),
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

fn trace_find_refs(duration: Duration, refs: Option<Vec<Location>>) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Find references request completed in {:.4} seconds.",
                duration.as_secs_f32()
            ),
            verbose: if let Some(loc) = refs {
                Some(json!(loc).to_string())
            } else {
                None
            },
        },
    })
}

fn trace_find_macro_definition_refs_sync(
    duration: Duration,
    id: NumberOrString,
    file: Uri,
) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Find macro definition sync with ID {} and file \"{}\" completed in {:.4} seconds.",
                id,
                file,
                duration.as_secs_f32()
            ),
            verbose: None,
        },
    })
}

fn trace_find_subscript_macro_refs_sync(
    duration: Duration,
    id: NumberOrString,
    file: Uri,
) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Find subscript macro sync with ID {} and file \"{}\" completed in {:.4} seconds.",
                id,
                file,
                duration.as_secs_f32()
            ),
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

fn trace_goto_ext_def_sync(duration: Duration, id: NumberOrString, file: Uri) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Go to external definition sync for ID {} and file \"{}\" completed in {:.4} seconds.",
                id,
                file,
                duration.as_secs_f32()
            ),
            verbose: None,
        },
    })
}

fn trace_task_aborted(duration: Duration, id: &NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "WARNING: Processing of request with ID {} aborted after {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: None,
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

        let mut ongoing = Vec::<Option<OngoingTask>>::new();
        let mut blocked = Vec::<Task>::new();

        try_schedule(&mut ts, job, &mut ongoing, &mut blocked).expect("Must not fail.");

        assert!(matches!(
            ongoing[0],
            Some(OngoingTask::TextDocUpdate { .. })
        ));

        try_schedule(&mut ts, job_copy, &mut ongoing, &mut blocked).expect("Must not fail.");

        assert_eq!(ongoing.len(), 1);
        assert!(matches!(blocked[0], Task::TextDocNew { .. }));
    }
}
