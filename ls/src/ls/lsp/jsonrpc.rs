// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeOwned, Deserializer, MapAccess, Visitor},
    ser::{SerializeStruct, Serializer},
};
use serde_json::{Error, Value, error::Category};

use crate::{
    ls::lsp::Message,
    ls::request::{
        DidChangeTextDocumentNotification, DidCloseTextDocumentNotification,
        DidOpenTextDocumentNotification, ExitNotification, GoToDefinitionRequest,
        InitializeRequest, InitializedNotification, LogTraceNotification, Notification, Request,
        SetTraceNotification, ShutdownRequest,
    },
    ls::response::{
        ErrorResponse, GoToDefinitionResponse, InitializeResponse, NullResponse, Response,
    },
    protocol::{
        DefinitionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, ErrorCodes, InitializeParams, InitializedParams, NumberOrString,
        ResponseError, SetTraceParams,
    },
};

/// Serialization formats of `RequestMessage`, `NotificationMessage`, and
/// `ResponseMessage` when they are sent on the line from client to server or
/// vice versa.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LineMessage {
    RequestMessage(RequestMessage),
    NotificationMessage(NotificationMessage),
    ResponseMessage(ResponseMessage),
}

/// Serialization format for `RequestMessage` type.
#[derive(Debug, Serialize, Deserialize)]
pub struct RequestMessage {
    pub jsonrpc: String,
    pub id: NumberOrString,
    pub method: String,
    pub params: Option<Value>,
}

/// Serialization format for `NotificationMessage` type.
#[derive(Debug, Deserialize, Serialize)]
pub struct NotificationMessage {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}

/// Serialization format for `ResponseMessage` type.
#[derive(Debug)]
pub struct ResponseMessage {
    pub jsonrpc: String,
    pub id: Option<NumberOrString>,
    pub result: Option<Value>,
    pub error: Option<ResponseError>,
}

const JSONRPC_VER: &'static str = "2.0";

impl LineMessage {
    pub fn get_jsonrpc(&self) -> &str {
        match self {
            LineMessage::RequestMessage(RequestMessage { jsonrpc, .. }) => jsonrpc,
            LineMessage::NotificationMessage(NotificationMessage { jsonrpc, .. }) => jsonrpc,
            LineMessage::ResponseMessage(ResponseMessage { jsonrpc, .. }) => jsonrpc,
        }
    }

    pub fn get_id(self) -> Option<NumberOrString> {
        match self {
            LineMessage::RequestMessage(RequestMessage { id, .. }) => Some(id),
            LineMessage::NotificationMessage(_) => None,
            LineMessage::ResponseMessage(ResponseMessage { id, .. }) => id,
        }
    }
}

impl ResponseMessage {
    fn build_result(jsonrpc: String, id: Option<NumberOrString>, result: Value) -> Self {
        ResponseMessage {
            jsonrpc,
            id,
            result: Some(result),
            error: None,
        }
    }

    fn build_error(jsonrpc: String, id: Option<NumberOrString>, error: ResponseError) -> Self {
        ResponseMessage {
            jsonrpc,
            id,
            result: None,
            error: Some(error),
        }
    }
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

impl<'de> de::Deserialize<'de> for ResponseMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        enum Field {
            Jsonrpc,
            Id,
            Result,
            Error,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("`jsonrpc`, `id`, `result`, or `error`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: de::Error,
                    {
                        match value {
                            "jsonrpc" => Ok(Field::Jsonrpc),
                            "id" => Ok(Field::Id),
                            "result" => Ok(Field::Result),
                            "error" => Ok(Field::Error),
                            _ => Err(de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct ResponseMessageVisitor;

        impl<'de> Visitor<'de> for ResponseMessageVisitor {
            type Value = ResponseMessage;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct ResponseMessage")
            }

            fn visit_map<V>(self, mut map: V) -> Result<ResponseMessage, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut jsonrpc: Option<String> = None;
                let mut id: Option<NumberOrString> = None;
                let mut result: Option<Value> = None;
                let mut error: Option<ResponseError> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Jsonrpc => {
                            if jsonrpc.is_none() {
                                return Err(de::Error::duplicate_field("jsonrpc"));
                            }
                            jsonrpc = Some(map.next_value()?);
                        }
                        Field::Id => {
                            if id.is_none() {
                                return Err(de::Error::duplicate_field("id"));
                            }
                            id = Some(map.next_value()?);
                        }
                        Field::Result => {
                            if result.is_none() {
                                return Err(de::Error::duplicate_field("result"));
                            }
                            result = Some(map.next_value()?);
                        }
                        Field::Error => {
                            if error.is_none() {
                                return Err(de::Error::duplicate_field("error"));
                            }
                            error = Some(map.next_value()?);
                        }
                    }
                }
                let jsonrpc = jsonrpc.ok_or_else(|| de::Error::missing_field("jsonrpc"))?;

                if result.is_none() && error.is_none() {
                    Err(de::Error::custom("neither field `result` nor `error`"))
                } else if result.is_some() && error.is_some() {
                    Err(de::Error::custom(
                        "either field `result` or `error` expected, not both",
                    ))
                } else if result.is_some() {
                    Ok(ResponseMessage::build_result(jsonrpc, id, result.unwrap()))
                } else {
                    Ok(ResponseMessage::build_error(jsonrpc, id, error.unwrap()))
                }
            }
        }

