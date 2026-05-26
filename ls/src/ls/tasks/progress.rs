// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::time::Instant;

use crate::{
    ls::{
        Message, Notification, Request, TaskCounter, TaskCounterExternal, TaskCounters, TaskDone,
        tasks::{self, OngoingTask, WorkDoneProgressPhase, workspace},
    },
    protocol::{
        LogTraceParams, NumberOrString, ProgressParams, ProgressToken, TraceValue,
        WorkDoneProgressCreateParams, WorkDoneProgressReport, WorkDoneProgressValue,
    },
};

pub fn initiate_server_workdone_progress(
    trace_level: TraceValue,
    work: NumberOrString,
    counters: &mut TaskCounters,
    ongoing: &mut Vec<Option<OngoingTask>>,
    outgoing: &mut Vec<Option<Message>>,
) {
    let token = counters.progress.next_id();

    let mut tok: Option<ProgressToken> = None;
    if trace_level != TraceValue::Off {
        tok = Some(token.clone());
    }

    let params = workspace::announce_progress_workspace_discovery(token.clone());

    ongoing.push(Some(OngoingTask::WindowWorkDoneProgress {
        id: counters.tasks_ext.next_id(),
        token,
        work,
        onset: Instant::now(),
        phase: WorkDoneProgressPhase::Ready(params),
    }));

    if trace_level != TraceValue::Off {
        outgoing.push(Some(trace_server_work_announced(tok.unwrap())));
    }
}

pub fn cancel_server_workdone_progress(
    trace_level: TraceValue,
    token: ProgressToken,
    ongoing: &mut [Option<OngoingTask>],
    outgoing: &mut Vec<Option<Message>>,
) {
    let slot = find_workdone_progress_by_token(&token, ongoing);
    if let Some(idx) = slot {
        let Some(OngoingTask::WindowWorkDoneProgress { phase, .. }) = &mut ongoing[idx] else {
            unreachable!("No other variant allowed.");
        };
        *phase = WorkDoneProgressPhase::Aborted;
    }

    if trace_level != TraceValue::Off {
        let msg = if slot.is_some() {
            trace_server_work_canceled(token)
        } else {
            warn_server_work_not_found(token)
        };
        outgoing.push(Some(msg));
    }
}

pub fn confirm_server_workdone_progress(
    trace_level: TraceValue,
    job: &mut OngoingTask,
    outgoing: &mut Vec<Option<Message>>,
) {
    let OngoingTask::WindowWorkDoneProgress { id, phase, .. } = job else {
        unreachable!("No other variant allowed.");
    };

    if let WorkDoneProgressPhase::Announced(params) = phase {
        *phase = WorkDoneProgressPhase::Initialized(params.clone());
    } else if trace_level != TraceValue::Off {
        outgoing.push(Some(tasks::warn_response_already_processed(id.clone())));
    }
}

pub fn abort_server_workdone_progress(
    trace_level: TraceValue,
    job: &mut OngoingTask,
    outgoing: &mut Vec<Option<Message>>,
) {
    let OngoingTask::WindowWorkDoneProgress { id, phase, .. } = job else {
        unreachable!("No other variant allowed.");
    };
    *phase = WorkDoneProgressPhase::Aborted;

    if *phase != WorkDoneProgressPhase::Aborted && trace_level != TraceValue::Off {
        outgoing.push(Some(tasks::warn_response_already_processed(id.clone())));
    }
}

pub fn find_workdone_progress_by_token(
    tok: &ProgressToken,
    ongoing: &[Option<OngoingTask>],
) -> Option<usize> {
    ongoing.iter().position(|t| match t {
        Some(OngoingTask::WindowWorkDoneProgress { token, .. }) => token == tok,
        Some(_) => false,
        None => unreachable!("Not empty slots allowed."),
    })
}

pub fn find_workdone_progress_by_id(
    id: &ProgressToken,
    ongoing: &[Option<OngoingTask>],
) -> Option<usize> {
    ongoing.iter().position(|t| match t {
        Some(OngoingTask::WindowWorkDoneProgress { work, .. }) => work == id,
        Some(_) => false,
        None => unreachable!("Not empty slots allowed."),
    })
}

