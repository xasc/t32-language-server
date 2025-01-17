// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    config::Config,
    proc::{proc_alive, ProcState},
    protocol::{ErrorCodes, InitializeParams, InitializeResult, ResponseError, ServerCapabilities},
    request::{ExitNotification, Request},
    response::ResponseResult,
    transport::StdioChannel,
    ReturnCode,
};

struct InitializationStatus {
    req: Request,
    rc: ReturnCode,
}

pub fn serve(mut channel: StdioChannel, mut cfg: Config) -> ReturnCode {
    let status = wait_for_initialize_req(&mut channel, cfg.parent_pid);
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

    match wait_for_initialized_notif(&mut channel, cfg.parent_pid) {
        Ok(_) => (),
        Err(rc) => return shutdown(channel, rc),
    }

    drop(channel);
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
    parent_pid: Option<u32>,
) -> InitializationStatus {
    let mut shutdown_request_recv = false;
    loop {
        let req = match channel.recv_msg() {
            Ok(Some(r)) => r,
            Ok(None) => {
                // The server should shut down, if it detects that its parent
                // process is not alive anymore. No actual shutdown request was
                // received, so we exit with an error code. We only check if we
                // did not receive any message from the client.
                if let Some(pid) = parent_pid {
                    if ProcState::Alive != proc_alive(pid) {
                        return InitializationStatus {
                            req: Request::ExitNotification(ExitNotification {}),
                            rc: ReturnCode::UnavailableErr,
                        };
                    }
                }
                continue;
            }
            Err(err) => {
                // The message could not be parsed, so we have no request ID to
                // work with.
                channel.send_response_error(None, err);
                continue;
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
                data: None,
            }),
            None => {
                cfg.parent_pid = Some(parent_pid);
            }
        }
    }

    Ok(())
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

fn wait_for_initialized_notif(
    channel: &mut StdioChannel,
    parent_pid: Option<u32>,
) -> Result<(), ReturnCode> {
    let mut shutdown_request_recv = false;
    loop {
        let req = match channel.recv_msg() {
            Ok(Some(r)) => r,
            Ok(None) => {
                // The server should shut down, if it detects that its parent
                // process is not alive anymore. No actual shutdown request was
                // received, so we exit with an error code. We only check if we
                // did not receive any message from the client.
                if let Some(pid) = parent_pid {
                    if ProcState::Alive != proc_alive(pid) {
                        return Err(ReturnCode::UnavailableErr);
                    }
                }
                continue;
            }
            Err(err) => {
                // The message could not be parsed, so we have no request ID to
                // work with.
                channel.send_response_error(None, err);
                continue;
            }
        };

        match &req {
            Request::InitializedNotification(_) => return Ok(()),
            Request::ExitNotification(_) => {
                return Err(if shutdown_request_recv {
                    ReturnCode::OkExit
                } else {
                    ReturnCode::ErrExit
                })
            }
            r => {
                if let Request::ShutdownRequest(_) = r {
                    shutdown_request_recv = true;
                }
                channel.send_response_error(req.get_id(), error_no_initialize_conf());
            }
        }
    }
}

fn error_no_initialize_conf() -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidRequest as i64,
        message: "Error: Server still waiting for initialized notification. Cannot handle request."
            .to_string(),
        data: None,
    }
}
