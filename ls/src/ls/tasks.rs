// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

pub mod startup;

mod docsync;
mod lang;
mod progress;
mod runners;
mod workspace;

pub use runners::{Task, TaskDone, TaskSystem};

use std::time::{Duration, Instant};

use serde_json::json;
use tree_sitter::Tree;

use crate::{
    ReturnCode,
    config::{Config, Workspace},
    ls::{
        self, RunState, Tasks,
        doc::{TextDoc, TextDocData, TextDocStatus, TextDocs},
        log_notif,
        lsp::Message,
        request::{Notification, Request},
        response::{
            ErrorResponse, FindReferencesResponse, FoldingRangeResponse, GoToDefinitionResponse,
            LocationResult, NullResponse, Response, SemanticTokensFullResponse,
            SemanticTokensRangeResponse,
        },
        tasks::{
            docsync::{process_doc_change_notif, process_doc_close_notif, process_doc_open_notif},
            lang::{
                FindDefintionsForMacroRefResult, MacroDefinitionLocation, MacroReferenceOrigin,
                process_find_references_req, process_find_references_result,
                process_folding_range_req, process_goto_definition_req,
                process_goto_definition_result, process_semantic_tokens_full_req,
                process_semantic_tokens_range_req, progress_find_external_macro_definitions,
                progress_find_macro_def_references, progress_find_subscript_macro_refs,
                progress_goto_external_macro_def,
                recv_find_external_definitions_for_macro_reference_sync,
                recv_find_macro_def_references_sync, recv_find_subscript_macro_references_sync,
                recv_goto_external_macro_def_sync,
            },
            workspace::{process_files_did_rename_notif, process_rename_files_result},
        },
        workspace::{FileIndex, ResolvedRenameFileOperations, WorkspaceMembers},
    },
    protocol::{
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, ErrorCodes, FileRename, Location,
        LocationLink, LogTraceParams, NumberOrString, ProgressParams, ProgressToken, ResponseError,
        SetTraceParams, TextDocumentItem, TraceValue, Uri, WorkDoneProgressCancelParams,
    },
    t32::{LANGUAGE_ID, lang_id_supported},
    utils::FileLocationMap,
};

#[derive(Debug, PartialEq)]
pub enum FindMacroReferencesPhase {
    ReferencesInSubscripts {
        visited: Vec<Uri>,
        results: FileLocationMap,
        undone: Vec<Uri>,
    },
    ReferencesFromDefinitions {
        subscripts: Vec<Uri>,
        results: FileLocationMap,
        undone: MacroDefinitionLocationMap,
    },
    ExternalDefinitions {
        visited: FileCallMap,
        results: MacroDefinitionLocationMap,
        undone: ExtMacroDefLookups,
    },
}

#[derive(Debug, PartialEq)]
pub enum OngoingTask {
    CodeFolds(NumberOrString, Instant),
    DidRenameFiles(NumberOrString, Instant),
    FindMacroReferences {
        id: NumberOrString,
        onset: Instant,
        progress: TaskProgress,
        origin: MacroReferenceOrigin,
        phase: FindMacroReferencesPhase,
    },
    FindReferences(NumberOrString, Instant),
    GoToDefinition(NumberOrString, Instant),
    GoToExternalMacroDef {
        id: NumberOrString,
        onset: Instant,
        progress: TaskProgress,
        origin: MacroReferenceOrigin,
        visited: FileCallMap,
        undone: ExtMacroDefLookups,
        results: Vec<LocationLink>,
    },
    SemanticTokensFull(NumberOrString, Instant),
    SemanticTokensRange(NumberOrString, Instant),
    TextDocUpdate {
        uri: Uri,
        onset: Instant,
    },
    WindowWorkDoneProgress {
        id: NumberOrString,
        token: ProgressToken,
        onset: Instant,
        work: NumberOrString,
        phase: WorkDoneProgressPhase,
    },
    WorkspaceDiscovery {
        id: NumberOrString,
        onset: Instant,
        progress: TaskProgress,
        phase: WorkspaceDiscoveryPhase,
    },
}

#[derive(Debug, PartialEq)]
pub enum OngoingTaskHandle {
    Identifier(NumberOrString),
    Uri(Uri),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProgressCounter {
    Ready,
    Counting(u32),
}

#[derive(Debug, PartialEq, Eq)]
pub enum WorkDoneProgressPhase {
    Ready(ProgressParams),
    Announced(ProgressParams),
    Aborted,
    Initialized(ProgressParams),
    Reporting {
        reported: u32,
        next: Option<ProgressParams>,
    },
    Finished {
        begin: Option<ProgressParams>,
        end: Option<ProgressParams>,
    },
}

#[derive(Debug, PartialEq)]
pub enum WorkspaceDiscoveryPhase {
    Scanning(Workspace, Option<WorkspaceMembers>),
    Indexing(WorkspaceMembers, Option<FileIndex>),
    Parsing(WorkspaceMembers, FileIndex),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskProgress {
    pub completed: ProgressCounter,
    pub total: u32,
    cycles: u32,
    max_cycles: u32,
}

/// To find macro definitions in other files, we need to check for the presence
/// of script calls. `LOCAL` and `GLOBAL` macro definitions remain valid in
/// subscripts.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtMacroDefLookups {
    pub files: Vec<Uri>,
    pub callees: Vec<Uri>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileCallMap {
    files: Vec<(Uri, u32)>,
    calls: Vec<Vec<Uri>>,
}

// TODO: Split of key.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroDefinitionLocationMap {
    files: Vec<(Uri, u32)>,
    macros: Vec<Vec<MacroDefinitionLocation>>,
}

pub struct MacroDefinitionLocationMapIntoIterator {
    map: MacroDefinitionLocationMap,
}

pub struct MacroDefinitionLocationMapIterator<'a> {
    map: &'a MacroDefinitionLocationMap,
    idx: usize,
}

