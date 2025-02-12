// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use crate::protocol::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, InitializedParams,
    LogTraceParams, NumberOrString, SetTraceParams,
};

// Requests from client to server.
#[derive(Debug)]
pub enum Request {
    InitializeRequest(InitializeRequest),
    ShutdownRequest(ShutdownRequest),
}

#[derive(Debug)]
pub enum Notification {
    DidChangeTextDocumentNotification(DidChangeTextDocumentNotification),
    DidOpenTextDocumentNotification(DidOpenTextDocumentNotification),
    ExitNotification(ExitNotification),
    InitializedNotification(InitializedNotification),
    LogTraceNotification(LogTraceNotification),
    SetTraceNotification(SetTraceNotification),
}

#[derive(Debug)]
pub struct DidChangeTextDocumentNotification {
    pub params: DidChangeTextDocumentParams,
}

#[derive(Debug)]
pub struct DidOpenTextDocumentNotification {
    pub params: DidOpenTextDocumentParams,
}

#[derive(Debug)]
pub struct ExitNotification {}

#[derive(Debug)]
pub struct InitializedNotification {
    #[allow(dead_code)]
    pub params: InitializedParams,
}

#[derive(Debug)]
pub struct InitializeRequest {
    pub id: NumberOrString,
    pub params: InitializeParams,
}

#[derive(Debug)]
pub struct LogTraceNotification {
    pub params: LogTraceParams,
}

#[derive(Debug)]
pub struct SetTraceNotification {
    pub params: SetTraceParams,
}

#[derive(Debug)]
pub struct ShutdownRequest {
    pub id: NumberOrString,
}

impl Request {
    pub fn get_id(&self) -> Option<&NumberOrString> {
        match self {
            Request::InitializeRequest(InitializeRequest { id, .. }) => Some(id),
            Request::ShutdownRequest(ShutdownRequest { id }) => Some(id),
        }
    }
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
