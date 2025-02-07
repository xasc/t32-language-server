// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod lsp;
mod proc;
mod request;
mod response;
mod tasks;
mod textdoc;
mod transport;

use std::time::{Duration, Instant};

use crate::{
    config::Config,
    ls::lsp::Message,
    ls::transport::StdioChannel,
    ls::{
        proc::{proc_alive, ProcState},
        request::{
            DidOpenTextDocumentNotification, ExitNotification, LogTraceNotification, Notification,
            Request, SetTraceNotification,
        },
        response::{ErrorResponse, InitializeResponse, NullResponse, Response},
        tasks::{OngoingTask, Task, TaskDone, TaskSystem},
        textdoc::{import_doc, TextDoc, TextDocStatus, TextDocs},
    },
    protocol::{
        DidOpenTextDocumentParams, ErrorCodes, InitializeError, InitializeParams, InitializeResult,
        LogTraceParams, ResponseError, ServerCapabilities, SetTraceParams, TextDocumentItem,
        TraceValue,
    },
    ReturnCode,
};

struct InitializationStatus {
    msg: Message,
    rc: ReturnCode,
}

struct ProcHeartbeat {
    pid: Option<u32>,
    interval: Duration,
    last_beat: Instant,
}

struct Tasks {
    runner: TaskSystem,
    blocked: Vec<Task>,
    ongoing: Vec<OngoingTask>,
}

struct State {
    shutdown_request_recv: bool,
    exit_requested: bool,
    heartbeat: ProcHeartbeat,
    tasks: Tasks,
    docs: TextDocs,
}

impl ProcHeartbeat {
    fn build(cfg: &Config) -> Self {
        ProcHeartbeat {
            pid: cfg.parent_pid,
            interval: cfg.pid_check_interval,
            last_beat: Instant::now() - cfg.pid_check_interval,
        }
    }

    fn elapsed(&self, now: &Instant) -> bool {
        *now - self.last_beat >= self.interval
    }

    fn check(&mut self, now: &Instant) -> bool {
        self.last_beat = *now;
        ProcState::Alive == proc_alive(self.pid.expect("PID must be specified."))
    }
}

pub fn serve(mut cfg: Config) -> ReturnCode {
    let mut channel = match transport::build_channel(&cfg) {
        Ok(c) => c,
        Err(rc) => return rc,
    };
    let heartbeat = ProcHeartbeat::build(&cfg);

    let InitializationStatus { msg, rc } = wait_for_initialize_req(&mut channel, heartbeat);
    match msg {
        Message::Request(Request::InitializeRequest(req)) => {
            if let Err(error) = process_initialize_params(&req.params, &mut cfg) {
                channel.send_msg(Message::Response(Response::ErrorResponse(ErrorResponse {
                    id: Some(req.id),
                    error,
                })));
                return ReturnCode::ProtcolErr;
            } else {
                let result = InitializeResult::build(ServerCapabilities::build());
                channel.send_msg(Message::Response(Response::InitializeResponse(
                    InitializeResponse { id: req.id, result },
                )));
            }
        }
        // No shutdown request was received before
        Message::Notification(Notification::ExitNotification(n)) => {
            if cfg.trace_level != TraceValue::Off {
                channel.send_msg(Message::Notification(log_notif(
                    &Notification::ExitNotification(n),
                )));
            }
            return shutdown(channel, rc);
        }
        _ => unreachable!(),
    }

    if cfg.trace_level != TraceValue::Off {
        channel.send_msg(Message::Notification(notif_initialized()));
    }

    match handle_requests(&mut channel, cfg) {
        Ok(_) => (),
        Err(rc) => return shutdown(channel, rc),
    }

    shutdown(channel, ReturnCode::OkExit);
    ReturnCode::OkExit
}