#[derive(Clone, Debug)]
pub struct RenameFileOperations {
    pub old: Vec<Uri>,
    pub new: Vec<Uri>,
}

impl MacroDefinitionLocationMap {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            macros: Vec::new(),
        }
    }

    pub fn insert(&mut self, file: &Uri, def: MacroDefinitionLocation) {
        debug_assert_eq!(self.files.len(), self.macros.len());
        match self.files.binary_search_by(|(f, _)| f.cmp(file)) {
            Ok(ii) => {
                match self.macros[self.files[ii].1 as usize].binary_search_by(|m| m.cmp(&def)) {
                    Ok(_) => return,
                    Err(idx) => self.macros[self.files[ii].1 as usize].insert(idx, def),
                }
            }
            Err(ii) => {
                self.files
                    .insert(ii, (file.clone(), self.macros.len() as u32));
                self.macros.push(vec![def]);
            }
        }
        debug_assert_eq!(self.files.len(), self.macros.len());
    }

    pub fn clear(&mut self) {
        debug_assert_eq!(self.files.len(), self.macros.len());

        self.files.clear();
        self.macros.clear();

        debug_assert_eq!(self.files.len(), self.macros.len());
    }

    #[expect(unused)]
    pub fn get(&self, file: &Uri) -> Option<&[MacroDefinitionLocation]> {
        debug_assert_eq!(self.files.len(), self.macros.len());

        if let Ok(ii) = self.files.binary_search_by(|(f, _)| f.cmp(file)) {
            Some(&self.macros[self.files[ii].1 as usize])
        } else {
            None
        }
    }

    pub fn iter<'a>(&'a self) -> MacroDefinitionLocationMapIterator<'a> {
        debug_assert_eq!(self.files.len(), self.macros.len());

        MacroDefinitionLocationMapIterator { map: self, idx: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn num_files(&self) -> usize {
        self.files.len()
    }
}

impl OngoingTask {
    pub fn get_id(&self) -> &NumberOrString {
        match self {
            OngoingTask::CodeFolds(id, ..)
            | OngoingTask::DidRenameFiles(id, ..)
            | OngoingTask::FindMacroReferences { id, .. }
            | OngoingTask::FindReferences(id, ..)
            | OngoingTask::GoToExternalMacroDef { id, .. }
            | OngoingTask::GoToDefinition(id, ..)
            | OngoingTask::SemanticTokensFull(id, ..)
            | OngoingTask::SemanticTokensRange(id, ..)
            | OngoingTask::WindowWorkDoneProgress { id, .. }
            | OngoingTask::WorkspaceDiscovery { id, .. } => id,
            OngoingTask::TextDocUpdate { .. } => {
                unreachable!("Other types have not ID field.")
            }
        }
    }

    pub fn get_onset(&self) -> &Instant {
        match self {
            OngoingTask::CodeFolds(_, onset)
            | OngoingTask::DidRenameFiles(_, onset)
            | OngoingTask::FindMacroReferences { onset, .. }
            | OngoingTask::FindReferences(_, onset)
            | OngoingTask::GoToExternalMacroDef { onset, .. }
            | OngoingTask::GoToDefinition(.., onset)
            | OngoingTask::SemanticTokensFull(_, onset)
            | OngoingTask::SemanticTokensRange(_, onset)
            | OngoingTask::TextDocUpdate { onset, .. }
            | OngoingTask::WindowWorkDoneProgress { onset, .. }
            | OngoingTask::WorkspaceDiscovery { onset, .. } => onset,
        }
    }

    pub fn aborted(&self) -> bool {
        match self {
            OngoingTask::FindMacroReferences { progress, .. }
            | OngoingTask::GoToExternalMacroDef { progress, .. }
            | OngoingTask::WorkspaceDiscovery { progress, .. } => progress.aborted(),
            OngoingTask::CodeFolds(..)
            | OngoingTask::DidRenameFiles(..)
            | OngoingTask::FindReferences(..)
            | OngoingTask::GoToDefinition(..)
            | OngoingTask::SemanticTokensFull { .. }
            | OngoingTask::SemanticTokensRange { .. }
            | OngoingTask::TextDocUpdate { .. }
            | OngoingTask::WindowWorkDoneProgress { .. } => false,
        }
    }
}

impl ProgressCounter {
    pub fn value(&self) -> u32 {
        match self {
            Self::Ready => 0u32,
            Self::Counting(val) => *val,
        }
    }
}

impl TaskProgress {
    pub fn new(total: u32) -> Self {
        TaskProgress {
            completed: ProgressCounter::Ready,
            cycles: 0,
            total,
            max_cycles: u32::MAX,
        }
    }

    pub fn with_limit(mut self, max_cycles: u32) -> Self {
        self.max_cycles = max_cycles;
        self
    }

    #[cfg(test)]
    pub fn set_cycles(&mut self, cycles: u32) {
        self.cycles = cycles;
    }

    pub fn finished(&self) -> bool {
        self.total <= 0
    }

    pub fn ack_ready(&mut self) {
        self.completed = ProgressCounter::Counting(0);
    }

    /// Next task step ready for execution.
    pub fn ready(&self) -> bool {
        if self.total <= 0 {
            false
        } else if let ProgressCounter::Ready = self.completed {
            true
        } else {
            false
        }
    }

    #[expect(unused)]
    pub fn counting(&self) -> bool {
        !(self.completed == ProgressCounter::Ready)
    }

    pub fn aborted(&self) -> bool {
        if self.total > 0 {
            false
        } else if let ProgressCounter::Counting(0) = self.completed {
            true
        } else {
            false
        }
    }

