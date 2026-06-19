// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

use crate::{
    ReturnCode,
    config::{Config, Workspace},
    ls::{
        self, FileIndex, InitState, Message, TaskCounter, TaskCounters, TaskSystem,
        response::NullResponse,
        tasks::{
            self, Notification, OngoingTask, Request, Response, Task, TaskDone, Tasks,
            WorkspaceDiscoveryPhase, progress, workspace,
        },
    },
    protocol::{SetTraceParams, TraceValue, WorkDoneProgressCancelParams},
};

pub fn try_schedule(ts: &mut TaskSystem, job: Task) -> Result<(), ReturnCode> {
    ts.schedule(&job)
}

pub fn discover_files(
    cfg: &Config,
    workspace: Workspace,
    counters: &mut TaskCounters,
    ongoing: &mut Vec<Option<OngoingTask>>,
    outgoing: &mut Vec<Option<Message>>,
) {
    debug_assert!(ongoing.is_empty());
    debug_assert!(outgoing.is_empty());

    let work = counters.tasks_int.next_id();

    workspace::prepare_workspace_discovery(work.clone(), workspace, ongoing);

    if cfg.server_progress_supported {
        progress::initiate_server_workdone_progress(
            cfg.trace_level,
            work,
            counters,
            ongoing,
            outgoing,
        );
    }
    debug_assert!(!ongoing.is_empty());
}
pub fn process_completed_task(
    done: TaskDone,
    cfg: &Config,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) {
    match done {
        TaskDone::WorkspaceFileDiscoverySync(id, members) => {
            tasks::workspace::recv_workspace_file_discovery_sync(
                &id,
                members,
                &mut ts.ongoing,
                outgoing,
            );

            if cfg.trace_level != TraceValue::Off {
                let onset = tasks::get_task_onset_by_id(&id, &ts.ongoing);
                outgoing.push(Some(tasks::trace_workspace_discovery_sync(
                    Instant::now() - *onset,
                    id,
                    Some("Workspace has been scanned for files.".to_string()),
                )));
            }
        }
        TaskDone::WorkspaceFileIndexSync(id, files) => {
            tasks::workspace::recv_workspace_file_indexing_sync(&id, files, &mut ts.ongoing);

            if cfg.trace_level != TraceValue::Off {
                let onset = tasks::get_task_onset_by_id(&id, &ts.ongoing);
                outgoing.push(Some(tasks::trace_workspace_discovery_sync(
                    Instant::now() - *onset,
                    id,
                    Some("File index for workspace has been created.".to_string()),
                )));
            }
        }
        TaskDone::CodeFolds(..)
        | TaskDone::DidRenameFiles(..)
        | TaskDone::FindExternalDefinitionsForMacroRefSync(..)
        | TaskDone::FindMacroReferences(..)
        | TaskDone::FindMacroReferencesFromDefinitionsSync(..)
        | TaskDone::FindMacroReferencesInSubscriptsSync(..)
        | TaskDone::FindReferences(..)
        | TaskDone::GoToDefinition(..)
        | TaskDone::GoToExternalMacroDef(..)
        | TaskDone::GoToExternalMacroDefSync(..)
        | TaskDone::SemanticTokensFull(..)
        | TaskDone::SemanticTokensRange(..)
        | TaskDone::TextDocNew(..)
        | TaskDone::TextDocEdit(..)
        | TaskDone::WindowWorkDoneProgress(..)
        | TaskDone::WorkspaceFileDiscovery(..)
        | TaskDone::WorkspaceFileParseSync(..) => {
            unreachable!("These tasks are not processed while the server is booting.")
        }
    }
}

pub fn progress_multi_part_tasks(
    cfg: &Config,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<Option<FileIndex>, ReturnCode> {
    let mut files: Option<FileIndex> = None;
    let mut tasks: Vec<Task> = Vec::new();

    for job in ts.ongoing.iter_mut() {
        let Some(task) = job else {
            unreachable!("No empty slots allowed.")
        };

        if cfg.trace_level != TraceValue::Off && task.aborted() {
            outgoing.push(Some(tasks::trace_task_aborted(
                Instant::now() - *task.get_onset(),
                task.get_id(),
            )));
        }

        match task {
            OngoingTask::WorkspaceDiscovery { phase, .. } => match phase {
                WorkspaceDiscoveryPhase::Scanning(..) => {
                    workspace::progress_workspace_file_discovery(job, &mut tasks);
                }
                WorkspaceDiscoveryPhase::Indexing(..) => {
                    files = workspace::progress_workspace_file_indexing(job, &mut tasks);
                }
                WorkspaceDiscoveryPhase::Parsing(..) => {
                    unreachable!("Task is not schedule while the server is booting.")
                }
            },
            OngoingTask::WindowWorkDoneProgress { .. } => {
                progress::broadcast_work_done(cfg.trace_level, job, outgoing, &mut ts.completed);
            }
            OngoingTask::CodeFolds(..)
            | OngoingTask::DidRenameFiles(..)
            | OngoingTask::FindMacroReferences { .. }
            | OngoingTask::FindReferences(..)
            | OngoingTask::GoToExternalMacroDef { .. }
            | OngoingTask::GoToDefinition(..)
            | OngoingTask::SemanticTokensFull(..)
            | OngoingTask::SemanticTokensRange(..)
            | OngoingTask::TextDocUpdate { .. } => {
                unreachable!("Cannot be queued for execution.")
            }
        }
    }

    for job in tasks {
        try_schedule(&mut ts.runner, job)?;
    }
    Ok(files)
}

pub fn process_msg(
    msg: Message,
    g: &mut InitState,
    cfg: &mut Config,
    outgoing: &mut Vec<Option<Message>>,
    postponed: &mut Vec<Option<Message>>,
) {
    match msg {
        // All new requests after a shutdown request was received should
        // be trigger an `InvalidRequest` error.
        m if g.shutdown_request_recv && m.is_request() => {
            outgoing.push(Some(ls::error_shutdown_seq(
                m.get_request().get_id().clone(),
            )));
        }
        Message::Notification(Notification::DidChangeTextDocumentNotification { .. })
        | Message::Notification(Notification::DidCloseTextDocumentNotification { .. })
        | Message::Notification(Notification::DidOpenTextDocumentNotification { .. })
        | Message::Notification(Notification::DidRenameFilesNotification { .. })
        | Message::Request(Request::FindReferences { .. })
        | Message::Request(Request::FoldingRange { .. })
        | Message::Request(Request::GoToDefinition { .. })
        | Message::Request(Request::SemanticTokensFull { .. })
        | Message::Request(Request::SemanticTokensRange { .. }) => {
            postponed.push(Some(msg));
        }
        Message::Notification(Notification::SetTraceNotification {
            params: SetTraceParams { value },
        }) => {
            cfg.trace_level = value;
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
}