pub fn broadcast_work_done(
    task: &mut Option<OngoingTask>,
    counter: &mut TaskCounterExternal,
    outgoing: &mut Vec<Option<Message>>,
    done: &mut Vec<Option<TaskDone>>,
) {
    let Some(OngoingTask::WindowWorkDoneProgress {
        id, token, phase, ..
    }) = task
    else {
        unreachable!("Must not called with any other variant.");
    };

    match phase {
        WorkDoneProgressPhase::Reporting { reported, next } if next.is_some() => {
            let params = next.take();
            if params.is_none() {
                return;
            }
            let params = params.unwrap();

            let ProgressParams {
                value:
                    WorkDoneProgressValue::Report(WorkDoneProgressReport {
                        percentage: Some(percentage),
                        ..
                    }),
                ..
            } = &params
            else {
                unreachable!("No other variant possible.")
            };
            *reported = *percentage;

            let msg = Message::Notification(Notification::WorkDoneProgressNotification { params });
            outgoing.push(Some(msg));
        }
        WorkDoneProgressPhase::Reporting { .. } => (),
        WorkDoneProgressPhase::Ready(params) => {
            let msg = Message::Request(Request::WindowWorkDoneProgressCreate {
                id: counter.next_id(),
                params: WorkDoneProgressCreateParams {
                    token: token.clone(),
                },
            });
            outgoing.push(Some(msg));

            *phase = WorkDoneProgressPhase::Announced(params.clone());
        }
        WorkDoneProgressPhase::Initialized(params) => {
            let msg = Message::Notification(Notification::WorkDoneProgressNotification {
                params: params.clone(),
            });
            outgoing.push(Some(msg));

            *phase = WorkDoneProgressPhase::Reporting {
                reported: 0,
                next: None,
            };
        }
        WorkDoneProgressPhase::Finished(p) if p.is_some() => {
            let params = p.take().unwrap();

            let msg = Message::Notification(Notification::WorkDoneProgressNotification { params });
            outgoing.push(Some(msg));

            done.push(Some(TaskDone::WindowWorkDoneProgress(id.clone(), false)));
        }
        WorkDoneProgressPhase::Finished(..) | WorkDoneProgressPhase::Announced(..) => (),

        WorkDoneProgressPhase::Aborted => {
            done.push(Some(TaskDone::WindowWorkDoneProgress(id.clone(), true)));

            *phase = WorkDoneProgressPhase::Finished(None);
        }
    }
}
fn trace_server_work_announced(token: ProgressToken) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Server has announced progress reporting with token \"{}\".",
                token
            ),
            verbose: None,
        },
    })
}

fn trace_server_work_canceled(token: ProgressToken) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Client has canceled server progress reporting with token \"{}\".",
                token
            ),
            verbose: None,
        },
    })
}

