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
        language::{
            FindDefintionsForMacroRefResult, MacroDefinitionLocation, MacroReferenceOrigin,
        },
        log_notif,
        lsp::Message,
        mainloop::{trace_doc_cannot_read, trace_doc_change},
        request::{Notification, Request},
        response::{
            ErrorResponse, FindReferencesResponse, GoToDefinitionResponse, LocationResult,
            NullResponse, Response, SemanticTokensFullResponse, SemanticTokensRangeResponse,
        },
        tasks::{
            docsync::{process_doc_change_notif, process_doc_close_notif, process_doc_open_notif},
            lang::{
                process_find_references_req, process_find_references_result,
                process_goto_definition_req, process_goto_definition_result,
                process_semantic_tokens_full_req, process_semantic_tokens_range_req,
                progress_find_external_macro_definitions, progress_find_macro_def_references,
                progress_find_subscript_macro_refs, progress_goto_external_macro_def,
                recv_find_external_definitions_for_macro_reference_sync,
                recv_find_macro_def_references_sync, recv_find_subscript_macro_references_sync,
                recv_goto_external_macro_def_sync,
            },
            workspace::{process_files_did_rename_notif, process_rename_files_result},
        },
        workspace::{FileIndex, ResolvedRenameFileOperations},
    },
    protocol::{
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, ErrorCodes, FileRename, Location,
        LocationLink, LogTraceParams, NumberOrString, ResponseError, SetTraceParams,
        TextDocumentItem, TraceValue, Uri,
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
}

