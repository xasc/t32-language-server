// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! Task system with one queue per worker and task stealing. Based on
//! this [talk](https://youtu.be/zULU6Hhp42w) from Sean Parent.

use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{
        Arc, Condvar, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::{Builder, JoinHandle, available_parallelism},
};

use tree_sitter::Tree;
use url::Url;

use crate::{
    ReturnCode,
    config::{SemanticTokenEncoding, Workspace},
    ls::{
        doc::{TextDoc, TextDocData},
        language::{
            FindDefintionsForMacroRefResult, FindMacroReferencesResult, FindReferencesResult,
            GotoDefinitionResult, MacroReferenceOrigin,
        },
        tasks::{MacroDefinitionLocation, OngoingTaskHandle, RenameFileOperations},
        workspace::{FileIndex, ResolvedRenameFileOperations, WorkspaceMembers},
    },
    protocol::{
        Location, LocationLink, NumberOrString, Position, Range, SemanticTokens,
        SemanticTokensLegend, TextDocumentContentChangeEvent, TextDocumentItem, Uri,
    },
    t32::{FindMacroRefsLangContext, FindRefsLangContext, GotoDefLangContext, LangExpressions},
};

#[derive(Debug, Clone)]
pub enum Task {
    DidRenameFiles(
        NumberOrString,
        RenameFileOperations,
        FileIndex,
        fn(RenameFileOperations, &mut FileIndex) -> ResolvedRenameFileOperations,
    ),
    FindExternalDefinitionsForMacroRef {
        id: NumberOrString,
        textdoc: TextDocData,
        t32: GotoDefLangContext,
        callers: Vec<Uri>,
        origin: MacroReferenceOrigin,
        callee: Uri,
        find: fn(
            TextDocData,
            GotoDefLangContext,
            Vec<Uri>,
            MacroReferenceOrigin,
            Uri,
        ) -> FindDefintionsForMacroRefResult,
    },
    FindMacroReferencesFromDefinitions {
        id: NumberOrString,
        textdoc: TextDocData,
        t32: FindMacroRefsLangContext,
        r#macro: String,
        definitions: Vec<MacroDefinitionLocation>,
        find: fn(
            TextDocData,
            FindMacroRefsLangContext,
            String,
            Vec<MacroDefinitionLocation>,
        ) -> FindMacroReferencesResult,
    },
    FindMacroReferencesInSubscripts {
        id: NumberOrString,
        textdoc: TextDocData,
        t32: FindMacroRefsLangContext,
        r#macro: String,
        find: fn(TextDocData, FindMacroRefsLangContext, String) -> FindMacroReferencesResult,
    },
    FindReferences {
        id: NumberOrString,
        textdoc: TextDocData,
        t32: FindRefsLangContext,
        position: Position,

        #[expect(unused)]
        declaration_included: bool,

        find: fn(TextDocData, FindRefsLangContext, Position) -> Option<FindReferencesResult>,
    },
    GoToDefinition(
        NumberOrString,
        TextDocData,
        GotoDefLangContext,
        Position,
        fn(TextDocData, GotoDefLangContext, Position) -> Option<GotoDefinitionResult>,
    ),
    GoToExternalMacroDef {
        id: NumberOrString,
        textdoc: TextDocData,
        t32: GotoDefLangContext,
        callers: Vec<Uri>,
        origin: MacroReferenceOrigin,
        backtrace: Uri,
        find: fn(
            TextDocData,
            GotoDefLangContext,
            Vec<Uri>,
            MacroReferenceOrigin,
            Uri,
        ) -> (Option<GotoDefinitionResult>, Vec<Uri>),
    },
    SemanticTokensFull(
        NumberOrString,
        SemanticTokensLegend,
        SemanticTokenEncoding,
        TextDocData,
        fn(SemanticTokensLegend, SemanticTokenEncoding, TextDocData) -> SemanticTokens,
    ),
    SemanticTokensRange(
        NumberOrString,
        SemanticTokensLegend,
        SemanticTokenEncoding,
        TextDocData,
        Range,
        fn(SemanticTokensLegend, SemanticTokenEncoding, TextDocData, Range) -> SemanticTokens,
    ),
    TextDocNew(
        TextDocumentItem,
        FileIndex,
        fn(TextDocumentItem, FileIndex) -> (TextDoc, Tree, LangExpressions),
    ),
    TextDocEdit(
        TextDocData,
        FileIndex,
        Vec<TextDocumentContentChangeEvent>,
        fn(
            TextDocData,
            FileIndex,
            Vec<TextDocumentContentChangeEvent>,
        ) -> (TextDoc, Tree, LangExpressions),
    ),
    WorkspaceFileDiscovery(
        Workspace,
        &'static [&'static str],
        fn(&Workspace, &[&str]) -> WorkspaceMembers,
    ),
    WorkspaceFileScan(
        Url,
        FileIndex,
        fn(Url, FileIndex) -> Result<(TextDoc, Tree, LangExpressions), Uri>,
    ),
    WorkspaceFileIndexNew(Vec<Url>, fn(Vec<Url>) -> FileIndex),
}