        const FIELDS: &[&str] = &["jsonrpc", "id", "result", "error"];
        deserializer.deserialize_struct("ResponseMessage", FIELDS, ResponseMessageVisitor)
    }
}

pub fn parse_message(buf: &[u8]) -> Result<LineMessage, ResponseError> {
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

pub fn deserialize_msg(msg: LineMessage) -> Result<Message, ErrorResponse> {
    let ver = msg.get_jsonrpc();
    if ver != JSONRPC_VER {
        let ver = ver.to_string();
        return Err(ErrorResponse {
            id: msg.get_id(),
            error: error_jsonrcp_ver(&ver),
        });
    }

    match msg {
        LineMessage::RequestMessage(m) => deserialize_request(m),
        LineMessage::NotificationMessage(n) => deserialize_notif(n),
        _ => unreachable!(),
    }
}

pub fn serialize_msg(msg: Message) -> String {
    let repr = match msg {
        Message::Response(resp) => serialize_response(resp),
        Message::Notification(n) => serialize_nofif(n),
        Message::Request(_) => todo!(),
    };
    serde_json::to_string(&repr).expect("Serialization must not fail.")
}

fn deserialize_request(msg: RequestMessage) -> Result<Message, ErrorResponse> {
    const INITIALIZE: &'static str = "initialize";
    const SHUTDOWN: &'static str = "shutdown";
    const TEXTDOC_DEFINITION: &'static str = "textDocument/definition";

    match msg.method.as_str() {
        INITIALIZE => match deserialize_msg_params::<InitializeParams>(msg.params) {
            Ok(params) => Ok(Message::Request(Request::InitializeRequest(
                InitializeRequest { id: msg.id, params },
            ))),
            Err(err) => Err(ErrorResponse {
                id: Some(msg.id),
                error: err,
            }),
        },
        SHUTDOWN => Ok(Message::Request(Request::ShutdownRequest(
            ShutdownRequest { id: msg.id },
        ))),
        TEXTDOC_DEFINITION => match deserialize_msg_params::<DefinitionParams>(msg.params) {
            Ok(params) => Ok(Message::Request(Request::GoToDefinition(
                GoToDefinitionRequest { id: msg.id, params },
            ))),
            Err(err) => Err(ErrorResponse {
                id: Some(msg.id),
                error: err,
            }),
        },
        method => Err(ErrorResponse {
            id: Some(msg.id),
            error: error_type(method),
        }),
    }
}

fn deserialize_notif(msg: NotificationMessage) -> Result<Message, ErrorResponse> {
    const EXIT: &'static str = "exit";
    const INITIALIZED: &'static str = "initialized";
    const SET_TRACE: &'static str = "$/setTrace";
    const TEXTDOC_DID_CLOSE: &'static str = "textDocument/didClose";
    const TEXTDOC_DID_CHANGE: &'static str = "textDocument/didChange";
    const TEXTDOC_DID_OPEN: &'static str = "textDocument/didOpen";

    match msg.method.as_str() {
        EXIT => Ok(Message::Notification(Notification::ExitNotification(
            ExitNotification {},
        ))),
        INITIALIZED => match deserialize_msg_params::<InitializedParams>(msg.params) {
            Ok(params) => Ok(Message::Notification(
                Notification::InitializedNotification(InitializedNotification { params }),
            )),
            Err(err) => Err(ErrorResponse {
                id: None,
                error: err,
            }),
        },
        SET_TRACE => match deserialize_msg_params::<SetTraceParams>(msg.params) {
            Ok(params) => Ok(Message::Notification(Notification::SetTraceNotification(
                SetTraceNotification { params },
            ))),
            Err(err) => Err(ErrorResponse {
                id: None,
                error: err,
            }),
        },
        TEXTDOC_DID_CLOSE => {
            match deserialize_msg_params::<DidCloseTextDocumentParams>(msg.params) {
                Ok(params) => Ok(Message::Notification(
                    Notification::DidCloseTextDocumentNotification(
                        DidCloseTextDocumentNotification { params },
                    ),
                )),
                Err(err) => Err(ErrorResponse {
                    id: None,
                    error: err,
                }),
            }
        }
        TEXTDOC_DID_CHANGE => {
            match deserialize_msg_params::<DidChangeTextDocumentParams>(msg.params) {
                Ok(params) => Ok(Message::Notification(
                    Notification::DidChangeTextDocumentNotification(
                        DidChangeTextDocumentNotification { params },
                    ),
                )),
                Err(err) => Err(ErrorResponse {
                    id: None,
                    error: err,
                }),
            }
        }
        TEXTDOC_DID_OPEN => match deserialize_msg_params::<DidOpenTextDocumentParams>(msg.params) {
            Ok(params) => Ok(Message::Notification(
                Notification::DidOpenTextDocumentNotification(DidOpenTextDocumentNotification {
                    params,
                }),
            )),
            Err(err) => Err(ErrorResponse {
                id: None,
                error: err,
            }),
        },
        method => Err(ErrorResponse {
            id: None,
            error: error_type(method),
        }),
    }
}

fn deserialize_msg_params<T: DeserializeOwned>(params: Option<Value>) -> Result<T, ResponseError> {
    if params.is_none() {
        return Err(error_missing("params"));
    }

    match request_params::<T>(params.unwrap()) {
        Ok(params) => Ok(params),
        Err(err) => Err(err),
    }
}

fn serialize_nofif(msg: Notification) -> LineMessage {
    const LOG_TRACE: &'static str = "$/logTrace";

    match msg {
        Notification::LogTraceNotification(LogTraceNotification { params }) => {
            LineMessage::NotificationMessage(NotificationMessage {
                jsonrpc: JSONRPC_VER.to_string(),
                method: LOG_TRACE.to_string(),
                params: Some(serde_json::to_value(params).expect("Serialization must not fail.")),
            })
        }
        _ => unreachable!("Notification type must not be sent to client."),
    }
}

fn serialize_response(msg: Response) -> LineMessage {
    let (id, result, error): (Option<NumberOrString>, Option<Value>, Option<ResponseError>) =
        match msg {
            Response::ErrorResponse(ErrorResponse { id, error }) => (id, None, Some(error)),
            Response::GoToDefinitionResponse(GoToDefinitionResponse { id, result }) => (
                Some(id),
                Some(serde_json::to_value(result).expect("Serialization must not fail.")),
                None,
            ),
            Response::InitializeResponse(InitializeResponse { id, result }) => (
                Some(id),
                Some(serde_json::to_value(result).expect("Serialization must not fail.")),
                None,
            ),
            Response::NullResponse(NullResponse { id }) => (Some(id), None, None),
        };

    LineMessage::ResponseMessage(ResponseMessage {
        jsonrpc: JSONRPC_VER.to_string(),
        id,
        result,
        error,
    })
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

fn error_missing(field: &str) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Missing field \"{}\" in message content.",
            field
        )),
        data: None,
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

