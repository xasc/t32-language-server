// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::time::{Duration, Instant};

use crate::{
    config::Config,
    proc::{proc_alive, ProcState},
    protocol::{ErrorCodes, InitializeError, InitializeParams, InitializeResult, ResponseError, ServerCapabilities},
    request::{ExitNotification, Request},
    response::ResponseResult,
    transport::StdioChannel,
    ReturnCode,
};

struct InitializationStatus {
    req: Request,
    rc: ReturnCode,
}

struct ProcHeartbeat {
    pid: Option<u32>,
    interval: Duration,
    last_beat: Instant,
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

pub fn serve(mut channel: StdioChannel, mut cfg: Config) -> ReturnCode {
    let heartbeat = ProcHeartbeat::build(&cfg);

    let status = wait_for_initialize_req(&mut channel, heartbeat);
    match status.req {
        Request::InitializeRequest(req) => {
            if let Err(err) = process_initialize_params(&req.params, &mut cfg) {
                channel.send_response_error(Some(req.id), err);
                return ReturnCode::ProtcolErr;
            } else {
                let result = ResponseResult::InitializeResult(InitializeResult::build(
                    ServerCapabilities::build(),
                ));
                channel.send_response(req.id, result);
            }
        }
        // No shutdown request was received before
        Request::ExitNotification(_) => return shutdown(channel, status.rc),
        _ => unreachable!(),
    }

    let heartbeat = ProcHeartbeat::build(&cfg);
    match handle_requests(&mut channel, heartbeat) {
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
        let req = match read_msg(channel, &mut heartbeat) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(rc) => {
                return InitializationStatus {
                    req: Request::ExitNotification(ExitNotification {}),
                    rc,
                }
            }
        };

        match &req {
            Request::InitializeRequest(_) | Request::ExitNotification(_) => {
                return InitializationStatus {
                    req,
                    rc: if shutdown_request_recv {
                        ReturnCode::OkExit
                    } else {
                        ReturnCode::ErrExit
                    },
                }
            }
            r if r.is_request() => {
                if let Request::ShutdownRequest(_) = r {
                    shutdown_request_recv = true;
                }
                channel.send_response_error(
                    Some(req.get_id().expect("Requests must have a ID.")),
                    error_not_initialized(),
                );
            }
            _ => (),
        }
    }
}

fn handle_requests(channel: &mut StdioChannel, mut heartbeat: ProcHeartbeat) -> Result<(), ReturnCode> {
    let mut shutdown_request_recv = false;
    loop {
        let req = match read_msg(channel, &mut heartbeat) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(rc) => return Err(rc),
        };

        match &req {
            Request::ExitNotification(_) => {
                return Err(if shutdown_request_recv {
                    ReturnCode::OkExit
                } else {
                    ReturnCode::ErrExit
                })
            }
            r if r.is_request() => {
                if let Request::ShutdownRequest(_) = r {
                    shutdown_request_recv = true;
                }
            }
            _ => (),
        }
    }
}

fn process_initialize_params(
    params: &InitializeParams,
    cfg: &mut Config,
) -> Result<(), ResponseError> {
    if let Some(pid) = params.process_id {
        let parent_pid = match u32::try_from(pid) {
            Ok(num) => num,
            Err(_) => {
                return Err(ResponseError {
                    code: ErrorCodes::InvalidParams as i64,
                    message: format!(
                        "Error: Process ID of the parent process {} is invalid.",
                        pid
                    ),
                    data: None,
                })
            }
        };

        match cfg.parent_pid {
            Some(ppid) if ppid == parent_pid => (),
            Some(ppid) => return Err(ResponseError {
                code: ErrorCodes::InvalidParams as i64,
                message: format!(
                    "Error: Process ID of the parent process {} is different from the process ID specified by \"--clientProcessId=\" {}.",
                    parent_pid, ppid
                ),
                data: Some(serde_json::to_value(InitializeError { retry: true }).expect("Must convert to value.")),
            }),
            None => {
                cfg.parent_pid = Some(parent_pid);
            }
        }
    }

    Ok(())
}

fn read_msg(
    channel: &mut StdioChannel,
    heartbeat: &mut ProcHeartbeat,
) -> Result<Option<Request>, ReturnCode> {
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
            channel.send_response_error(None, err);
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

/// We ignore the specification here when it states that the exit code 1 should
/// be returned if no shutdown request has been received. Instead, we try to return
/// a meaningful error code.
fn shutdown(channel: StdioChannel, rc: ReturnCode) -> ReturnCode {
    drop(channel);
    rc
}

fn error_no_initialize_conf() -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidRequest as i64,
        message: "Error: Server still waiting for initialized notification. Cannot handle request."
            .to_string(),
        data: None,
    }
}
