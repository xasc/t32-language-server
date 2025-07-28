// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

use crate::protocol::{
    DefinitionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializedParams, LogTraceParams, NumberOrString,
    SetTraceParams,
};

// Requests from client to server.
#[derive(Debug)]
pub enum Request {
    GoToDefinition {
        id: NumberOrString,
        params: DefinitionParams,
    },
    InitializeRequest {
        id: NumberOrString,
        params: InitializeParams,
    },
    ShutdownRequest {
        id: NumberOrString,
    },
}

#[derive(Debug)]
pub enum Notification {
    DidCloseTextDocumentNotification {
        params: DidCloseTextDocumentParams,
    },
    DidChangeTextDocumentNotification {
        params: DidChangeTextDocumentParams,
    },
    DidOpenTextDocumentNotification {
        params: DidOpenTextDocumentParams,
    },
    ExitNotification {},
    InitializedNotification {
        #[allow(dead_code)]
        params: InitializedParams,
    },
    LogTraceNotification {
        params: LogTraceParams,
    },
    SetTraceNotification {
        params: SetTraceParams,
    },
}

impl Request {
    pub fn get_id(&self) -> Option<&NumberOrString> {
        match self {
            Request::GoToDefinition { id, .. } => Some(id),
            Request::InitializeRequest { id, .. } => Some(id),
            Request::ShutdownRequest { id } => Some(id),
        }
    }
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