#[derive(Debug)]
pub enum TaskDone {
    DidRenameFiles(NumberOrString, ResolvedRenameFileOperations, FileIndex),
    FindExternalDefinitionsForMacroRefSync(NumberOrString, FindDefintionsForMacroRefResult),
    FindMacroReferences(NumberOrString, Option<Vec<Location>>),
    FindMacroReferencesFromDefinitionsSync(NumberOrString, FindMacroReferencesResult),
    FindMacroReferencesInSubscriptsSync(NumberOrString, FindMacroReferencesResult),
    FindReferences(NumberOrString, Option<FindReferencesResult>),
    GoToDefinition(NumberOrString, Option<GotoDefinitionResult>),
    GoToExternalMacroDef(NumberOrString, Vec<LocationLink>),
    GoToExternalMacroDefSync(NumberOrString, Option<GotoDefinitionResult>, Uri, Vec<Uri>),
    SemanticTokensFull(NumberOrString, SemanticTokens),
    SemanticTokensRange(NumberOrString, SemanticTokens),
    TextDocNew(TextDoc, Tree, LangExpressions),
    TextDocEdit(TextDoc, Tree, LangExpressions),
    WorkspaceFileDiscovery(WorkspaceMembers),
    WorkspaceFileScan(Result<(TextDoc, Tree, LangExpressions), Uri>),
    WorkspaceFileIndexNew(FileIndex),
}

pub struct TaskSystem {
    pub rx: Receiver<TaskDone>,
    queues: Arc<Vec<JobQueue>>,
    threads: Vec<Option<JoinHandle<Result<(), ReturnCode>>>>,
    work: Arc<(AtomicU32, AtomicBool)>,
    signal: Arc<(Condvar, Mutex<usize>)>,
    slot: usize,
}

pub struct JobQueue {
    queue: Mutex<VecDeque<Task>>,
}

impl TaskDone {
    pub fn get_task_handle(&self) -> Option<OngoingTaskHandle> {
        match self {
            TaskDone::DidRenameFiles(id, ..)
            | TaskDone::FindExternalDefinitionsForMacroRefSync(id, ..)
            | TaskDone::FindMacroReferences(id, ..)
            | TaskDone::FindMacroReferencesFromDefinitionsSync(id, ..)
            | TaskDone::FindMacroReferencesInSubscriptsSync(id, ..)
            | TaskDone::FindReferences(id, ..)
            | TaskDone::GoToDefinition(id, ..)
            | TaskDone::GoToExternalMacroDef(id, ..)
            | TaskDone::GoToExternalMacroDefSync(id, ..)
            | TaskDone::SemanticTokensFull(id, ..)
            | TaskDone::SemanticTokensRange(id, ..) => {
                Some(OngoingTaskHandle::Identifier(id.clone()))
            }
            TaskDone::TextDocEdit(doc, ..)
            | TaskDone::TextDocNew(doc, ..)
            | TaskDone::WorkspaceFileScan(Ok((doc, ..))) => {
                Some(OngoingTaskHandle::Uri(doc.uri.clone()))
            }
            TaskDone::WorkspaceFileScan(Err(uri)) => Some(OngoingTaskHandle::Uri(uri.clone())),
            TaskDone::WorkspaceFileDiscovery(..) | TaskDone::WorkspaceFileIndexNew(..) => {
                unreachable!(
                    "Workspace scan tasks are only triggered once after server start. They are cleared manually."
                )
            }
        }
    }
}

