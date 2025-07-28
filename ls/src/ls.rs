// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod doc;
mod language;
mod lsp;
mod mainloop;
mod proc;
mod request;
mod response;
mod tasks;
mod transport;
mod workspace;

use std::time::{Duration, Instant};

use url;

use crate::{
    ReturnCode,
    config::{Config, Workspace},
    ls::lsp::Message,
    ls::transport::StdioChannel,
    ls::{
        doc::TextDocs,
        proc::{ProcState, proc_alive},
        request::{Notification, Request},
        response::{ErrorResponse, InitializeResponse, Response},
        tasks::{OngoingTask, Task, TaskDone, TaskSystem},
    },
    protocol::{
        ErrorCodes, InitializeError, InitializeParams, InitializeResult, LogTraceParams,
        ResponseError, ServerCapabilities, TraceValue,
    },
};

pub use crate::ls::workspace::FileIndex;

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
    completed: Vec<Option<TaskDone>>,
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
        Message::Request(Request::InitializeRequest { id, params }) => {
            if let Err(error) = process_initialize_params(params, &mut cfg) {
                channel.send_msg(Message::Response(Response::ErrorResponse(ErrorResponse {
                    id: Some(id),
                    error,
                })));
                return ReturnCode::ProtcolErr;
            } else {
                let result = InitializeResult::build(ServerCapabilities::build());
                channel.send_msg(Message::Response(Response::InitializeResponse(
                    InitializeResponse { id: id, result },
                )));
            }
        }
        // No shutdown request was received before
        Message::Notification(Notification::ExitNotification {}) => {
            if cfg.trace_level != TraceValue::Off {
                channel.send_msg(Message::Notification(log_notif(
                    &Notification::ExitNotification {},
                )));
            }
            return shutdown(channel, rc);
        }
        _ => unreachable!(),
    }

    if cfg.trace_level != TraceValue::Off {
        channel.send_msg(Message::Notification(notif_initialized()));
    }

    match mainloop::handle_requests(&mut channel, cfg) {
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
                    msg: Message::Notification(Notification::ExitNotification {}),
                    rc,
                };
            }
        };

        match msg {
            Message::Request(Request::InitializeRequest { .. })
            | Message::Notification(Notification::ExitNotification { .. }) => {
                return InitializationStatus {
                    msg,
                    rc: if shutdown_request_recv {
                        ReturnCode::OkExit
                    } else {
                        ReturnCode::ErrExit
                    },
                };
            }
            m if m.is_request() => {
                if let Message::Request(Request::ShutdownRequest { .. }) = m {
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

fn process_initialize_params(
    params: InitializeParams,
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

    // If the key `workspaceFolders` is present in `InitializeParams`, then we
    // can infer that the client must support the `workspace.workspaceFolders`
    // capability. We don't need to check for it separately.
    if params.workspace_folders.is_some() {
        cfg.workspace = Workspace::Folders(params.workspace_folders);
    } else if params.root_uri.is_some() {
        debug_assert!(url::Url::parse(params.root_uri.as_ref().unwrap()).is_ok());
        cfg.workspace = Workspace::Root(params.root_uri);
    } else if params.root_path.is_some() {
        // This is not guaranteed to be an URI, so we try to convert it into
        // one.
        let dir = match url::Url::from_directory_path(params.root_path.as_ref().unwrap()) {
            Ok(url) => url.to_string(),
            Err(_) => params.root_path.unwrap(),
        };
        cfg.workspace = Workspace::Root(Some(dir));
    }

    // The workspace folder can be `null` if no folder was selected in the client.
    // It is possible to query the current workspace folder selection with a
    // `workspaceFolders` request. The client capabilities tell us if this is
    // supported.
    cfg.workspace_folders_supported = params
        .capabilities
        .workspace
        .is_some_and(|ws| ws.workspace_folders.unwrap_or(false));

    // Check whether the client support `LocationLink` in the response results.
    cfg.location_links.definitions_supported =
        params.capabilities.text_document.is_some_and(|td| {
            td.definition
                .is_some_and(|def| def.link_support.unwrap_or(false))
        });

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

fn error_not_initialized() -> ResponseError {
    ResponseError {
        code: ErrorCodes::ServerNotInitialized as i64,
        message: "Error: Server not initialized. Cannot handle request.".to_string(),
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
        data: Some(
            serde_json::to_value(InitializeError { retry: true }).expect("Must convert to value."),
        ),
    }
}

fn notif_initialized() -> Notification {
    Notification::LogTraceNotification {
        params: LogTraceParams {
            message: "INFO: Server is initialized.".to_string(),
            verbose: None,
        },
    }
}

fn log_notif(msg: &Notification) -> Notification {
    Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!("INFO: Received notification \"{:}\".", msg),
            verbose: None,
        },
    }
}

/// We ignore the specification here when it states that the exit code 1 should
/// be returned if no shutdown request has been received. Instead, we try to return
/// a meaningful error code.
fn shutdown(channel: StdioChannel, rc: ReturnCode) -> ReturnCode {
    drop(channel);
    rc
}