    pub fn advance(&mut self) {
        let counter: u32 = match self.completed {
            ProgressCounter::Ready => 1,
            ProgressCounter::Counting(val) => val + 1,
        };

        if counter >= self.total {
            self.cycles += 1;
            self.completed = ProgressCounter::Ready;

            if self.cycles >= self.max_cycles {
                self.total = 0;
            }
        } else {
            self.completed = ProgressCounter::Counting(counter);
        }
    }

    pub fn mark_completed(&mut self) {
        self.total = 0
    }
}

impl ExtMacroDefLookups {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            callees: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        debug_assert_eq!(self.files.len(), self.callees.len());
        self.files.is_empty()
    }

    pub fn clear(&mut self) {
        debug_assert_eq!(self.files.len(), self.callees.len());

        self.files.clear();
        self.callees.clear();

        debug_assert_eq!(self.files.len(), self.callees.len());
    }

    pub fn add(&mut self, file: Uri, callee: Uri) {
        debug_assert_eq!(self.files.len(), self.callees.len());

        self.files.push(file);
        self.callees.push(callee);

        debug_assert_eq!(self.files.len(), self.callees.len());
    }

    pub fn num_files(&self) -> usize {
        self.files.len()
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

        if let Ok(ii) = self.files.binary_search_by(|(f, _)| f.cmp(file)) {
            Some(&self.calls[self.files[ii].1 as usize])
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

impl IntoIterator for MacroDefinitionLocationMap {
    type Item = (Uri, Vec<MacroDefinitionLocation>);
    type IntoIter = MacroDefinitionLocationMapIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        MacroDefinitionLocationMapIntoIterator { map: self }
    }
}

impl Iterator for MacroDefinitionLocationMapIntoIterator {
    type Item = (Uri, Vec<MacroDefinitionLocation>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.map.files.is_empty() {
            None
        } else {
            let (file, offset) = self.map.files.swap_remove(0);
            let macros: Vec<MacroDefinitionLocation> = self.map.macros[offset as usize].clone();

            Some((file, macros))
        }
    }
}

impl<'a> Iterator for MacroDefinitionLocationMapIterator<'a> {
    type Item = (&'a Uri, &'a Vec<MacroDefinitionLocation>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.map.files.len() {
            let offset = self.idx;
            self.idx += 1;

            let file: &Uri = &self.map.files[offset].0;
            let offset: u32 = self.map.files[offset].1;

            Some((file, &self.map.macros[offset as usize]))
        } else {
            None
        }
    }
}

pub fn recv_completed_tasks(
    cfg: &Config,
    ts: &mut Tasks,
    docs: &mut TextDocs,
    files: &mut FileIndex,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<bool, ReturnCode> {
    let mut completed: Vec<TaskDone> = Vec::new();
    for done in ts.runner.rx.try_iter() {
        completed.push(done);
    }

    for done in ts.completed.iter_mut() {
        completed.push(done.take().expect("No empty slots allowed."));
    }
    ts.completed.clear();

    let compl_recv = !completed.is_empty();

    for done in completed {
        let handle = done.get_task_handle();
        let finished = process_completed_task(done, cfg, ts, docs, files, outgoing)?;

        if finished && let Some(h) = handle {
            conclude_work_done_progress(&h, &mut ts.ongoing);
            mark_ongoing_task_completed(&h, &mut ts.ongoing);
        }
    }
    Ok(compl_recv)
}

pub fn recv_responses(
    trace_level: TraceValue,
    incoming: &mut [Option<Message>],
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) {
    for msg in incoming {
        let Some(Message::Response(resp)) = msg else {
            continue;
        };

        match resp {
            Response::NullResponse(NullResponse { id }) => {
                let idx = find_ongoing_task_by_id(id, &ts.ongoing);
                if trace_level != TraceValue::Off && idx.is_none() {
                    warn_response_id_unknown(id.clone());
                } else {
                    eval_client_success_response(
                        trace_level,
                        &mut ts.ongoing[idx.unwrap()]
                            .as_mut()
                            .expect("Not empty slots allowed."),
                        outgoing,
                    );
                }
            }
            Response::ErrorResponse(ErrorResponse { id, .. }) => {
                if id.is_none() {
                    continue;
                }
                let id = id.as_ref().unwrap();

                let idx = find_ongoing_task_by_id(id, &ts.ongoing);
                if trace_level != TraceValue::Off && idx.is_none() {
                    warn_response_id_unknown(id.clone());
                } else {
                    eval_client_error_response(
                        trace_level,
                        &mut ts.ongoing[idx.unwrap()]
                            .as_mut()
                            .expect("Not empty slots allowed."),
                        outgoing,
                    );
                }
            }
            Response::FoldingRangeResponse(..)
            | Response::FindReferencesResponse(..)
            | Response::GoToDefinitionResponse(..)
            | Response::InitializeResponse(..)
            | Response::SemanticTokensFullResponse(..)
            | Response::SemanticTokensRangeResponse(..) => {
                unreachable!("Response types are not forwarded.")
            }
        }
    }
}

pub fn schedule_tasks(
    incoming: &mut [Option<Message>],
    g: &mut RunState,
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
) -> Result<bool, ReturnCode> {
    match done {
        TaskDone::CodeFolds(id, folds) => {
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::CodeFolds(_, onset)) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_folding_range(
                    Instant::now() - *onset,
                    id.clone(),
                )));
            }

            outgoing.push(Some(Message::Response(Response::FoldingRangeResponse(
                FoldingRangeResponse {
                    id,
                    result: if folds.is_empty() { None } else { Some(folds) },
                },
            ))));
            Ok(true)
        }
        TaskDone::DidRenameFiles(
            id,
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
                outgoing.push(Some(trace_doc_rename(
                    id,
                    &renamed,
                    Instant::now() - *onset,
                )));

                for dir in missing_dirs {
                    outgoing.push(Some(trace_dir_unknown(&dir)));
                }

                for file in missing_files {
                    outgoing.push(Some(trace_doc_unknown(&file)));
                }
            }
            Ok(true)
        }
        TaskDone::FindExternalDefinitionsForMacroRefSync(id, result) => {
            let mut uri: Option<Uri> = None;
            if cfg.trace_level != TraceValue::Off {
                uri = match &result {
                    FindDefintionsForMacroRefResult::Final(_, file)
                    | FindDefintionsForMacroRefResult::Partial(_, file, _) => Some(file.clone()),
                };
            }
            recv_find_external_definitions_for_macro_reference_sync(&id, result, &mut ts.ongoing);

            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");

                let onset = match &ts.ongoing[idx] {
                    Some(OngoingTask::FindMacroReferences { onset, .. }) => onset,
                    _ => unreachable!("No other type possible."),
                };
                let Some(uri) = uri else { unreachable!() };

                outgoing.push(Some(trace_find_ext_definitions_for_macro_ref_sync(
                    Instant::now() - *onset,
                    id,
                    uri,
                )));
            }
            Ok(false)
        }
        TaskDone::FindMacroReferences(id, locations) => {
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");

                let onset = match &ts.ongoing[idx] {
                    Some(OngoingTask::FindMacroReferences { onset, .. }) => onset,
                    _ => unreachable!("No other type possible."),
                };

                outgoing.push(Some(trace_find_refs(
                    id.clone(),
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
            Ok(true)
        }
        TaskDone::FindMacroReferencesFromDefinitionsSync(id, result) => {
            let mut uri: Option<Uri> = None;
            if cfg.trace_level != TraceValue::Off {
                uri = Some(result.uri.clone());
            }

            recv_find_macro_def_references_sync(&id, result, &mut ts.ongoing);
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::FindMacroReferences { onset, .. }) = &ts.ongoing[idx] else {
                    unreachable!("Must not retrieve any other variant.");
                };
                let Some(uri) = uri else { unreachable!() };

                outgoing.push(Some(trace_find_macro_definition_refs_sync(
                    Instant::now() - *onset,
                    id,
                    uri,
                )));
            }
            Ok(false)
        }
        TaskDone::FindMacroReferencesInSubscriptsSync(id, result) => {
            let mut uri: Option<Uri> = None;
            if cfg.trace_level != TraceValue::Off {
                uri = Some(result.uri.clone());
            }

            recv_find_subscript_macro_references_sync(&id, result, &mut ts.ongoing);
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::FindMacroReferences { onset, .. }) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                let Some(uri) = uri else { unreachable!() };

                outgoing.push(Some(trace_find_subscript_macro_refs_sync(
                    Instant::now() - *onset,
                    id,
                    uri,
                )));
            }
            Ok(false)
        }
        TaskDone::FindReferences(id, result) => {
            if let Some(resp) =
                process_find_references_result(cfg, docs, files, &id, result, ts, outgoing)
            {
                if cfg.trace_level != TraceValue::Off {
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing)
                        .expect("Must be a registered task.");
                    let Some(OngoingTask::FindReferences(_, onset)) = &ts.ongoing[idx] else {
                        unreachable!("Must not retrieve any other variant.");
                    };
                    outgoing.push(Some(trace_find_refs(
                        id,
                        Instant::now() - *onset,
                        resp.result.clone(),
                    )));
                }
                outgoing.push(Some(Message::Response(Response::FindReferencesResponse(
                    resp,
                ))));
                Ok(true)
            } else {
                Ok(false)
            }
        }
        TaskDone::GoToDefinition(id, goto_def) => {
            if let Some(resp) =
                process_goto_definition_result(docs, &id, goto_def, cfg.trace_level, ts, outgoing)
            {
                if cfg.trace_level != TraceValue::Off {
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing)
                        .expect("Must be a registered task.");
                    let Some(OngoingTask::GoToDefinition(_, onset)) = &ts.ongoing[idx] else {
                        unreachable!("Must not retrieve any other variant.");
                    };
                    outgoing.push(Some(trace_goto_def(
                        Instant::now() - *onset,
                        id,
                        resp.result.clone(),
                    )));
                }
                outgoing.push(Some(Message::Response(Response::GoToDefinitionResponse(
                    resp,
                ))));
                Ok(true)
            } else {
                Ok(false)
            }
        }
        TaskDone::GoToExternalMacroDef(id, links) => {
            let result = if links.is_empty() {
                None
            } else {
                Some(LocationResult::ExtMeta(links))
            };

            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::GoToExternalMacroDef { onset, .. }) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_goto_def(
                    Instant::now() - *onset,
                    id.clone(),
                    result.clone(),
                )));
            }

            outgoing.push(Some(Message::Response(Response::GoToDefinitionResponse(
                GoToDefinitionResponse { id, result },
            ))));
            Ok(true)
        }
        TaskDone::GoToExternalMacroDefSync(id, defs, script, callers) => {
            recv_goto_external_macro_def_sync(&id, &script, defs, callers, &mut ts.ongoing);
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::GoToExternalMacroDef { onset, .. }) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_goto_ext_def_sync(
                    Instant::now() - *onset,
                    id,
                    script,
                )));
            }
            Ok(false)
        }
        TaskDone::SemanticTokensFull(id, tokens) => {
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::SemanticTokensFull(_, onset)) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_sem_tokens_full(
                    Instant::now() - *onset,
                    id.clone(),
                )));
            }

            outgoing.push(Some(Message::Response(
                Response::SemanticTokensFullResponse(SemanticTokensFullResponse {
                    id,
                    result: Some(tokens),
                }),
            )));
            Ok(true)
        }
        TaskDone::SemanticTokensRange(id, tokens) => {
            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::SemanticTokensRange(_, onset)) = &ts.ongoing[idx] else {
                    unreachable!("No other type possible.");
                };
                outgoing.push(Some(trace_sem_tokens_range(
                    Instant::now() - *onset,
                    id.clone(),
                )));
            }

            outgoing.push(Some(Message::Response(
                Response::SemanticTokensRangeResponse(SemanticTokensRangeResponse {
                    id,
                    result: Some(tokens),
                }),
            )));
            Ok(true)
        }
        TaskDone::TextDocNew(doc, tree, globals) => {
            if cfg.trace_level != TraceValue::Off {
                let onset = get_task_onset_by_doc(&doc.uri, &ts.ongoing);
                outgoing.push(Some(trace_doc_change(&doc, &tree, Instant::now() - *onset)));
            }
            docs.add(doc, tree, globals, TextDocStatus::Open);
            Ok(true)
        }
        TaskDone::TextDocEdit(doc, tree, globals) => {
            if cfg.trace_level != TraceValue::Off {
                let onset = get_task_onset_by_doc(&doc.uri, &ts.ongoing);
                outgoing.push(Some(trace_doc_change(&doc, &tree, Instant::now() - *onset)));
            }
            docs.update(doc, tree, globals);
            Ok(true)
        }
        TaskDone::WindowWorkDoneProgress(id, aborted) => {
            if cfg.trace_level != TraceValue::Off {
                let onset = get_task_onset_by_id(&id, &ts.ongoing);
                outgoing.push(Some(trace_window_workdone(
                    Instant::now() - *onset,
                    id.clone(),
                    aborted,
                )));
            }
            Ok(true)
        }
        TaskDone::WorkspaceFileParseSync(id, file_data) => {
            let mut uri: Option<Uri> = None;
            if cfg.trace_level != TraceValue::Off
                && let Ok((doc, _, _)) = &file_data
            {
                uri = Some(doc.uri.clone());
            }

            workspace::recv_workspace_file_parsing_sync(
                cfg.trace_level,
                &id,
                file_data,
                docs,
                &mut ts.ongoing,
                outgoing,
            );

            if cfg.trace_level != TraceValue::Off {
                let idx =
                    find_ongoing_task_by_id(&id, &ts.ongoing).expect("Must be a registered task.");
                let Some(OngoingTask::WorkspaceDiscovery { onset, .. }) = &ts.ongoing[idx] else {
                    unreachable!("Must not retrieve any other variant.");
                };

                outgoing.push(Some(trace_workspace_discovery_sync(
                    Instant::now() - *onset,
                    id,
                    if uri.is_some() {
                        Some(format!("File \"{}\"has been parsed.", uri.unwrap()))
                    } else {
                        None
                    },
                )));
            }
            Ok(false)
        }
        TaskDone::WorkspaceFileDiscovery(id) => {
            docs.sync();

            if cfg.trace_level != TraceValue::Off {
                let onset = get_task_onset_by_id(&id, &ts.ongoing);

                outgoing.push(Some(trace_workspace_indexed(
                    Instant::now() - *onset,
                    id.clone(),
                    &cfg.workspace,
                )));
            }
            Ok(true)
        }
        TaskDone::WorkspaceFileDiscoverySync(..) | TaskDone::WorkspaceFileIndexSync(..) => {
            unreachable!("These tasks have already been completed before main loop entry.")
        }
    }
}