impl TaskSystem {
    pub fn build() -> Self {
        let num_workers = {
            let lower_bound = 4;

            match available_parallelism() {
                Ok(num) => usize::from(std::cmp::max(
                    num.saturating_mul(NonZeroUsize::new(8).expect("Number is greater than 0."))
                        .div_ceil(NonZeroUsize::new(10).expect("Number is greater than 0.")),
                    NonZeroUsize::new(lower_bound).expect("Number is greater than 0."),
                )),
                Err(_) => lower_bound,
            }
        };

        let mut queues: Vec<JobQueue> = Vec::with_capacity(num_workers);
        for _ in 0..num_workers {
            queues.push(JobQueue::build());
        }

        let queues: Arc<Vec<JobQueue>> = Arc::new(queues);
        let mut threads: Vec<Option<JoinHandle<Result<(), ReturnCode>>>> =
            Vec::with_capacity(num_workers);

        let signal: Arc<(Condvar, Mutex<usize>)> = Arc::new((Condvar::new(), Mutex::new(0)));
        let work: Arc<(AtomicU32, AtomicBool)> =
            Arc::new((AtomicU32::new(0), AtomicBool::new(true)));

        let (tx, rx) = channel::<TaskDone>();
        let tx = Arc::new(tx);

        for ii in 0..num_workers {
            let s = signal.clone();
            let w = work.clone();
            let q = queues.clone();
            let ch = tx.clone();

            let builder = Builder::new();

            threads.push(Some(
                builder
                    .name(format!("Worker #{}", ii))
                    .spawn(move || Self::run(ii, s, w, q, ch))
                    .expect("Worker thread creation must work."),
            ));
        }

        TaskSystem {
            rx,
            queues,
            threads,
            work,
            signal,
            slot: 0,
        }
    }

    pub fn schedule(&mut self, job: &Task) -> Result<(), ReturnCode> {
        let num_queues = self.queues.len();
        loop {
            for ii in 0..num_queues {
                if self.queues[(self.slot + ii) % num_queues].try_push(job)? {
                    let (num_jobs, ..) = &*self.work;
                    let (enqueued, ..) = &*self.signal;

                    num_jobs.fetch_add(1, Ordering::Relaxed);
                    enqueued.notify_one();

                    self.slot += 1;
                    return Ok(());
                }
            }
            self.slot += 1;
        }
    }

