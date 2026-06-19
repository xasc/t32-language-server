// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

use crate::{
    ReturnCode,
    config::{T32DefaultDirs, Workspace},
    ls::{
        FileData, Notification, TaskCounter,
        doc::{self, TextDocs},
        lsp::Message,
        tasks::{
            self, OngoingTask, RenameFileOperations, Task, TaskDone, TaskProgress, Tasks,
            WorkDoneProgressPhase, WorkspaceDiscoveryPhase, progress, try_schedule,
        },
        workspace::{FileIndex, WorkspaceMembers, index_files, locate_files, rename_files},
    },
    protocol::{
        FileRename, LogTraceParams, NumberOrString, ProgressParams, ProgressToken, TraceValue, Uri,
        WorkDoneProgressBegin, WorkDoneProgressEnd, WorkDoneProgressReport, WorkDoneProgressValue,
    },
    t32::SUFFIXES,
};

pub fn prepare_workspace_discovery(
    id: NumberOrString,
    workspace: Workspace,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    debug_assert!(ongoing.is_empty());

    ongoing.push(Some(OngoingTask::WorkspaceDiscovery {
        id: id.clone(),
        onset: Instant::now(),
        progress: TaskProgress::new(1).with_limit(1),
        phase: WorkspaceDiscoveryPhase::Scanning(workspace, None),
    }));
}