pub fn mark_ongoing_task_completed(
    handle: &OngoingTaskHandle,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = match handle {
        OngoingTaskHandle::Identifier(id) => {
            find_ongoing_task_by_id(id, ongoing).expect("Must be a registered task.")
        }
        OngoingTaskHandle::Uri(uri) => find_ongoing_task_by_doc(uri, ongoing),
    };
    ongoing.remove(idx);
}
pub fn process_msg(
    msg: Message,
    g: &mut RunState,
    cfg: &mut Config,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    match msg {
        // All new requests after a shutdown request was received should
        // be trigger an `InvalidRequest` error.
        m if g.shutdown_request_recv && m.is_request() => {
            outgoing.push(Some(ls::error_shutdown_seq(
                m.get_request().get_id().clone(),
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
                process_doc_open_notif(
                    text_document,
                    g.files.clone(),
                    cfg.t32_dirs.clone(),
                    &mut g.tasks,
                )?;
            } else {
                outgoing.push(Some(error_lang_id_unsupported(&text_document.language_id)));
            }
        }
        Message::Notification(Notification::DidChangeTextDocumentNotification { params }) => {
            process_doc_change_notif(
                params,
                &g.docs,
                g.files.clone(),
                cfg.t32_dirs.clone(),
                &mut g.tasks,
                outgoing,
            )?;
        }
        Message::Notification(Notification::DidRenameFilesNotification { params }) => {
            process_files_did_rename_notif(&mut g.tasks, params.files, g.files.clone())?;
        }
        Message::Notification(Notification::SetTraceNotification {
            params: SetTraceParams { value },
        }) => {
            cfg.trace_level = value;
        }
        Message::Request(Request::FoldingRange { id, params }) => {
            process_folding_range_req(
                id,
                params,
                cfg.trace_level,
                cfg.code_folding.clone(),
                &mut g.docs,
                &mut g.tasks,
                outgoing,
            )?;
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
        Message::Request(Request::SemanticTokensFull { id, params }) => {
            process_semantic_tokens_full_req(
                id,
                params,
                cfg.trace_level,
                cfg.semantic_tokens.clone(),
                &mut g.docs,
                &mut g.tasks,
                outgoing,
            )?;
        }
        Message::Request(Request::SemanticTokensRange { id, params }) => {
            process_semantic_tokens_range_req(
                id,
                params,
                cfg.trace_level,
                cfg.semantic_tokens.clone(),
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
        Message::Notification(Notification::WorkDoneProgressCancelNotification {
            params: WorkDoneProgressCancelParams { token },
        }) => {
            progress::cancel_server_workdone_progress(
                cfg.trace_level,
                token,
                &mut g.tasks.ongoing,
                outgoing,
            );
        }
        // Ignore these messages silently.
        Message::Notification(Notification::InitializedNotification { .. })
        | Message::Notification(Notification::LogTraceNotification { .. })
        | Message::Notification(Notification::WorkDoneProgressNotification { .. })
        | Message::Response(_)
        | Message::Request(Request::InitializeRequest { .. })
        | Message::Request(Request::WindowWorkDoneProgressCreate { .. }) => (),
    }
    Ok(())
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
    let mut exec: Vec<usize> = Vec::new();

    for (ii, job) in blocked.iter().enumerate() {
        if task_blocked(job, &ongoing) {
            continue;
        }
        ts.schedule(job)?;
        add_task_status_tracking(job, ongoing);

        exec.push(ii);
    }

    for ii in exec.iter().rev() {
        blocked.swap_remove(*ii);
    }

    Ok(())
}

/// Some requests like document updates can only be processed one at a time.
/// This functions checks whether there is an ongoing task that would block
/// the scheduling of the new one.
/// Document lookup operations like *Go to Definition* should use the latest
/// document version, so we delay the corresponding task until the document
/// update has been completed.
/// Rename operations delay all file updates, because they may target a file
/// which is in the process of being renamed. Until the complete workspace
/// has been indexed, file rename operations have to wait.
fn task_blocked(job: &Task, ongoing: &[Option<OngoingTask>]) -> bool {
    match job {
        Task::DidRenameFiles(_, RenameFileOperations { old, .. }, ..) => {
            // Continue with rename operations even though other requests that
            // query file data are ongoing.
            ongoing.iter().any(|o| match o {
                Some(task) => match task {
                    OngoingTask::DidRenameFiles(..) | OngoingTask::WorkspaceDiscovery { .. } => {
                        true
                    }
                    OngoingTask::TextDocUpdate { uri: file, .. } => old.contains(&file),

                    OngoingTask::CodeFolds(..)
                    | OngoingTask::FindMacroReferences { .. }
                    | OngoingTask::FindReferences(..)
                    | OngoingTask::GoToDefinition(..)
                    | OngoingTask::GoToExternalMacroDef { .. }
                    | OngoingTask::SemanticTokensFull { .. }
                    | OngoingTask::SemanticTokensRange { .. }
                    | OngoingTask::WindowWorkDoneProgress { .. } => false,
                },
                None => unreachable!("Not empty slots allowed."),
            })
        }
        Task::CodeFolds(
            _,
            _,
            _,
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::FindExternalDefinitionsForMacroRef {
            textdoc:
                TextDocData {
                    doc: TextDoc { uri, .. },
                    ..
                },
            ..
        }
        | Task::FindMacroReferencesFromDefinitions {
            textdoc:
                TextDocData {
                    doc: TextDoc { uri, .. },
                    ..
                },
            ..
        }
        | Task::FindMacroReferencesInSubscripts {
            textdoc:
                TextDocData {
                    doc: TextDoc { uri, .. },
                    ..
                },
            ..
        }
        | Task::FindReferences {
            textdoc:
                TextDocData {
                    doc: TextDoc { uri, .. },
                    ..
                },
            ..
        }
        | Task::GoToExternalMacroDef {
            textdoc:
                TextDocData {
                    doc: TextDoc { uri, .. },
                    ..
                },
            ..
        }
        | Task::GoToDefinition(
            _,
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::SemanticTokensFull(
            _,
            _,
            _,
            TextDocData {
                doc: TextDoc { uri, .. },
                ..
            },
            ..,
        )
        | Task::SemanticTokensRange(
            _,
            _,
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
            // Wait with requests that retrieve file data only until file
            // renaming and file data updates have completed. The results of
            // workspace discovery can be discarded if the discovery process
            // takes too long.
            Some(task) => match task {
                OngoingTask::DidRenameFiles(..) => true,
                OngoingTask::TextDocUpdate { uri: file, .. } => file == uri,
                OngoingTask::CodeFolds(..)
                | OngoingTask::FindMacroReferences { .. }
                | OngoingTask::FindReferences(..)
                | OngoingTask::GoToDefinition(..)
                | OngoingTask::GoToExternalMacroDef { .. }
                | OngoingTask::SemanticTokensFull { .. }
                | OngoingTask::SemanticTokensRange { .. }
                | OngoingTask::WindowWorkDoneProgress { .. }
                | OngoingTask::WorkspaceDiscovery { .. } => false,
            },
            None => unreachable!("Not empty slots allowed."),
        }),
        // Complete as fast as possible.
        Task::WorkspaceFileScan(..) => false,
        Task::WorkspaceFileDiscovery(..) | Task::WorkspaceFileIndexNew(..) => {
            unreachable!("These tasks are not scheduled after the server has booted.")
        }
    }
}

/// Document updates need to be processed in the order in which they were
/// received. Hence, we need to monitor for which documents we are currently
/// processing an update.
/// No status tracking for multi-stage tasks is performed.
fn add_task_status_tracking(job: &Task, ongoing: &mut Vec<Option<OngoingTask>>) {
    let t = match job {
        Task::CodeFolds(id, ..) => OngoingTask::CodeFolds(id.clone(), Instant::now()),
        Task::DidRenameFiles(id, ..) => OngoingTask::DidRenameFiles(id.clone(), Instant::now()),
        Task::FindReferences { id, .. } => OngoingTask::FindReferences(id.clone(), Instant::now()),
        Task::GoToDefinition(id, ..) => OngoingTask::GoToDefinition(id.clone(), Instant::now()),
        Task::SemanticTokensFull(id, ..) => {
            OngoingTask::SemanticTokensFull(id.clone(), Instant::now())
        }
        Task::SemanticTokensRange(id, ..) => {
            OngoingTask::SemanticTokensRange(id.clone(), Instant::now())
        }
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
        Task::FindExternalDefinitionsForMacroRef { .. }
        | Task::FindMacroReferencesFromDefinitions { .. }
        | Task::FindMacroReferencesInSubscripts { .. }
        | Task::GoToExternalMacroDef { .. }
        | Task::WorkspaceFileDiscovery(..)
        | Task::WorkspaceFileIndexNew(..)
        | Task::WorkspaceFileScan(..) => return,
    };
    ongoing.push(Some(t));
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
            OngoingTask::FindMacroReferences { phase, .. } => match phase {
                FindMacroReferencesPhase::ReferencesFromDefinitions { .. } => {
                    progress_find_macro_def_references(docs, job, &mut tasks)?
                }
                FindMacroReferencesPhase::ReferencesInSubscripts { .. } => {
                    progress_find_subscript_macro_refs(docs, job, &mut tasks, &mut ts.completed)?
                }
                FindMacroReferencesPhase::ExternalDefinitions { .. } => {
                    progress_find_external_macro_definitions(
                        docs,
                        job,
                        &mut tasks,
                        &mut ts.completed,
                    )?
                }
            },
            OngoingTask::WorkspaceDiscovery { .. } => {
                workspace::progress_workspace_file_parsing(
                    &cfg.t32_dirs,
                    job,
                    &mut tasks,
                    &mut ts.completed,
                );
            }
            OngoingTask::WindowWorkDoneProgress { .. } => {
                progress::broadcast_work_done(job, outgoing, &mut ts.completed);
            }
            OngoingTask::CodeFolds(..)
            | OngoingTask::DidRenameFiles(..)
            | OngoingTask::FindReferences(..)
            | OngoingTask::GoToDefinition(..)
            | OngoingTask::SemanticTokensFull(..)
            | OngoingTask::SemanticTokensRange(..)
            | OngoingTask::TextDocUpdate { .. } => (),
        }
    }

    for job in tasks {
        try_schedule(&mut ts.runner, job, &mut ts.ongoing, &mut ts.blocked)?;
    }
    Ok(())
}

fn conclude_work_done_progress(handle: &OngoingTaskHandle, ongoing: &mut Vec<Option<OngoingTask>>) {
    let id: &NumberOrString = match handle {
        OngoingTaskHandle::Identifier(id) => id,
        OngoingTaskHandle::Uri(_) => return,
    };

    let workspace_discovery_done: bool = {
        let idx = find_ongoing_task_by_id(id, ongoing).expect("Must be a registered task.");

        ongoing[idx].as_ref().is_some_and(|t| {
            if let OngoingTask::WorkspaceDiscovery { .. } = t {
                true
            } else {
                false
            }
        })
    };

    if workspace_discovery_done {
        let Some(idx) = progress::find_workdone_progress_by_id(&id, ongoing) else {
            return;
        };
        workspace::conclude_workspace_file_parsing_progress(&mut ongoing[idx]);
    }
}

fn eval_client_success_response(
    trace_level: TraceValue,
    job: &mut OngoingTask,
    outgoing: &mut Vec<Option<Message>>,
) {
    match job {
        OngoingTask::WindowWorkDoneProgress { .. } => {
            progress::confirm_server_workdone_progress(trace_level, job, outgoing);
        }
        // These requests are sent from client to server, so the server is
        // never the one responding.
        OngoingTask::CodeFolds(..)
        | OngoingTask::DidRenameFiles(..)
        | OngoingTask::FindMacroReferences { .. }
        | OngoingTask::FindReferences(..)
        | OngoingTask::GoToDefinition(..)
        | OngoingTask::GoToExternalMacroDef { .. }
        | OngoingTask::SemanticTokensFull(..)
        | OngoingTask::SemanticTokensRange(..)
        | OngoingTask::TextDocUpdate { .. }
        | OngoingTask::WorkspaceDiscovery { .. } => {
            debug_assert!(1 == 0);
        }
    }
}

fn eval_client_error_response(
    trace_level: TraceValue,
    job: &mut OngoingTask,
    outgoing: &mut Vec<Option<Message>>,
) {
    match job {
        OngoingTask::WindowWorkDoneProgress { .. } => {
            progress::abort_server_workdone_progress(trace_level, job, outgoing);
        }
        // These requests are sent from client to server, so the server is
        // never the one responding.
        OngoingTask::CodeFolds(..)
        | OngoingTask::DidRenameFiles(..)
        | OngoingTask::FindMacroReferences { .. }
        | OngoingTask::FindReferences(..)
        | OngoingTask::GoToDefinition(..)
        | OngoingTask::GoToExternalMacroDef { .. }
        | OngoingTask::SemanticTokensFull(..)
        | OngoingTask::SemanticTokensRange(..)
        | OngoingTask::TextDocUpdate { .. }
        | OngoingTask::WorkspaceDiscovery { .. } => {
            debug_assert!(1 == 0);
        }
    }
}

fn get_task_onset_by_doc<'a>(doc: &str, ongoing: &'a [Option<OngoingTask>]) -> &'a Instant {
    let idx = find_ongoing_task_by_doc(doc, ongoing);
    let Some(task) = &ongoing[idx] else {
        unreachable!("No empty slots allowed.")
    };
    task.get_onset()
}

fn get_task_onset_by_id<'a>(
    id: &NumberOrString,
    ongoing: &'a [Option<OngoingTask>],
) -> &'a Instant {
    let idx = find_ongoing_task_by_id(id, ongoing).expect("Must be a registered task.");
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

fn find_ongoing_task_by_id(
    identifier: &NumberOrString,
    ongoing: &[Option<OngoingTask>],
) -> Option<usize> {
    ongoing.iter().position(|t| match t {
        Some(OngoingTask::CodeFolds(id, ..))
        | Some(OngoingTask::DidRenameFiles(id, ..))
        | Some(OngoingTask::FindReferences(id, ..))
        | Some(OngoingTask::FindMacroReferences { id, .. })
        | Some(OngoingTask::GoToDefinition(id, ..))
        | Some(OngoingTask::GoToExternalMacroDef { id, .. })
        | Some(OngoingTask::SemanticTokensFull(id, _))
        | Some(OngoingTask::SemanticTokensRange(id, _))
        | Some(OngoingTask::WindowWorkDoneProgress { id, .. })
        | Some(OngoingTask::WorkspaceDiscovery { id, .. }) => id == identifier,

        Some(OngoingTask::TextDocUpdate { .. }) => false,

        None => {
            unreachable!("No other tasks can by selected by id.")
        }
    })
}

fn find_ongoing_task_by_doc(doc: &str, ongoing: &[Option<OngoingTask>]) -> usize {
    ongoing
        .iter()
        .position(|t| match t {
            Some(OngoingTask::TextDocUpdate { uri, .. }) => uri == doc,
            None => unreachable!("Not empty slots allowed."),

            Some(OngoingTask::CodeFolds(..))
            | Some(OngoingTask::DidRenameFiles(..))
            | Some(OngoingTask::FindMacroReferences { .. })
            | Some(OngoingTask::FindReferences { .. })
            | Some(OngoingTask::GoToDefinition(..))
            | Some(OngoingTask::GoToExternalMacroDef { .. })
            | Some(OngoingTask::SemanticTokensFull { .. })
            | Some(OngoingTask::SemanticTokensRange { .. })
            | Some(OngoingTask::WindowWorkDoneProgress { .. })
            | Some(OngoingTask::WorkspaceDiscovery { .. }) => false,
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
                "ERROR: Language ID \"{}\" is not supported for text documents. The only supported language ID is \"{}\".",
                lang_id, LANGUAGE_ID
            ),
            data: None,
        },
    }))
}

