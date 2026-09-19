// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::fmt;

use crate::protocol::DefinitionParams;
use crate::protocol::DidChangeTextDocumentParams;
use crate::protocol::DidCloseTextDocumentParams;
use crate::protocol::DidOpenTextDocumentParams;
use crate::protocol::FoldingRangeParams;
use crate::protocol::InitializeParams;
use crate::protocol::InitializedParams;
use crate::protocol::LogTraceParams;
use crate::protocol::NumberOrString;
use crate::protocol::ProgressParams;
use crate::protocol::ReferenceParams;
use crate::protocol::RenameFilesParams;
use crate::protocol::SemanticTokensParams;
use crate::protocol::SemanticTokensRangeParams;
use crate::protocol::SetTraceParams;
use crate::protocol::WorkDoneProgressCancelParams;
use crate::protocol::WorkDoneProgressCreateParams;

// Requests from client to server.
#[derive(Debug, PartialEq)]
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
    WindowWorkDoneProgressCreate {
        id: NumberOrString,
        params: WorkDoneProgressCreateParams,
    },
}

#[derive(Debug, PartialEq)]
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
        params: InitializedParams,
    },
    LogTraceNotification {
        params: LogTraceParams,
    },
    SetTraceNotification {
        params: SetTraceParams,
    },
    WorkDoneProgressNotification {
        params: ProgressParams,
    },
    WorkDoneProgressCancelNotification {
        params: WorkDoneProgressCancelParams,
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
            Request::WindowWorkDoneProgressCreate { id, .. } => id,
        }
    }
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
