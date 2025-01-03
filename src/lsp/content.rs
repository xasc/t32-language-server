// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::de::DeserializeOwned;
use serde_json::{error::Category, Error, Value};

use crate::{
    protocol::{ErrorCodes, InitializeParams, RequestMessage, ResponseError},
    request::{ExitNotification, InitializeRequest, Request, ShutdownRequest},
};

pub fn parse_message(buf: &[u8]) -> Result<RequestMessage, ResponseError> {
    match serde_json::from_slice(buf) {
        Ok(val) => Ok(val),
        Err(err) => {
            match err.classify() {
                Category::Io => unreachable!(), // Byte buffer must be valid.
                Category::Syntax => return Err(error_syntax(err, Some(buf))),
                Category::Data => return Err(error_data(err, Some(buf))),
                Category::Eof => return Err(error_incomplete(err, buf.len())),
            }
        }
    }
}

pub fn make_request(msg: RequestMessage) -> Result<Request, ResponseError> {
    const EXIT: &'static str = "exit";
    const INITIALIZE: &'static str = "initialize";
    const SHUTDOWN: &'static str = "shutdown";

    match msg.method.as_str() {
        EXIT => Ok(Request::ExitNotification(ExitNotification {
            id: msg.id.expect("Exit notification must have \"id\" field."),
        })),
        INITIALIZE => Ok(Request::InitializeRequest(InitializeRequest {
            id: msg.id.expect("Initialize request must have \"id\" field."),
            params: request_params::<InitializeParams>(
                msg.params
                    .expect("Initialize request must have \"params\" field."),
            )?,
        })),
        SHUTDOWN => Ok(Request::ShutdownRequest(ShutdownRequest {
            id: msg.id.expect("Shutdown request must have \"id\" field."),
        })),
        _ => unreachable!(),
    }
}

fn error_syntax(err: Error, buf: Option<&[u8]>) -> ResponseError {
    if err.line() == 0 {
        ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from(format!(
                "Syntax error: Unexpected data in message content due to {}.",
                err.to_string()
            )),
            data: None,
        }
    } else {
        let offset = match buf {
            Some(b) => get_error_offset(&err, b),
            None => 0,
        };

        ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from(format!(
                "Syntax error: Unexpected data in message content at offset \"{}\" due to {}.",
                offset,
                err.to_string(),
            )),
            data: None,
        }
    }
}

fn error_data(err: Error, buf: Option<&[u8]>) -> ResponseError {
    if err.line() == 0 {
        ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from(format!(
                "Data error: Semantically incorrect data in message content due to {}.",
                err.to_string()
            )),
            data: None,
        }
    } else {
        let offset = match buf {
            Some(b) => get_error_offset(&err, b),
            None => 0,
        };
        ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from(format!(
                "Data error: Semantically incorrect data in message content at offset \"{}\" due to {}.",
                offset,
                err.to_string()
            )),
            data: None,
        }
    }
}

fn error_incomplete(err: Error, len: usize) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Data error: Message content is incomplete due to {}. Expected a total length of \"{}\" bytes.",
            err.to_string(),
            len
        )),
        data: None,
    }
}

fn get_error_offset(err: &Error, buf: &[u8]) -> usize {
    let mut line: usize = 1;
    let mut col: usize = 1;

    let mut offset: usize = 0;
    if err.line() <= 0 {
        return offset;
    }

    for ch in buf {
        if line == err.line() {
            if err.column() <= 0 || col == err.column() {
                break;
            }
            col += 1;
        } else if (*ch as char) == '\n' {
            line += 1;
        }
        offset += 1;
    }
    offset
}

fn request_params<T: DeserializeOwned>(params: Value) -> Result<T, ResponseError> {
    match serde_json::from_value::<T>(params) {
        Ok(val) => Ok(val),
        Err(err) => {
            match err.classify() {
                Category::Syntax => return Err(error_syntax(err, None)),
                Category::Data => return Err(error_data(err, None)),
                Category::Io => unreachable!(), // Byte buffer must be valid.
                Category::Eof => unreachable!(), // We already have a valid JSON value.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::protocol;
    use serde_json::json;

    #[test]
    fn throws_error_on_missing_field() {
        let content = r#"{ "jsonrpc": "2.0", "id": 1 }"#;

        let rc = parse_message(content.as_bytes());
        assert!(matches!(rc.err(), Some(ResponseError { .. })));
    }

    #[test]
    fn can_create_exit_notification() {
        let msg = RequestMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(protocol::NumberOrString::Number(1)),
            method: "exit".to_string(),
            params: None,
        };

        let r = make_request(msg).expect("Should not fail.");

        assert!(matches!(r, Request::ExitNotification(_)));
    }

    #[test]
    fn can_create_initialize_request() {
        let params = json!({
            "processId": 1,
            "rootUri": null,
            "capabilities": {}
        });

        let msg = RequestMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(protocol::NumberOrString::Number(1)),
            method: "initialize".to_string(),
            params: Some(params.clone()),
        };

        let r = make_request(msg).expect("Should not fail.");

        assert!(matches!(r, Request::InitializeRequest(_)));
    }

    #[test]
    fn can_create_shutdown_request() {
        let msg = RequestMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(protocol::NumberOrString::Number(1)),
            method: "shutdown".to_string(),
            params: None,
        };

        let r = make_request(msg).expect("Should not fail.");

        assert!(matches!(r, Request::ShutdownRequest(_)));
    }
}