fn trace_folding_range(duration: Duration, id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Folding range request with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: None,
        },
    })
}

fn trace_dir_unknown(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: Directory \"{}\" is not known.", uri),
            verbose: None,
        },
    })
}
pub fn trace_doc_change(doc: &TextDoc, tree: &Tree, duration: Duration) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Text document \"{}\" was updated to version {} in {:.4} seconds.",
                doc.uri,
                doc.version,
                duration.as_secs_f32()
            ),
            verbose: Some(
                json!({
                    "text": doc.text,
                    "tree": tree.root_node().to_sexp(),
                })
                .to_string(),
            ),
        },
    })
}

fn trace_doc_rename(
    id: NumberOrString,
    renamed: &RenameFileOperations,
    duration: Duration,
) -> Message {
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
                "INFO: File rename request with ID \"{}\" completed in {:.4} seconds.",
                id,
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

fn trace_find_refs(id: NumberOrString, duration: Duration, refs: Option<Vec<Location>>) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Find references request with ID \"{}\" completed in {:.4} seconds.",
                id,
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
                "INFO: Find macro definition references sync with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: Some(json!(file).to_string()),
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
                "INFO: Find subscript macro references sync with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: Some(json!(file).to_string()),
        },
    })
}

fn trace_find_ext_definitions_for_macro_ref_sync(
    duration: Duration,
    id: NumberOrString,
    file: Uri,
) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Find external definitions for macro reference sync with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: Some(json!(file).to_string()),
        },
    })
}

