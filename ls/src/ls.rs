// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    config::Config,
    proc::{proc_alive, ProcState},
    protocol::{ErrorCodes, InitializeResult, ResponseError, ServerCapabilities},
    request::{ExitNotification, Request},
    response::ResponseResult,
    transport::StdioChannel,
    ReturnCode,
};

struct InitializationStatus {
    req: Request,
    shutdown_request_recv: bool,
}

pub fn serve(mut channel: StdioChannel, cfg: Config) -> ReturnCode {
    let status = wait_for_initialization(&mut channel, cfg.parent_pid);
    match status.req {
        Request::InitializeRequest(req) => {
            let result = ResponseResult::InitializeResult(InitializeResult::build(
                ServerCapabilities::build(),
            ));
            channel.send_response(req.id, result);
        }
        // No shutdown request was received before
        Request::ExitNotification(_) => return shutdown(status.shutdown_request_recv),
        _ => unreachable!(),
    }

    // let _ = eval(buf);

    drop(channel);
    ReturnCode::OkExit
}

/// Wait for initialization request. Returns `ServerNotInitialized` error for
/// other types of `RequestMessage`. Notifications are dropped. The only
/// exception is the exit notification after which the server shuts down.
/// Exit notifications without prior shutdown request result should trigger an
/// error exit code. However, sending a shutdown request without prior
/// initialization will return an error response.
fn wait_for_initialization(
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
                            req: Request::ExitNotification(ExitNotification { id: None }),
                            shutdown_request_recv: false,
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
                    shutdown_request_recv,
                }
            }
            r if r.is_request() => {
                if let Request::ShutdownRequest(_) = r {
                    shutdown_request_recv = true;
                }
                channel.send_response_error(
                    Some(req.get_id().expect("Must be request.")),
                    error_not_initialized(),
                );
            }
            _ => (),
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

fn shutdown(initialized: bool) -> ReturnCode {
    if initialized {
        ReturnCode::OkExit
    } else {
        ReturnCode::ErrExit
    }
}