pub fn progress_workspace_file_discovery(task: &mut Option<OngoingTask>, outgoing: &mut Vec<Task>) {
    let Some(OngoingTask::WorkspaceDiscovery { progress, .. }) = task else {
        unreachable!("Must not called with any other variant.");
    };

    if progress.finished() {
        let Some(OngoingTask::WorkspaceDiscovery {
            id,
            onset,
            phase: WorkspaceDiscoveryPhase::Scanning(_, Some(members)),
            ..
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        *task = Some(OngoingTask::WorkspaceDiscovery {
            id,
            onset,
            progress: TaskProgress::new(1).with_limit(1),
            phase: WorkspaceDiscoveryPhase::Indexing(members, None),
        });
    } else if progress.ready() {
        move_to_file_discovery_stage(task.as_mut().expect("No empty slots allowed."), outgoing);
    }
}

pub fn progress_workspace_file_indexing(
    task: &mut Option<OngoingTask>,
    outgoing: &mut Vec<Task>,
) -> Option<FileIndex> {
    let Some(OngoingTask::WorkspaceDiscovery { progress, .. }) = task else {
        unreachable!("Must not called with any other variant.");
    };

    if progress.finished() {
        let Some(OngoingTask::WorkspaceDiscovery {
            id,
            onset,
            phase: WorkspaceDiscoveryPhase::Indexing(workspace, Some(files)),
            ..
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        *task = Some(OngoingTask::WorkspaceDiscovery {
            id,
            onset,
            progress: TaskProgress::new(workspace.get_num_files()).with_limit(1),
            phase: WorkspaceDiscoveryPhase::Parsing(workspace, files.clone()),
        });
        Some(files)
    } else if progress.ready() {
        move_to_file_indexing_stage(task.as_mut().expect("No empty slots allowed."), outgoing);
        None
    } else {
        None
    }
}
pub fn progress_workspace_file_parsing(
    t32_dirs: &T32DefaultDirs,
    task: &mut Option<OngoingTask>,
    outgoing: &mut Vec<Task>,
    done: &mut Vec<Option<TaskDone>>,
) {
    let Some(OngoingTask::WorkspaceDiscovery { progress, .. }) = task else {
        unreachable!("Must not called with any other variant.");
    };

    if progress.finished() {
        let Some(OngoingTask::WorkspaceDiscovery {
            id,
            phase: WorkspaceDiscoveryPhase::Parsing(..),
            ..
        }) = task
        else {
            unreachable!("Must not be called with any other variant.");
        };
        done.push(Some(TaskDone::WorkspaceFileDiscovery(id.clone())));
    } else if progress.ready() {
        move_to_file_parsing_stage(
            t32_dirs,
            task.as_mut().expect("No empty slots allowed."),
            outgoing,
        );
    }
}

pub fn conclude_workspace_file_parsing_progress(task: &mut Option<OngoingTask>) {
    let Some(OngoingTask::WindowWorkDoneProgress { token, phase, .. }) = task else {
        unreachable!("No other variant allowed.");
    };

    // Operation has completed before any progress could be reported.
    let old: Option<ProgressParams> = match phase {
        WorkDoneProgressPhase::Announced(params)
        | WorkDoneProgressPhase::Initialized(params)
        | WorkDoneProgressPhase::Ready(params) => Some(params.clone()),
        WorkDoneProgressPhase::Reporting { .. } | WorkDoneProgressPhase::Aborted => None,
        WorkDoneProgressPhase::Finished { .. } => {
            unreachable!("Progress reporting cannot be in this phase.")
        }
    };

    let params = finish_progress_workspace_discovery(token.clone());
    *phase = WorkDoneProgressPhase::Finished {
        begin: old,
        end: Some(params),
    };
}

pub fn recv_workspace_file_discovery_sync(
    id: &NumberOrString,
    sync: WorkspaceMembers,
    ongoing: &mut Vec<Option<OngoingTask>>,
    outgoing: &mut Vec<Option<Message>>,
) {
    let idx = tasks::find_ongoing_task_by_id(&id, ongoing).expect("Must be a registered task.");
    let Some(OngoingTask::WorkspaceDiscovery {
        progress,
        phase: WorkspaceDiscoveryPhase::Scanning(_, members),
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
    };

    if sync.missing_roots.len() > 0 {
        outgoing.push(Some(trace_root_invalid(&sync.missing_roots)));
    }
    progress.advance();

    *members = Some(sync);
}

pub fn recv_workspace_file_indexing_sync(
    id: &NumberOrString,
    sync: FileIndex,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = tasks::find_ongoing_task_by_id(&id, ongoing).expect("Must be a registered task.");
    let Some(OngoingTask::WorkspaceDiscovery {
        progress,
        phase: WorkspaceDiscoveryPhase::Indexing(_, files),
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
    };
    progress.advance();

    *files = Some(sync);
}

pub fn recv_workspace_file_parsing_sync(
    trace_level: TraceValue,
    id: &NumberOrString,
    sync: Result<FileData, Uri>,
    docs: &mut TextDocs,
    ongoing: &mut Vec<Option<OngoingTask>>,
    outgoing: &mut Vec<Option<Message>>,
) {
    let (total, completed): (u32, u32) = {
        let idx = tasks::find_ongoing_task_by_id(&id, ongoing).expect("Must be a registered task.");
        let Some(OngoingTask::WorkspaceDiscovery {
            onset,
            progress,
            phase: WorkspaceDiscoveryPhase::Parsing(_, _),
            ..
        }) = &mut ongoing[idx]
        else {
            unreachable!("Must not retrieve any other variant.");
        };

        if trace_level != TraceValue::Off {
            let msg: Message = match &sync {
                Ok((doc, tree, _)) => tasks::trace_doc_change(doc, tree, Instant::now() - *onset),
                Err(uri) => trace_doc_cannot_read(&uri),
            };
            outgoing.push(Some(msg));
        }
        progress.advance();

        (progress.total, progress.completed.value())
    };

    if let Some(idx) = progress::find_workdone_progress_by_id(&id, ongoing) {
        if let Some(OngoingTask::WindowWorkDoneProgress {
            token,
            phase: WorkDoneProgressPhase::Reporting { reported, next },
            ..
        }) = &mut ongoing[idx]
        {
            let params =
                report_progress_workspace_discovery(token.clone(), *reported, total, completed);
            if params.is_some() {
                *next = params;
                *reported = completed / total;
            }
        }
    }

    let Ok((doc, tree, t32)) = sync else {
        return;
    };

    // Do not overwrite data for files that have already been requested by the
    // client.
    if let None = docs.get_doc(&doc.uri) {
        docs.add(doc, tree, t32, doc::TextDocStatus::Closed);
    }
}

pub fn process_files_did_rename_notif(
    tasks: &mut Tasks,
    renamed: Vec<FileRename>,
    files: FileIndex,
) -> Result<(), ReturnCode> {
    let job = Task::DidRenameFiles(
        tasks.counters.tasks_int.next_id(),
        RenameFileOperations::from(renamed),
        files,
        rename_files,
    );
    try_schedule(
        &mut tasks.runner,
        job,
        &mut tasks.ongoing,
        &mut tasks.blocked,
    )
}

pub fn process_rename_files_result(
    changes: &RenameFileOperations,
    new_files: FileIndex,
    docs: &mut TextDocs,
    files: &mut FileIndex,
) {
    if changes.old.len() <= 0 {
        return;
    }
    *files = new_files;
    docs.rename_files(changes);
}

pub fn announce_progress_workspace_discovery(token: ProgressToken) -> ProgressParams {
    ProgressParams {
        token,
        value: WorkDoneProgressValue::Begin(WorkDoneProgressBegin {
            title: "Indexing workspace".to_string(),
            cancellable: Some(false),
            message: None,
            percentage: Some(0),
        }),
    }
}

pub fn report_progress_workspace_discovery(
    token: ProgressToken,
    reported: u32,
    total: u32,
    completed: u32,
) -> Option<ProgressParams> {
    debug_assert!(reported <= 100);

    if total <= 0 {
        return None;
    }

    let percent = completed / total;
    if percent <= reported || percent - reported < 5 {
        return None;
    }

    Some(ProgressParams {
        token,
        value: WorkDoneProgressValue::Report(WorkDoneProgressReport {
            cancellable: Some(false),
            message: Some(format!("{} of {} files completed", completed, total)),
            percentage: Some(percent),
        }),
    })
}

pub fn finish_progress_workspace_discovery(token: ProgressToken) -> ProgressParams {
    ProgressParams {
        token,
        value: WorkDoneProgressValue::End(WorkDoneProgressEnd {
            message: Some("All files have been indexed.".to_string()),
        }),
    }
}

fn move_to_file_discovery_stage(task: &mut OngoingTask, outgoing: &mut Vec<Task>) {
    let OngoingTask::WorkspaceDiscovery {
        id,
        progress,
        phase: WorkspaceDiscoveryPhase::Scanning(workspace, _),
        ..
    } = task
    else {
        unreachable!("Must not be called with any other variant.");
    };
    progress.ack_ready();

    outgoing.push(Task::WorkspaceFileDiscovery(
        id.clone(),
        workspace.clone(),
        &SUFFIXES,
        locate_files,
    ));
}

fn move_to_file_indexing_stage(task: &mut OngoingTask, outgoing: &mut Vec<Task>) {
    let OngoingTask::WorkspaceDiscovery {
        id,
        progress,
        phase: WorkspaceDiscoveryPhase::Indexing(workspace, None),
        ..
    } = task
    else {
        unreachable!("Must not be called with any other variant.");
    };
    progress.ack_ready();

    outgoing.push(Task::WorkspaceFileIndexNew(
        id.clone(),
        workspace.files.clone(),
        index_files,
    ));
}

fn move_to_file_parsing_stage(
    t32_dirs: &T32DefaultDirs,
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
) -> FileIndex {
    let OngoingTask::WorkspaceDiscovery {
        id,
        progress,
        phase: WorkspaceDiscoveryPhase::Parsing(workspace, files),
        ..
    } = task
    else {
        unreachable!("Must not be called with any other variant.");
    };

    progress.ack_ready();

    for uri in workspace.files.iter() {
        outgoing.push(Task::WorkspaceFileScan(
            id.clone(),
            uri.clone(),
            files.clone(),
            t32_dirs.clone(),
            doc::read_doc,
        ));
    }
    files.clone()
}
fn trace_root_invalid(roots: &[Uri]) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "WARNING: Workspace root(s) \"{}\" do not exist.",
                roots.join("\", \"")
            ),
            verbose: None,
        },
    })
}
fn trace_doc_cannot_read(uri: &str) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("WARNING: File \"{}\" could not be read.", uri),
            verbose: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use crate::utils;

    use super::*;

    use std::{env, path};

    use url::Url;

    use crate::ls::tasks;

    fn workspace() -> Workspace {
        let dir = env::current_dir().unwrap().join("tests").join("samples");
        let uri: Url = Url::from_directory_path(path::absolute(dir).unwrap()).unwrap();

        Workspace::Root(Some(uri.to_string()))
    }

    fn workspace_members() -> WorkspaceMembers {
        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("c1.cmm");
        let uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        WorkspaceMembers {
            files: vec![uri],
            missing_roots: Vec::new(),
        }
    }

    fn file() -> FileData {
        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("c.cmm");
        let uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let files = utils::create_file_idx();
        let dirs = T32DefaultDirs::build(None, None);

        doc::read_doc(uri, &files, &dirs).expect("Must not fail.")
    }

    fn missing_file() -> Uri {
        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("missing.cmm");
        Url::from_file_path(path::absolute(file).unwrap())
            .unwrap()
            .to_string()
    }

    #[test]
    fn can_start_file_discovery_stage() {
        let id = NumberOrString::Number(0);
        let mut ongoing: Vec<Option<OngoingTask>> = Vec::new();

        let workspace = workspace();

        prepare_workspace_discovery(id.clone(), workspace.clone(), &mut ongoing);

        let mut ongoing = ongoing[0].take().expect("Must not be empty.");
        let mut outgoing: Vec<Task> = Vec::new();

        move_to_file_discovery_stage(&mut ongoing, &mut outgoing);

        let mut progress = TaskProgress::new(1).with_limit(1);
        progress.ack_ready();

        let onset = ongoing.get_onset().clone();

        assert!(
            ongoing
                == OngoingTask::WorkspaceDiscovery {
                    id,
                    onset,
                    progress,
                    phase: WorkspaceDiscoveryPhase::Scanning(workspace, None),
                }
        );
        assert_eq!(outgoing.len(), 1);
    }

    #[test]
    fn can_progress_file_discovery_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace();
        let sync = workspace_members();

        let mut progress = TaskProgress::new(1).with_limit(1);

        let mut ongoing = vec![Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            phase: WorkspaceDiscoveryPhase::Scanning(workspace.clone(), None),
        })];

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        recv_workspace_file_discovery_sync(&id, sync.clone(), &mut ongoing, &mut outgoing);

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        progress.advance();

        assert_eq!(
            ongoing,
            OngoingTask::WorkspaceDiscovery {
                id,
                onset,
                progress,
                phase: WorkspaceDiscoveryPhase::Scanning(workspace, Some(sync)),
            }
        );

        let OngoingTask::WorkspaceDiscovery { progress, .. } = ongoing else {
            panic!("No other variant allowed.");
        };
        assert!(progress.finished());
    }

    #[test]
    fn can_finish_file_discovery_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace();
        let members = workspace_members();

        let mut ongoing = Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(0),
            phase: WorkspaceDiscoveryPhase::Scanning(workspace.clone(), Some(members.clone())),
        });
        let mut outgoing: Vec<Task> = Vec::new();

        progress_workspace_file_discovery(&mut ongoing, &mut outgoing);

        let ongoing = ongoing.expect("Must not be empty.");

        assert!(
            ongoing
                == OngoingTask::WorkspaceDiscovery {
                    id,
                    onset,
                    progress: TaskProgress::new(1).with_limit(1),
                    phase: WorkspaceDiscoveryPhase::Indexing(members, None),
                }
        );
        assert_eq!(outgoing.len(), 0);
    }

    #[test]
    fn reports_missing_workspace_roots() {
        fn missing_roots() -> WorkspaceMembers {
            let dir = env::current_dir().unwrap().join("tests").join("samples");
            let uri: Url = Url::from_directory_path(path::absolute(dir).unwrap()).unwrap();

            WorkspaceMembers {
                files: Vec::new(),
                missing_roots: vec![uri.to_string()],
            }
        }

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace();

        let mut progress = TaskProgress::new(1).with_limit(1);

        let mut ongoing = vec![Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            phase: WorkspaceDiscoveryPhase::Scanning(workspace.clone(), None),
        })];

        let sync = missing_roots();

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        recv_workspace_file_discovery_sync(&id, sync.clone(), &mut ongoing, &mut outgoing);

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        progress.advance();

        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.iter().any(|m| {
            let Some(Message::Notification(Notification::LogTraceNotification {
                params: LogTraceParams { message, .. },
            })) = m
            else {
                panic!()
            };

            message.contains("WARNING: Workspace root(s)") && message.contains("not exist")
        }));
        assert_eq!(
            ongoing,
            OngoingTask::WorkspaceDiscovery {
                id,
                onset,
                progress,
                phase: WorkspaceDiscoveryPhase::Scanning(workspace, Some(sync)),
            }
        );
    }

    #[test]
    fn can_start_file_indexing_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace_members();

        let mut ongoing = OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset,
            progress: TaskProgress::new(1).with_limit(1),
            phase: WorkspaceDiscoveryPhase::Indexing(workspace.clone(), None),
        };

        let mut outgoing: Vec<Task> = Vec::new();

        move_to_file_indexing_stage(&mut ongoing, &mut outgoing);

        let mut progress = TaskProgress::new(1).with_limit(1);
        progress.ack_ready();

        assert_eq!(outgoing.len(), 1);
        assert!(
            ongoing
                == OngoingTask::WorkspaceDiscovery {
                    id,
                    onset,
                    progress,
                    phase: WorkspaceDiscoveryPhase::Indexing(workspace, None),
                }
        );
    }

    #[test]
    fn can_progress_file_indexing_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut progress = TaskProgress::new(1).with_limit(1);
        let workspace = workspace_members();

        let mut ongoing = vec![Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            phase: WorkspaceDiscoveryPhase::Indexing(workspace.clone(), None),
        })];

        let sync = utils::create_file_idx();

        recv_workspace_file_indexing_sync(&id, sync.clone(), &mut ongoing);

        progress.advance();

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        assert_eq!(
            ongoing,
            OngoingTask::WorkspaceDiscovery {
                id,
                onset,
                progress,
                phase: WorkspaceDiscoveryPhase::Indexing(workspace, Some(sync)),
            }
        );

        let OngoingTask::WorkspaceDiscovery { progress, .. } = ongoing else {
            panic!("No other variant allowed.");
        };
        assert!(progress.finished());
    }

    #[test]
    fn can_finish_file_indexing_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace_members();
        let files = utils::create_file_idx();

        let mut ongoing = Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(0),
            phase: WorkspaceDiscoveryPhase::Indexing(workspace.clone(), Some(files.clone())),
        });
        let mut outgoing: Vec<Task> = Vec::new();

        assert!(
            progress_workspace_file_indexing(&mut ongoing, &mut outgoing)
                .is_some_and(|f| f == files)
        );

        let ongoing = ongoing.expect("Must not be empty.");

        assert!(
            ongoing
                == OngoingTask::WorkspaceDiscovery {
                    id,
                    onset,
                    progress: TaskProgress::new(1).with_limit(1),
                    phase: WorkspaceDiscoveryPhase::Parsing(workspace, files),
                }
        );
        assert_eq!(outgoing.len(), 0);
    }

    #[test]
    fn can_start_file_scanning_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace_members();
        let files = utils::create_file_idx();
        let dirs = T32DefaultDirs::build(None, None);

        let mut ongoing = OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset,
            progress: TaskProgress::new(1).with_limit(1),
            phase: WorkspaceDiscoveryPhase::Parsing(workspace.clone(), files.clone()),
        };

        let mut outgoing: Vec<Task> = Vec::new();

        move_to_file_parsing_stage(&dirs, &mut ongoing, &mut outgoing);

        let mut progress = TaskProgress::new(1).with_limit(1);
        progress.ack_ready();

        assert_eq!(outgoing.len(), 1);
        assert!(
            ongoing
                == OngoingTask::WorkspaceDiscovery {
                    id,
                    onset,
                    progress,
                    phase: WorkspaceDiscoveryPhase::Parsing(workspace, files),
                }
        );
    }

    #[test]
    fn can_progress_file_parsing_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace_members();
        let files = utils::create_file_idx();
        let mut docs = TextDocs::new();

        let mut progress = TaskProgress::new(1).with_limit(1);

        let mut ongoing = vec![Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            phase: WorkspaceDiscoveryPhase::Parsing(workspace.clone(), files.clone()),
        })];
        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let sync = Ok(file());

        recv_workspace_file_parsing_sync(
            TraceValue::Messages,
            &id,
            sync,
            &mut docs,
            &mut ongoing,
            &mut outgoing,
        );

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        progress.advance();

        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.iter().any(|m| {
            let Some(Message::Notification(Notification::LogTraceNotification {
                params: LogTraceParams { message, .. },
            })) = m
            else {
                panic!()
            };

            message.contains("INFO: Text document") && message.contains("updated")
        }));

        assert_eq!(
            ongoing,
            OngoingTask::WorkspaceDiscovery {
                id,
                onset,
                progress,
                phase: WorkspaceDiscoveryPhase::Parsing(workspace, files),
            }
        );

        let OngoingTask::WorkspaceDiscovery { progress, .. } = ongoing else {
            panic!("No other variant allowed.");
        };
        assert!(progress.finished());
    }

    #[test]
    fn reports_file_parse_errors() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace_members();
        let files = utils::create_file_idx();
        let mut docs = TextDocs::new();

        let mut progress = TaskProgress::new(1).with_limit(1);

        let mut ongoing = vec![Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            phase: WorkspaceDiscoveryPhase::Parsing(workspace.clone(), files.clone()),
        })];
        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let sync = Err(missing_file());

        recv_workspace_file_parsing_sync(
            TraceValue::Messages,
            &id,
            sync,
            &mut docs,
            &mut ongoing,
            &mut outgoing,
        );

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        progress.advance();

        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.iter().any(|m| {
            let Some(Message::Notification(Notification::LogTraceNotification {
                params: LogTraceParams { message, .. },
            })) = m
            else {
                panic!()
            };

            message.contains("WARNING: File") && message.contains("could not be read")
        }));

        assert_eq!(
            ongoing,
            OngoingTask::WorkspaceDiscovery {
                id,
                onset,
                progress,
                phase: WorkspaceDiscoveryPhase::Parsing(workspace, files),
            }
        );

        let OngoingTask::WorkspaceDiscovery { progress, .. } = ongoing else {
            panic!("No other variant allowed.");
        };
        assert!(progress.finished());
    }

    #[test]
    fn can_finish_file_parsing_stage() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let dirs = T32DefaultDirs::build(None, None);

        let workspace = workspace_members();
        let files = utils::create_file_idx();

        let mut ongoing = Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(0),
            phase: WorkspaceDiscoveryPhase::Parsing(workspace.clone(), files.clone()),
        });
        let mut outgoing: Vec<Task> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        progress_workspace_file_parsing(&dirs, &mut ongoing, &mut outgoing, &mut done);

        assert_eq!(outgoing.len(), 0);
        assert_eq!(done.len(), 1);

        let done = done[0].take().expect("Must not be empty.");

        assert!(matches!(done, TaskDone::WorkspaceFileDiscovery(_)));
        assert_eq!(
            done.get_task_handle().expect("Must not be empty."),
            tasks::OngoingTaskHandle::Identifier(id)
        );
    }

    #[test]
    fn does_not_overwrite_doc_with_stale_data() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let workspace = workspace_members();
        let files = utils::create_file_idx();
        let mut docs = TextDocs::new();

        let mut progress = TaskProgress::new(1).with_limit(1);

        let mut ongoing = vec![Some(OngoingTask::WorkspaceDiscovery {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            phase: WorkspaceDiscoveryPhase::Parsing(workspace.clone(), files.clone()),
        })];
        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let mut doc = file();
        doc.0.version = 1;
        docs.add(doc.0.clone(), doc.1, doc.2, doc::TextDocStatus::Open);

        let sync = Ok(file());

        recv_workspace_file_parsing_sync(
            TraceValue::Messages,
            &id,
            sync,
            &mut docs,
            &mut ongoing,
            &mut outgoing,
        );

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        progress.advance();

        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.iter().any(|m| {
            let Some(Message::Notification(Notification::LogTraceNotification {
                params: LogTraceParams { message, .. },
            })) = m
            else {
                panic!()
            };

            message.contains("INFO: Text document") && message.contains("updated")
        }));

        assert_eq!(
            ongoing,
            OngoingTask::WorkspaceDiscovery {
                id,
                onset,
                progress,
                phase: WorkspaceDiscoveryPhase::Parsing(workspace, files),
            }
        );

        let OngoingTask::WorkspaceDiscovery { progress, .. } = ongoing else {
            panic!("No other variant allowed.");
        };
        assert!(progress.finished());

        assert!(docs.get_doc(&doc.0.uri).is_some_and(|d| d.version == 1));
    }
}