fn error_type(method: &str) -> ResponseError {
    ResponseError {
        code: ErrorCodes::MethodNotFound as i64,
        message: format!(
            "Data error: Message method \"{}\" is not supported.",
            method
        ),
        data: None,
    }
}

fn error_jsonrcp_ver(ver: &str) -> ResponseError {
    ResponseError {
        code: ErrorCodes::InvalidRequest as i64,
        message: format!(
            "Data error: JSON-RPC protocol version \"{}\" is not supported. Expected version \"2.0\".",
            ver
        ),
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

    use crate::{
        ls::response::{GoToDefinitionResponse, InitializeResponse, LocationResult, Response},
        protocol::{self, Location, LocationLink, LogTraceParams, Position, Range},
    };
    use serde_json::json;

    #[test]
    fn throws_error_on_missing_field() {
        let content = r#"{ "jsonrpc": "2.0", "id": 1 }"#;

        let rc = parse_message(content.as_bytes());
        assert!(matches!(rc.err(), Some(ResponseError { .. })));
    }

    #[test]
    fn can_create_exit_notification() {
        let msg = LineMessage::NotificationMessage(NotificationMessage {
            jsonrpc: "2.0".to_string(),
            method: "exit".to_string(),
            params: None,
        });

        let notif = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            notif,
            Message::Notification(Notification::ExitNotification(_))
        ));
    }

    #[test]
    fn can_create_initialized_notification() {
        let msg = LineMessage::NotificationMessage(NotificationMessage {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: Some(json!({})),
        });

        let notif = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            notif,
            Message::Notification(Notification::InitializedNotification(_))
        ));
    }

    #[test]
    fn can_create_initialize_request() {
        let params = json!({
            "processId": 1,
            "rootUri": null,
            "capabilities": {}
        });

        let msg = LineMessage::RequestMessage(RequestMessage {
            jsonrpc: "2.0".to_string(),
            id: protocol::NumberOrString::Number(1),
            method: "initialize".to_string(),
            params: Some(params.clone()),
        });

        let req = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            req,
            Message::Request(Request::InitializeRequest(_))
        ));
    }

    #[test]
    fn can_create_shutdown_request() {
        let msg = LineMessage::RequestMessage(RequestMessage {
            jsonrpc: "2.0".to_string(),
            id: protocol::NumberOrString::Number(1),
            method: "shutdown".to_string(),
            params: None,
        });

        let req = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(req, Message::Request(Request::ShutdownRequest(_))));
    }

    #[test]
    fn can_create_goto_definition_request() {
        let msg = LineMessage::RequestMessage(RequestMessage {
            jsonrpc: "2.0".to_string(),
            id: protocol::NumberOrString::Number(1),
            method: "textDocument/definition".to_string(),
            params: Some(json!({
                "textDocument": {
                    "uri": "file:///project/test.cmm"
                },
                "position": {
                    "line": 8,
                    "character": 17
                },
            })),
        });

        let req = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(req, Message::Request(Request::GoToDefinition(_))));
    }

    #[test]
    fn can_create_set_trace_notification() {
        let msg = LineMessage::NotificationMessage(NotificationMessage {
            jsonrpc: "2.0".to_string(),
            method: "$/setTrace".to_string(),
            params: Some(json!({
                "value": "verbose",
            })),
        });

        let notif = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            notif,
            Message::Notification(Notification::SetTraceNotification(_))
        ));
    }

    #[test]
    fn can_create_did_open_text_document_notification() {
        let msg = LineMessage::NotificationMessage(NotificationMessage {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didOpen".to_string(),
            params: Some(json!({
                "textDocument": {
                    "uri": "file:///c:/project/readme.md",
                    "languageId": "practice",
                    "version": 1,
                    "text": "This is a test",
                },
            })),
        });

        let notif = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            notif,
            Message::Notification(Notification::DidOpenTextDocumentNotification(_))
        ));
    }

    #[test]
    fn can_create_did_change_text_document_notification() {
        let msg = LineMessage::NotificationMessage(NotificationMessage {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didChange".to_string(),
            params: Some(json!({
                "textDocument": {
                    "uri": "file:///c:/project/readme.md",
                    "version": 1,
                },
                "contentChanges": [{
                    "range": {
                        "start": {
                            "line": 5,
                            "character": 43,
                        },
                        "end": {
                            "line": 7,
                            "character": 0,
                        }
                    },
                    "text": "Replacement text",
                }],
            })),
        });

        let notif = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            notif,
            Message::Notification(Notification::DidChangeTextDocumentNotification(_))
        ));
    }

    #[test]
    fn can_create_did_close_text_document_notification() {
        let msg = LineMessage::NotificationMessage(NotificationMessage {
            jsonrpc: "2.0".to_string(),
            method: "textDocument/didClose".to_string(),
            params: Some(json!({
                "textDocument": {
                    "uri": "file:///c:/project/a.cmm",
                },
            })),
        });

        let notif = deserialize_msg(msg).expect("Should not fail.");

        assert!(matches!(
            notif,
            Message::Notification(Notification::DidCloseTextDocumentNotification(_))
        ));
    }

    #[test]
    fn can_create_error_response() {
        let error = r#"{"code":-32700,"message":"Error"}"#;
        let expected = format!(r#"{{"jsonrpc":"2.0","id":1,"error":{}}}"#, error);

        let error: ResponseError = serde_json::from_str(error).expect("Should not fail.");
        let msg = serialize_msg(Message::Response(Response::ErrorResponse(ErrorResponse {
            id: Some(NumberOrString::Number(1)),
            error,
        })));

        assert_eq!(msg, expected);
    }

    #[test]
    fn can_create_goto_definition_response() {
        let expected = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":[{\"originSelectionRange\":{\"end\":{\"character\":12,\"line\":9},\"start\":{\"character\":23,\"line\":8}},\"targetRange\":{\"end\":{\"character\":1,\"line\":3},\"start\":{\"character\":4,\"line\":0}},\"targetSelectionRange\":{\"end\":{\"character\":0,\"line\":2},\"start\":{\"character\":0,\"line\":1}},\"targetUri\":\"file:///project/test.cmm\"}]}";

        let response = GoToDefinitionResponse {
            id: NumberOrString::Number(1),
            result: Some(LocationResult::ExtMeta(vec![LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 8,
                        character: 23,
                    },
                    end: Position {
                        line: 9,
                        character: 12,
                    },
                }),
                target_uri: "file:///project/test.cmm".to_string(),
                target_range: Range {
                    start: Position {
                        line: 0,
                        character: 4,
                    },
                    end: Position {
                        line: 3,
                        character: 1,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 0,
                    },
                },
            }])),
        };
        let msg = serialize_msg(Message::Response(Response::GoToDefinitionResponse(
            response,
        )));
        assert_eq!(msg, expected);

        let expected = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"range\":{\"end\":{\"character\":0,\"line\":2},\"start\":{\"character\":0,\"line\":1}},\"uri\":\"file:///project/test.cmm\"}}";

        let response = GoToDefinitionResponse {
            id: NumberOrString::Number(2),
            result: Some(LocationResult::Single(Location {
                uri: "file:///project/test.cmm".to_string(),
                range: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 0,
                    },
                },
            })),
        };

        let msg = serialize_msg(Message::Response(Response::GoToDefinitionResponse(
            response,
        )));
        assert_eq!(msg, expected);

        let expected = "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":[{\"range\":{\"end\":{\"character\":17,\"line\":102},\"start\":{\"character\":4,\"line\":81}},\"uri\":\"file:///project/test.cmm\"}]}";

        let response = GoToDefinitionResponse {
            id: NumberOrString::Number(2),
            result: Some(LocationResult::Multi(vec![Location {
                uri: "file:///project/test.cmm".to_string(),
                range: Range {
                    start: Position {
                        line: 81,
                        character: 4,
                    },
                    end: Position {
                        line: 102,
                        character: 17,
                    },
                },
            }])),
        };

        let msg = serialize_msg(Message::Response(Response::GoToDefinitionResponse(
            response,
        )));
        assert_eq!(msg, expected.to_string());
    }

    #[test]
    fn can_create_initialize_response() {
        let result = r#"{"capabilities":{}}"#;
        let expected = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{}}}"#, result);

        let res: protocol::InitializeResult =
            serde_json::from_str(result).expect("Should not fail.");
        let msg = serialize_msg(Message::Response(Response::InitializeResponse(
            InitializeResponse {
                id: NumberOrString::Number(1),
                result: res,
            },
        )));

        assert_eq!(msg, expected);
    }

    #[test]
    fn can_create_log_trace_notification() {
        let expected = r#"{"jsonrpc":"2.0","method":"$/logTrace","params":{"message":"Log message","verbose":"Verbose log message"}}"#;

        let msg = Message::Notification(Notification::LogTraceNotification(LogTraceNotification {
            params: LogTraceParams {
                message: "Log message".to_string(),
                verbose: Some("Verbose log message".to_string()),
            },
        }));
        let notif = serialize_msg(msg);

        assert_eq!(notif, expected.to_string());
    }

    #[test]
    fn can_create_shutdown_response() {
        let expected = r#"{"jsonrpc":"2.0","id":99,"result":null}"#;

        let msg = Message::Response(Response::NullResponse(NullResponse {
            id: NumberOrString::Number(99),
        }));
        let response = serialize_msg(msg);

        assert_eq!(response, expected.to_string());
    }
}
