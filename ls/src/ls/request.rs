// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::fmt;

use crate::protocol::{
    DefinitionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, FoldingRangeParams, InitializeParams, InitializedParams,
    LogTraceParams, NumberOrString, ReferenceParams, RenameFilesParams, SemanticTokensParams,
    SemanticTokensRangeParams, SetTraceParams,
};

// Requests from client to server.
#[derive(Debug)]
pub enum Request {
    FindReferences {
        id: NumberOrString,
        params: ReferenceParams,
    },
    FoldingRange {
        id: NumberOrString,
        params: FoldingRangeParams,
    },
    GoToDefinition {
        id: NumberOrString,
        params: DefinitionParams,
    },
    InitializeRequest {
        id: NumberOrString,
        params: InitializeParams,
    },
    SemanticTokensFull {
        id: NumberOrString,
        params: SemanticTokensParams,
    },
    SemanticTokensRange {
        id: NumberOrString,
        params: SemanticTokensRangeParams,
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
    DidRenameFilesNotification {
        params: RenameFilesParams,
    },
    ExitNotification {},
    InitializedNotification {
        #[expect(unused)]
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
    pub fn get_id(&self) -> &NumberOrString {
        match self {
            Request::FindReferences { id, .. } => id,
            Request::FoldingRange { id, .. } => id,
            Request::GoToDefinition { id, .. } => id,
            Request::InitializeRequest { id, .. } => id,
            Request::SemanticTokensFull { id, .. } => id,
            Request::SemanticTokensRange { id, .. } => id,
            Request::ShutdownRequest { id } => id,
        }
    }
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