fn warn_server_work_not_found(token: ProgressToken) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "WARNING: Server cannot cancel progress reporting with token \"{}\". Token not found.",
                token
            ),
            verbose: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use crate::protocol::{
        ProgressParams, WorkDoneProgressBegin, WorkDoneProgressEnd, WorkDoneProgressReport,
        WorkDoneProgressValue,
    };

    use super::*;

    #[test]
    fn can_initiate_workdone_progress() {
        let mut counters = TaskCounters::new();
        counters.tasks_ext.next_id();

        counters.progress.next_id();
        counters.progress.next_id();

        let mut ongoing: Vec<Option<OngoingTask>> = Vec::new();
        let mut outgoing: Vec<Option<Message>> = Vec::new();

        initiate_server_workdone_progress(
            TraceValue::Off,
            NumberOrString::Number(5),
            &mut counters,
            &mut ongoing,
            &mut outgoing,
        );

        assert!(outgoing.is_empty());
        assert_eq!(ongoing.len(), 1);

        let ongoing = ongoing[0].take().expect("Must not be empty.");

        assert_matches!(
            ongoing,
            OngoingTask::WindowWorkDoneProgress {
                id: NumberOrString::Number(1),
                token: ProgressToken::Number(2),
                work: NumberOrString::Number(5),
                phase: WorkDoneProgressPhase::Ready(ProgressParams {
                    value: WorkDoneProgressValue::Begin(WorkDoneProgressBegin { .. }),
                    ..
                }),
                ..
            }
        );
    }

    #[test]
    fn can_announce_workdone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let title = "Ready".to_string();
        let message = "Message".to_string();

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Ready(ProgressParams {
                token: token.clone(),
                value: WorkDoneProgressValue::Begin(WorkDoneProgressBegin {
                    title: title.clone(),
                    cancellable: Some(false),
                    message: Some(message.clone()),
                    percentage: Some(0),
                }),
            }),
        });

        let mut counter = TaskCounterExternal::new();

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert!(done.is_empty());
        assert_eq!(outgoing.len(), 1);

        let outgoing = outgoing[0].take().expect("Must not be empty.");

        assert!(
            outgoing
                == Message::Request(Request::WindowWorkDoneProgressCreate {
                    id: NumberOrString::Number(0),
                    params: WorkDoneProgressCreateParams {
                        token: token.clone()
                    }
                })
        );

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id,
                token: token.clone(),
                work,
                onset,
                phase: WorkDoneProgressPhase::Announced(ProgressParams {
                    token,
                    value: WorkDoneProgressValue::Begin(WorkDoneProgressBegin {
                        title,
                        cancellable: Some(false),
                        message: Some(message),
                        percentage: Some(0)
                    })
                }),
            })
        );
    }

    #[test]
    fn can_confirm_workdone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let title = "Announced".to_string();
        let message = "Message".to_string();

        let params = WorkDoneProgressValue::Begin(WorkDoneProgressBegin {
            title: title.clone(),
            cancellable: Some(false),
            message: Some(message.clone()),
            percentage: Some(0),
        });

        let mut ongoing = OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Announced(ProgressParams {
                token: token.clone(),
                value: params.clone(),
            }),
        };

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        confirm_server_workdone_progress(TraceValue::Off, &mut ongoing, &mut outgoing);

        assert!(outgoing.is_empty());

        assert_eq!(
            ongoing,
            OngoingTask::WindowWorkDoneProgress {
                id,
                token: token.clone(),
                work,
                onset,
                phase: WorkDoneProgressPhase::Initialized(ProgressParams {
                    token,
                    value: params,
                }),
            }
        );
    }

    #[test]
    fn can_begin_workdone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let title = "Initialized".to_string();
        let message = "Message".to_string();

        let params = WorkDoneProgressValue::Begin(WorkDoneProgressBegin {
            title: title.clone(),
            cancellable: Some(false),
            message: Some(message.clone()),
            percentage: Some(0),
        });

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Initialized(ProgressParams {
                token: token.clone(),
                value: params.clone(),
            }),
        });

        let mut counter = TaskCounterExternal::new();

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert!(done.is_empty());
        assert_eq!(outgoing.len(), 1);

        let outgoing = outgoing[0].take().expect("Must not be empty.");

        assert!(
            outgoing
                == Message::Notification(Notification::WorkDoneProgressNotification {
                    params: ProgressParams {
                        token: token.clone(),
                        value: params,
                    }
                })
        );

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id,
                token: token.clone(),
                work,
                onset,
                phase: WorkDoneProgressPhase::Reporting {
                    reported: 0,
                    next: None,
                },
            })
        );
    }

    #[test]
    fn can_report_workdone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let message = "1 / 10".to_string();

        let params = WorkDoneProgressValue::Report(WorkDoneProgressReport {
            cancellable: None,
            message: Some(message.clone()),
            percentage: Some(60),
        });

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset,
            phase: WorkDoneProgressPhase::Reporting {
                reported: 55,
                next: Some(ProgressParams {
                    token: token.clone(),
                    value: params.clone(),
                }),
            },
        });

        let mut counter = TaskCounterExternal::new();

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert!(done.is_empty());
        assert_eq!(outgoing.len(), 1);

        let outgoing = outgoing[0].take().expect("Must not be empty.");

        assert!(
            outgoing
                == Message::Notification(Notification::WorkDoneProgressNotification {
                    params: ProgressParams {
                        token: token.clone(),
                        value: params,
                    }
                })
        );

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id: id.clone(),
                token: token.clone(),
                work: work.clone(),
                onset,
                phase: WorkDoneProgressPhase::Reporting {
                    reported: 60,
                    next: None,
                },
            })
        );

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset,
            phase: WorkDoneProgressPhase::Reporting {
                reported: 60,
                next: None,
            },
        });

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert!(outgoing.is_empty());
        assert!(done.is_empty());

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id,
                token,
                work,
                onset,
                phase: WorkDoneProgressPhase::Reporting {
                    reported: 60,
                    next: None,
                },
            })
        );
    }

    #[test]
    fn can_finish_wordone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let message = "Done".to_string();

        let params = WorkDoneProgressValue::End(WorkDoneProgressEnd {
            message: Some(message.clone()),
        });

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Finished(Some(ProgressParams {
                token: token.clone(),
                value: params.clone(),
            })),
        });

        let mut counter = TaskCounterExternal::new();

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert_eq!(done.len(), 1);
        assert_eq!(outgoing.len(), 1);

        let TaskDone::WindowWorkDoneProgress(identifier, aborted) =
            done[0].take().expect("Must not be empty.")
        else {
            panic!("Must be this variant.");
        };

        assert_eq!(identifier, id);
        assert_eq!(aborted, false);

        let outgoing = outgoing[0].take().expect("Must not be empty.");

        assert!(
            outgoing
                == Message::Notification(Notification::WorkDoneProgressNotification {
                    params: ProgressParams {
                        token: token.clone(),
                        value: params,
                    }
                })
        );

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id: id.clone(),
                token: token.clone(),
                work: work.clone(),
                onset,
                phase: WorkDoneProgressPhase::Finished(None),
            })
        );

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset,
            phase: WorkDoneProgressPhase::Finished(None),
        });

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert!(done.is_empty());
        assert!(outgoing.is_empty());

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id,
                token,
                work,
                onset,
                phase: WorkDoneProgressPhase::Finished(None),
            })
        );
    }

    #[test]
    fn can_abort_workdone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let message = "Aborted".to_string();

        let params = WorkDoneProgressValue::Report(WorkDoneProgressReport {
            cancellable: Some(false),
            message: Some(message.clone()),
            percentage: Some(31),
        });

        let mut ongoing = OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Finished(Some(ProgressParams {
                token: token.clone(),
                value: params.clone(),
            })),
        };

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        abort_server_workdone_progress(TraceValue::Off, &mut ongoing, &mut outgoing);

        assert!(outgoing.is_empty());

        assert_eq!(
            ongoing,
            OngoingTask::WindowWorkDoneProgress {
                id: id.clone(),
                token: token.clone(),
                work: work.clone(),
                onset,
                phase: WorkDoneProgressPhase::Aborted,
            }
        );

        let mut ongoing = Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Aborted,
        });

        let mut counter = TaskCounterExternal::new();

        let mut outgoing: Vec<Option<Message>> = Vec::new();
        let mut done: Vec<Option<TaskDone>> = Vec::new();

        broadcast_work_done(&mut ongoing, &mut counter, &mut outgoing, &mut done);

        assert!(outgoing.is_empty());

        let TaskDone::WindowWorkDoneProgress(identifier, aborted) =
            done[0].take().expect("Must not be empty.")
        else {
            panic!("Must be this variant.");
        };

        assert_eq!(identifier, id);
        assert_eq!(aborted, true);

        assert_eq!(
            ongoing,
            Some(OngoingTask::WindowWorkDoneProgress {
                id,
                token,
                work,
                onset,
                phase: WorkDoneProgressPhase::Finished(None),
            })
        );
    }

    #[test]
    fn can_cancel_workdone_progress() {
        let id = NumberOrString::Number(2);
        let token = NumberOrString::Number(1);
        let work = NumberOrString::Number(3);
        let onset = Instant::now();
        let title = "Cancelled".to_string();
        let message = "Message".to_string();

        let params = WorkDoneProgressValue::Begin(WorkDoneProgressBegin {
            title: title.clone(),
            cancellable: Some(false),
            message: Some(message.clone()),
            percentage: Some(0),
        });

        let mut ongoing = vec![Some(OngoingTask::WindowWorkDoneProgress {
            id: id.clone(),
            token: token.clone(),
            work: work.clone(),
            onset: onset.clone(),
            phase: WorkDoneProgressPhase::Announced(ProgressParams {
                token: token.clone(),
                value: params.clone(),
            }),
        })];

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        cancel_server_workdone_progress(
            TraceValue::Off,
            token.clone(),
            &mut ongoing,
            &mut outgoing,
        );

        assert!(outgoing.is_empty());

        assert!(ongoing.iter().any(|o| *o
            == Some(OngoingTask::WindowWorkDoneProgress {
                id: id.clone(),
                token: token.clone(),
                work: work.clone(),
                onset,
                phase: WorkDoneProgressPhase::Aborted,
            })));
    }
}
