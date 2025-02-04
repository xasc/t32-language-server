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
            DidOpenTextDocumentNotification, ExitNotification, Notification, Request,
            SetTraceNotification,
        },
        response::{ErrorResponse, InitializeResponse, NullResponse, Response},
        tasks::{Task, TaskDone, TaskSystem},
        textdoc::{import_doc, TextDocStatus, TextDocs},
    },
    protocol::{
        DidOpenTextDocumentParams, ErrorCodes, InitializeError, InitializeParams, InitializeResult,
        ResponseError, ServerCapabilities, SetTraceParams,
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

struct State {
    shutdown_request_recv: bool,
    exit_requested: bool,
    heartbeat: ProcHeartbeat,
    tasks: TaskSystem,
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
        Message::Notification(Notification::ExitNotification(_)) => return shutdown(channel, rc),
        _ => unreachable!(),
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
            Ok(Some(r)) => r,
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
        tasks: TaskSystem::build(),
        docs: TextDocs::build(),
    };

    let mut incoming: Vec<Option<Message>> = Vec::new();
    let mut outgoing: Vec<Option<Message>> = Vec::new();
    loop {
        recv_incoming(channel, &mut state.heartbeat, &mut incoming)?;

        schedule_tasks(&mut incoming, &mut state, &mut cfg, &mut outgoing)?;
        recv_completed_tasks(&mut state, &mut outgoing);

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
    state: &mut State,
    cfg: &mut Config,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    for msg in incoming {
        let msg = msg.take().expect("No empty slots in list.");

        match msg {
            // All new requests after a shutdown request was received should
            // be trigger an `InvalidRequest` error.
            m if state.shutdown_request_recv && m.is_request() => {
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
                state
                    .tasks
                    .schedule(Task::TextDocNew(text_document, import_doc))?;
            }
            Message::Notification(Notification::SetTraceNotification(SetTraceNotification {
                params: SetTraceParams { value },
            })) => {
                cfg.trace_level = value;
            }
            Message::Request(Request::ShutdownRequest(req)) => {
                state.shutdown_request_recv = true;
                outgoing.push(Some(Message::Response(Response::NullResponse(
                    NullResponse { id: req.id.clone() },
                ))));
            }
            Message::Notification(Notification::ExitNotification(_)) => {
                state.exit_requested = true;
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

fn recv_completed_tasks(state: &mut State, _outgoing: &mut Vec<Option<Message>>) {
    for done in state.tasks.rx.try_iter() {
        match done {
            TaskDone::TextDocNew(doc, tree) => state.docs.add(doc, tree, TextDocStatus::Open),
        }
    }
}

fn send_outgoing(channel: &mut StdioChannel, msgs: &mut Vec<Option<Message>>) {
    for msg in msgs {
        let msg = msg.take().expect("No empty slots allowed.");
        channel.send_msg(msg);
    }
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

/// We ignore the specification here when it states that the exit code 1 should
/// be returned if no shutdown request has been received. Instead, we try to return
/// a meaningful error code.
fn shutdown(channel: StdioChannel, rc: ReturnCode) -> ReturnCode {
    drop(channel);
    rc
}
