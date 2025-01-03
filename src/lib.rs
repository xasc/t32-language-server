// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later
//
mod config;
mod lsp;
mod proc;
mod protocol;
mod request;
mod response;
mod transport;

pub use config::Config;

use std::io::{BufRead, Write};

#[derive(Debug, PartialEq)]
pub enum ReturnCode {
    OkExit = 0,
    DataErr = 64,
    UsageErr = 65,
    IoErr = 74,
    ProtocolErr = 76,
}

pub struct Stdio<'a, R: BufRead, W: Write, E: Write> {
    pub reader: R,
    pub writer: &'a mut W,
    pub error: &'a mut E,
}

pub fn run<R, W, E>(args: Vec<String>, stdio: Stdio<R, W, E>) -> ReturnCode
where
    R: BufRead,
    W: Write,
    E: Write,
{
    let cfg = match config::Config::build(&args, stdio.reader, stdio.writer, stdio.error) {
        Ok(conf) => conf,
        Err(rc) => return rc,
    };
    let channel = transport::build_channel(cfg);

    serve(channel)
}

fn serve<R: BufRead, W: Write, E: Write>(mut channel: transport::Channel<R, W, E>) -> ReturnCode {
    let req = wait_for_initialization(&mut channel);
    debug_assert!(
        matches!(req, request::Request::InitializeRequest(_))
            || matches!(req, request::Request::InitializeRequest(_))
    );

    // let _ = eval(buf);
    ReturnCode::OkExit
}

/// Wait for initialization request. Returns `ServerNotInitialized` error for
/// other types of `RequestMessage`. Notifications are dropped. The only
/// exception is the exit notification after which the server shuts down.
fn wait_for_initialization<R: BufRead, W: Write, E: Write>(
    channel: &mut transport::Channel<R, W, E>,
) -> request::Request
where
    R: BufRead,
    W: Write,
    E: Write,
{
    loop {
        let req = match channel.read_msg() {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(err) => {
                channel.write_response_error(None, err);
                continue;
            }
        };

        match &req {
            request::Request::InitializeRequest(_) | request::Request::ExitNotification(_) => {
                return req
            }
            r if r.is_request() => {
                channel.write_response_error(Some(req.get_id()), error_not_initialized());
            }
            _ => (),
        }
    }
}

pub fn error_not_initialized() -> protocol::ResponseError {
    protocol::ResponseError {
        code: protocol::ErrorCodes::ServerNotInitialized as i64,
        message: "Error: Server not initialized. Cannot handle request.".to_string(),
        data: None,
    }
}
