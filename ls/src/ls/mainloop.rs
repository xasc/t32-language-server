// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    config::Config,
    ls::lsp::Message,
    ls::transport::StdioChannel,
    ls::{
        log_notif, read_msg,
        request::{DidOpenTextDocumentNotification, Notification, Request, SetTraceNotification},
        response::{ErrorResponse, NullResponse, Response},
        tasks::{OngoingTask, Task, TaskDone, TaskSystem},
        textdoc::{import_doc, TextDocStatus, TextDocs},
        ProcHeartbeat, State, Tasks,
    },
    protocol::{
        DidOpenTextDocumentParams, ErrorCodes, ResponseError, SetTraceParams, TextDocumentItem,
        TraceValue,
    },
    ReturnCode,
};

pub fn handle_requests(channel: &mut StdioChannel, mut cfg: Config) -> Result<(), ReturnCode> {
    let mut state = State {
        shutdown_request_recv: false,
        exit_requested: false,
        heartbeat: ProcHeartbeat::build(&cfg),
        tasks: Tasks {
            runner: TaskSystem::build(),
            blocked: Vec::new(),
            ongoing: Vec::new(),
        },
        docs: TextDocs::build(),
    };

    let mut incoming: Vec<Option<Message>> = Vec::new();
    let mut outgoing: Vec<Option<Message>> = Vec::new();

    loop {
        recv_incoming(channel, &mut state.heartbeat, &mut incoming)?;
        recv_completed_tasks(&mut state.tasks, &mut state.docs, &mut outgoing);

        schedule_tasks(&mut incoming, &mut state, &mut cfg, &mut outgoing)?;

        send_outgoing(channel, &mut outgoing);

        if state.exit_requested {
            return Err(if state.shutdown_request_recv {
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
                let req = m.get_request();
                outgoing.push(Some(Message::Response(Response::ErrorResponse(
                    ErrorResponse {
                        id: Some(
                            req.get_id()
                                .expect("Every request must have an ID.")
                                .clone(),
                        ),
                        error: error_shutdown_seq(),
                    },
                ))));
            }
            Message::Notification(Notification::DidOpenTextDocumentNotification(
                DidOpenTextDocumentNotification {
                    params: DidOpenTextDocumentParams { text_document },
                },
            )) => {
                try_schedule(
                    &mut g.tasks.runner,
                    Task::TextDocNew(text_document, import_doc),
                    &mut g.tasks.ongoing,
                    &mut g.tasks.blocked,
                )?;
            }
            Message::Notification(Notification::SetTraceNotification(SetTraceNotification {
                params: SetTraceParams { value },
            })) => {
                cfg.trace_level = value;
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
    add_ongoing_task_if_seq_ordered(&job, ongoing);

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
        add_ongoing_task_if_seq_ordered(job, ongoing);
    }
    blocked.retain(|t| task_blocked(t, &ongoing));

    Ok(())
}

fn recv_completed_tasks(ts: &mut Tasks, docs: &mut TextDocs, _outgoing: &mut Vec<Option<Message>>) {
    for done in ts.runner.rx.try_iter() {
        match done {
            TaskDone::TextDocNew(doc, tree) => {
                let idx = ts
                    .ongoing
                    .iter()
                    .position(|t| match t {
                        OngoingTask::TextDocUpdate { uri } => *uri == doc.uri,
                    })
                    .expect("Must be a registered task.");

                ts.ongoing.remove(idx);

                docs.add(doc, tree, TextDocStatus::Open);
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

/// Some requests like document updates can only be processed one at a time.
/// This functions checks whether there is an ongoing tasks that would block
/// the scheduling of the new one.
fn task_blocked(job: &Task, ongoing: &[OngoingTask]) -> bool {
    match job {
        Task::TextDocNew(TextDocumentItem { uri, .. }, ..) => ongoing.iter().any(|o| match o {
            OngoingTask::TextDocUpdate { uri: file } => file == uri,
        }),
    }
}

/// Document updates need to be processed in the order in which they were
/// received. Hence, we need to monitor for which documents we are currently
/// processing an update.
fn add_ongoing_task_if_seq_ordered(job: &Task, ongoing: &mut Vec<OngoingTask>) {
    let t = match job {
        Task::TextDocNew(TextDocumentItem { uri, .. }, ..) => {
            OngoingTask::TextDocUpdate { uri: uri.clone() }
        }
    };
    ongoing.push(t);
}

fn error_shutdown_seq() -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidRequest as i64,
        message: "Error: Server has received shutdown request. Cannot handle request.".to_string(),
        data: None,
    }
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