pub enum OngoingTaskHandle {
    Identifier(NumberOrString),
    Uri(Uri),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProgressCounter {
    Ready,
    Counting(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskProgress {
    completed: ProgressCounter,
    pub total: u32,
    cycles: u32,
    max_cycles: u32,
}

/// To find macro defintions in other files, we need to check for the presence
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
    fn get_id(&self) -> &NumberOrString {
        match self {
            OngoingTask::DidRenameFiles(id, ..)
            | OngoingTask::FindMacroReferences { id, .. }
            | OngoingTask::FindReferences(id, ..)
            | OngoingTask::GoToExternalMacroDef { id, .. }
            | OngoingTask::GoToDefinition(id, ..)
            | OngoingTask::SemanticTokensFull(id, ..)
            | OngoingTask::SemanticTokensRange(id, ..) => id,
            OngoingTask::TextDocUpdate { .. } => unreachable!("Other types have not ID field."),
        }
    }

    fn get_onset(&self) -> &Instant {
        match self {
            OngoingTask::DidRenameFiles(_, onset)
            | OngoingTask::FindMacroReferences { onset, .. }
            | OngoingTask::FindReferences(_, onset)
            | OngoingTask::GoToExternalMacroDef { onset, .. }
            | OngoingTask::GoToDefinition(.., onset)
            | OngoingTask::SemanticTokensFull(_, onset)
            | OngoingTask::SemanticTokensRange(_, onset)
            | OngoingTask::TextDocUpdate { onset, .. } => onset,
        }
    }

    fn aborted(&self) -> bool {
        match self {
            OngoingTask::FindMacroReferences { progress, .. }
            | OngoingTask::GoToExternalMacroDef { progress, .. } => progress.aborted(),
            OngoingTask::DidRenameFiles(..)
            | OngoingTask::GoToDefinition(..)
            | OngoingTask::FindReferences(..)
            | OngoingTask::SemanticTokensFull { .. }
            | OngoingTask::SemanticTokensRange { .. }
            | OngoingTask::TextDocUpdate { .. } => false,
        }
    }
}

impl ProgressCounter {
    #[expect(unused)]
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

    #[expect(unused)]
    pub fn with_limit(progress: Self, max_cycles: u32) -> Self {
        TaskProgress {
            max_cycles,
            ..progress
        }
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

    pub fn abort(&mut self) {
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
        let finished = process_completed_task(done, cfg, ts, docs, files, outgoing)?;

        if finished && let Some(handle) = handle {
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
) -> Result<bool, ReturnCode> {
    match done {
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
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);

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
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);

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
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
            if let Some(resp) = process_find_references_result(
                docs,
                files,
                &id,
                result,
                cfg.trace_level,
                ts,
                outgoing,
            ) {
                if cfg.trace_level != TraceValue::Off {
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
                    let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
            Ok(false)
        }
        TaskDone::SemanticTokensFull(id, tokens) => {
            if cfg.trace_level != TraceValue::Off {
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
                let idx = find_ongoing_task_by_id(&id, &ts.ongoing);
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
        TaskDone::WorkspaceFileScan(res) => {
            match res {
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
            }
            Ok(true)
        }
        TaskDone::WorkspaceFileDiscovery(_) | TaskDone::WorkspaceFileIndexNew(_) => {
            unreachable!("Workspace file scan tasks are only executed once after server start.")
        }
    }
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
        Task::DidRenameFiles(_, RenameFileOperations { old, .. }, ..) => {
            // Continue with rename operations even though other requests that
            // query file data are ongoing.
            ongoing.iter().any(|o| match o {
                Some(task) => match task {
                    OngoingTask::DidRenameFiles(..) => true,
                    OngoingTask::TextDocUpdate { uri: file, .. } => old.contains(&file),
                    OngoingTask::FindMacroReferences { .. }
                    | OngoingTask::FindReferences(..)
                    | OngoingTask::GoToDefinition(..)
                    | OngoingTask::GoToExternalMacroDef { .. }
                    | OngoingTask::SemanticTokensFull { .. }
                    | OngoingTask::SemanticTokensRange { .. } => false,
                },
                None => unreachable!("Not empty slots allowed."),
            })
        }
        Task::FindExternalDefinitionsForMacroRef {
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
            // renaming and file data updates have completed.
            Some(task) => match task {
                OngoingTask::DidRenameFiles(..) => true,
                OngoingTask::TextDocUpdate { uri: file, .. } => file == uri,
                OngoingTask::FindMacroReferences { .. }
                | OngoingTask::FindReferences(..)
                | OngoingTask::GoToDefinition(..)
                | OngoingTask::GoToExternalMacroDef { .. }
                | OngoingTask::SemanticTokensFull { .. }
                | OngoingTask::SemanticTokensRange { .. } => false,
            },
            None => unreachable!("Not empty slots allowed."),
        }),
        Task::WorkspaceFileScan(url, ..) => ongoing.iter().any(|o| match o {
            // Delay workspace scan only until file renaming and file data updates have completed.
            Some(task) => match task {
                OngoingTask::DidRenameFiles(..) => true,
                OngoingTask::TextDocUpdate { uri: file, .. } => file == url.as_str(),
                OngoingTask::FindMacroReferences { .. }
                | OngoingTask::FindReferences(..)
                | OngoingTask::GoToDefinition(..)
                | OngoingTask::GoToExternalMacroDef { .. }
                | OngoingTask::SemanticTokensFull { .. }
                | OngoingTask::SemanticTokensRange { .. } => false,
            },
            None => unreachable!("Not empty slots allowed."),
        }),
        // Complete these requests as fast as possible.
        Task::WorkspaceFileDiscovery(..) | Task::WorkspaceFileIndexNew(..) => false,
    }
}

/// Document updates need to be processed in the order in which they were
/// received. Hence, we need to monitor for which documents we are currently
/// processing an update.
/// No status tracking for multi-stage tasks is performed.
fn add_task_status_tracking(job: &Task, ongoing: &mut Vec<Option<OngoingTask>>) {
    let t = match job {
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
        Task::WorkspaceFileScan(url, ..) => OngoingTask::TextDocUpdate {
            uri: url.to_string(),
            onset: Instant::now(),
        },

        Task::FindExternalDefinitionsForMacroRef { .. }
        | Task::FindMacroReferencesFromDefinitions { .. }
        | Task::FindMacroReferencesInSubscripts { .. }
        | Task::GoToExternalMacroDef { .. }
        | Task::WorkspaceFileDiscovery(..)
        | Task::WorkspaceFileIndexNew(..) => return,
        //_ => return,
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
            Some(OngoingTask::DidRenameFiles(id, ..))
            | Some(OngoingTask::FindReferences(id, ..))
            | Some(OngoingTask::FindMacroReferences { id, .. })
            | Some(OngoingTask::GoToDefinition(id, ..))
            | Some(OngoingTask::GoToExternalMacroDef { id, .. })
            | Some(OngoingTask::SemanticTokensFull(id, _))
            | Some(OngoingTask::SemanticTokensRange(id, _)) => id == identifier,
            None | Some(OngoingTask::TextDocUpdate { .. }) => {
                unreachable!("No other tasks can by selected by id.")
            }
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
                "ERROR: Language ID \"{}\" is not supported for text documents. The only supported language ID is \"{}\".",
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
            message: "ERROR: Server has received shutdown request. Cannot handle request."
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