/// Wait for initialization request. Returns `ServerNotInitialized` error for
/// other types of `RequestMessage`. Notifications are dropped. The only
/// exception is the exit notification after which the server shuts down.
/// Exit notifications without prior shutdown request result should trigger an
/// error exit code. However, sending a shutdown request without prior
/// initialization will return an error response.
fn wait_for_initialize_req(
    channel: &mut StdioChannel,
    mut heartbeat: ProcHeartbeat,
) -> InitializationStatus {
    let mut shutdown_request_recv = false;
    loop {
        let msg = match read_msg(channel, &mut heartbeat) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(rc) => {
                return InitializationStatus {
                    msg: Message::Notification(Notification::ExitNotification(ExitNotification {})),
                    rc,
                }
            }
        };

        match msg {
            Message::Request(Request::InitializeRequest(_))
            | Message::Notification(Notification::ExitNotification(_)) => {
                return InitializationStatus {
                    msg,
                    rc: if shutdown_request_recv {
                        ReturnCode::OkExit
                    } else {
                        ReturnCode::ErrExit
                    },
                }
            }
            m if m.is_request() => {
                if let Message::Request(Request::ShutdownRequest(_)) = m {
                    shutdown_request_recv = true;
                }
                let req = m.get_request();
                channel.send_msg(Message::Response(Response::ErrorResponse(ErrorResponse {
                    id: Some(
                        req.get_id()
                            .expect("Every request must have an ID.")
                            .clone(),
                    ),
                    error: error_not_initialized(),
                })));
            }
            _ => (),
        }
    }
}

fn handle_requests(channel: &mut StdioChannel, mut cfg: Config) -> Result<(), ReturnCode> {
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

fn process_initialize_params(
    params: &InitializeParams,
    cfg: &mut Config,
) -> Result<(), ResponseError> {
    if let Some(pid) = params.process_id {
        let parent_pid = match u32::try_from(pid) {
            Ok(num) => num,
            Err(_) => {
                return Err(error_invalid_pid(pid));
            }
        };

        match cfg.parent_pid {
            Some(ppid) if ppid == parent_pid => (),
            Some(ppid) => return Err(error_pid_mismatch(parent_pid, ppid)),
            None => {
                cfg.parent_pid = Some(parent_pid);
            }
        }
    }

    if let Some(level) = &params.trace {
        cfg.trace_level = *level;
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

fn read_msg(
    channel: &mut StdioChannel,
    heartbeat: &mut ProcHeartbeat,
) -> Result<Option<Message>, ReturnCode> {
    match channel.recv_msg() {
        Ok(Some(r)) => Ok(Some(r)),
        Ok(None) => {
            // The server should shut down, if it detects that its parent
            // process is not alive anymore. No actual shutdown request was
            // received, so we exit with an error code. We only check if we
            // did not receive any new message from the client.
            if let Some(_) = heartbeat.pid {
                let now = Instant::now();
                if heartbeat.elapsed(&now) && !heartbeat.check(&now) {
                    return Err(ReturnCode::UnavailableErr);
                }
            }
            Ok(None)
        }
        Err(err) => {
            // The message could not be parsed, so we have no request ID to
            // work with.
            channel.send_msg(Message::Response(Response::ErrorResponse(err)));
            Ok(None)
        }
    }
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

fn error_not_initialized() -> ResponseError {
    ResponseError {
        code: ErrorCodes::ServerNotInitialized as i64,
        message: "Error: Server not initialized. Cannot handle request.".to_string(),
        data: None,
    }
}

fn error_shutdown_seq() -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidRequest as i64,
        message: "Error: Server has received shutdown request. Cannot handle request.".to_string(),
        data: None,
    }
}

fn error_invalid_pid(pid: i64) -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidParams as i64,
        message: format!(
            "Error: Process ID of the parent process {} is invalid.",
            pid
        ),
        data: Some(
            serde_json::to_value(InitializeError { retry: true }).expect("Must convert to value."),
        ),
    }
}

fn error_pid_mismatch(pid_msg: u32, pid_cli: u32) -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidParams as i64,
        message: format!(
            "Error: Process ID of the parent process {} is different from the process ID specified by \"--clientProcessId=\" {}.",
            pid_msg, pid_cli
        ),
        data: Some(serde_json::to_value(InitializeError { retry: true }).expect("Must convert to value.")),
    }
}

fn notif_initialized() -> Notification {
    Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: "INFO: Server is initialized.".to_string(),
            verbose: None,
        },
    })
}

fn log_notif(msg: &Notification) -> Notification {
    Notification::LogTraceNotification(LogTraceNotification {
        params: LogTraceParams {
            message: format!("INFO: Received notification \"{:}\".", msg),
            verbose: None,
        },
    })
}

/// We ignore the specification here when it states that the exit code 1 should
/// be returned if no shutdown request has been received. Instead, we try to return
/// a meaningful error code.
fn shutdown(channel: StdioChannel, rc: ReturnCode) -> ReturnCode {
    drop(channel);
    rc
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