    fn run(
        idx: usize,
        signal: Arc<(Condvar, Mutex<usize>)>,
        work: Arc<(AtomicU32, AtomicBool)>,
        queues: Arc<Vec<JobQueue>>,
        tx: Arc<Sender<TaskDone>>,
    ) -> Result<(), ReturnCode> {
        let (enqueued, lock) = &*signal;
        let (num_jobs, running) = &*work;

        while running.load(Ordering::Relaxed) {
            let _guard = enqueued
                .wait_while(lock.lock().expect("One lock per thread."), |_| {
                    num_jobs.load(Ordering::Relaxed) <= 0 && running.load(Ordering::Relaxed)
                })
                .unwrap();
            drop(_guard);

            loop {
                let num_queues = queues.len();
                for ii in 0..num_queues {
                    if let Some(job) = queues[(idx + ii) % num_queues].try_pop()? {
                        num_jobs.fetch_sub(1, Ordering::Relaxed);

                        let job = Self::execute(job);
                        let _ = tx.send(job);
                        break;
                    }
                }

                if num_jobs.load(Ordering::Relaxed) <= 0 || !running.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
        Ok(())
    }

    fn execute(job: Task) -> TaskDone {
        match job {
            Task::DidRenameFiles(id, renamed, mut file_idx, rename_files) => {
                let operations = rename_files(renamed, &mut file_idx);
                TaskDone::DidRenameFiles(id, operations, file_idx)
            }
            Task::FindExternalDefinitionsForMacroRef {
                id,
                textdoc,
                t32,
                callers,
                origin,
                callee,
                find,
            } => {
                let defs = (find)(textdoc, t32, callers, origin, callee);
                TaskDone::FindExternalDefinitionsForMacroRefSync(id, defs)
            }
            Task::FindMacroReferencesFromDefinitions {
                id,
                textdoc,
                t32,
                r#macro,
                definitions,
                find,
            } => TaskDone::FindMacroReferencesFromDefinitionsSync(
                id,
                find(textdoc, t32, r#macro, definitions),
            ),
            Task::FindMacroReferencesInSubscripts {
                id,
                textdoc,
                t32,
                r#macro,
                find,
            } => TaskDone::FindMacroReferencesInSubscriptsSync(id, find(textdoc, t32, r#macro)),
            Task::FindReferences {
                id,
                textdoc,
                t32,
                position,
                declaration_included: _,
                find,
            } => TaskDone::FindReferences(id, find(textdoc, t32, position)),
            Task::GoToDefinition(id, textdoc, t32, loc, find) => {
                TaskDone::GoToDefinition(id, find(textdoc, t32, loc))
            }
            Task::GoToExternalMacroDef {
                id,
                textdoc,
                t32,
                callers,
                origin,
                backtrace,
                find,
            } => {
                let uri = textdoc.doc.uri.clone();
                let defs = (find)(textdoc, t32, callers, origin, backtrace);

                TaskDone::GoToExternalMacroDefSync(id, defs.0, uri, defs.1)
            }
            Task::SemanticTokensFull(id, legend, encoding, textdoc, tokenize) => {
                TaskDone::SemanticTokensFull(id, tokenize(legend, encoding, textdoc))
            }
            Task::SemanticTokensRange(id, legend, encoding, textdoc, range, tokenize) => {
                TaskDone::SemanticTokensRange(id, tokenize(legend, encoding, textdoc, range))
            }
            Task::TextDocNew(doc, files, transform) => {
                let (doc, tree, t32) = transform(doc, files);
                TaskDone::TextDocNew(doc, tree, t32)
            }
            Task::TextDocEdit(textdoc, files, changes, update) => {
                let (doc, tree, t32) = update(textdoc, files, changes);
                TaskDone::TextDocEdit(doc, tree, t32)
            }
            Task::WorkspaceFileDiscovery(workspace, suffixes, locate) => {
                TaskDone::WorkspaceFileDiscovery(locate(&workspace, suffixes))
            }
            Task::WorkspaceFileScan(uri, files, scan) => {
                TaskDone::WorkspaceFileScan(scan(uri, files))
            }
            Task::WorkspaceFileIndexNew(files, index) => {
                TaskDone::WorkspaceFileIndexNew(index(files))
            }
        }
    }
}

impl Drop for TaskSystem {
    fn drop(&mut self) {
        let (.., running) = &*self.work;
        let (enqueued, ..) = &*self.signal;

        running.store(false, Ordering::Relaxed);
        enqueued.notify_all();

        for (ii, t) in self.threads.iter_mut().enumerate() {
            let status = t
                .take()
                .expect("Worker must exist.")
                .join()
                .expect("Stopping the task queue must not fail.");

            if let Err(rc) = status {
                eprintln!(
                    "ERROR: Task queue #{ii} exited with error code {}.",
                    rc as i32
                );
            }
        }
    }
}

impl JobQueue {
    pub fn build() -> Self {
        JobQueue {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn try_push(&self, job: &Task) -> Result<bool, ReturnCode> {
        let mut queue = match self.queue.try_lock() {
            Ok(q) => q,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => return Err(ReturnCode::UnavailableErr),
        };
        queue.push_back(job.clone());
        Ok(true)
    }

    pub fn try_pop(&self) -> Result<Option<Task>, ReturnCode> {
        let mut queue = match self.queue.try_lock() {
            Ok(q) => q,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err(ReturnCode::UnavailableErr),
        };

        if queue.is_empty() {
            Ok(None)
        } else {
            Ok(queue.pop_front())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::*;

    #[test]
    fn does_not_deadlock() {
        let ts = TaskSystem::build();
        thread::sleep(Duration::from_millis(250));

        drop(ts);
    }
}