fn trace_goto_def(duration: Duration, id: NumberOrString, defs: Option<LocationResult>) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Go to definition request with ID {} completed in {:.4} seconds.",
                id,
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

fn trace_sem_tokens_full(duration: Duration, id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Semantic tokens for whole file request with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: None,
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

fn trace_sem_tokens_range(duration: Duration, id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Semantic tokens for file range request with ID {} completed in {:.4} seconds.",
                id,
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

fn trace_window_workdone(duration: Duration, id: NumberOrString, aborted: bool) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Window workdone progress with ID {} {} {:.4} seconds.",
                id,
                if aborted {
                    "aborted after"
                } else {
                    "completed in"
                },
                duration.as_secs_f32()
            ),
            verbose: None,
        },
    })
}

fn trace_workspace_discovery_sync(
    duration: Duration,
    id: NumberOrString,
    details: Option<String>,
) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Workspace discovery sync with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: details,
        },
    })
}
fn trace_workspace_indexed(
    duration: Duration,
    id: NumberOrString,
    workspace: &Workspace,
) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Workspace discovery with ID {} completed in {:.4} seconds.",
                id,
                duration.as_secs_f32()
            ),
            verbose: Some(json!(workspace).to_string()),
        },
    })
}
fn warn_response_id_unknown(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "WARNING: Response ID {} is not known. Cannot find request with matching ID.",
                id
            ),
            verbose: None,
        },
    })
}

fn warn_response_already_processed(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "WARNING: Response ID {} is not known. Cannot find request with matching ID.",
                id
            ),
            verbose: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        config::T32DefaultDirs,
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
            T32DefaultDirs::default(),
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
