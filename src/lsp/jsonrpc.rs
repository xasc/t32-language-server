// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{
    de::DeserializeOwned,
    ser::{Serialize, SerializeStruct, Serializer},
};
use serde_json::{error::Category, Error, Value};

use crate::{
    lsp::RequestMessage,
    protocol::{ErrorCodes, InitializeParams, NumberOrString, ResponseError},
    request::{ExitNotification, InitializeRequest, Request, ShutdownRequest},
    response::ResponseResult,
};

/// Line format of `ResponseMessage` responses from
/// server to client.
struct ResponseMessage {
    pub jsonrpc: String,
    pub id: Option<NumberOrString>,
    pub result: Option<ResponseResult>,
    pub error: Option<ResponseError>,
}

impl Serialize for ResponseMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        debug_assert!(
            (self.error.is_none() && self.result.is_some())
                || (self.error.is_some() && self.result.is_none())
                || self.result.is_none()
        );

        let num_fields = match self.id {
            Some(_) => 3,
            None => 2,
        };
        let mut msg = serializer.serialize_struct("ResponseMessage", num_fields)?;

        msg.serialize_field("jsonrpc", &self.jsonrpc)?;

        if let Some(id) = &self.id {
            msg.serialize_field("id", id)?;
        }

        // `ResponseMessage` may only have either the `result` or `error` field, not both.
        // If a request was successful but there is no result to return, the response should
        // return `null` as the result.
        if let Some(err) = &self.error {
            msg.serialize_field("error", err)?;
        } else if let Some(res) = &self.result {
            msg.serialize_field("result", res)?;
        } else {
            msg.serialize_field("result", &Value::Null)?;
        }
        msg.end()
    }
}

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

pub fn make_response(id: NumberOrString, result: Option<ResponseResult>) -> String {
    let resp = ResponseMessage {
        jsonrpc: "2.0".to_string(),
        id: Some(id),
        error: None,
        result,
    };
    serde_json::ser::to_string(&resp).expect("Response serialization must not fail.")
}

pub fn make_error_response(id: Option<NumberOrString>, error: ResponseError) -> String {
    let msg = ResponseMessage {
        jsonrpc: "2.0".to_string(),
        id,
        error: Some(error),
        result: None,
    };
    serde_json::ser::to_string(&msg).expect("Response error serialization must not fail.")
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

    #[test]
    fn can_create_error_response() {
        let error = r#"{"code":-32700,"message":"Error"}"#;
        let expected = format!(r#"{{"jsonrpc":"2.0","id":1,"error":{}}}"#, error);

        let error: ResponseError = serde_json::from_str(error).expect("Should not fail.");
        let msg = make_error_response(Some(NumberOrString::Number(1)), error);

        assert_eq!(msg, expected);
    }

    #[test]
    fn can_create_response() {
        let result = r#"{"capabilities":{}}"#;
        let expected = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{}}}"#, result);

        let res: ResponseResult = serde_json::from_str(result).expect("Should not fail.");
        let msg = make_response(NumberOrString::Number(1), Some(res));

        assert_eq!(msg, expected);
    }

    #[test]
    fn can_create_null_response() {
        let result = "null";
        let expected = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{}}}"#, result);

        let msg = make_response(NumberOrString::Number(1), None);

        assert_eq!(msg, expected);
    }
}
