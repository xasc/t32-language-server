// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::ReturnCode;
use crate::config::T32DefaultDirs;
use crate::ls::doc::TextDoc;
use crate::ls::doc::TextDocData;
use crate::ls::doc::TextDocs;
use crate::ls::doc::import_doc;
use crate::ls::doc::update_doc;
use crate::ls::lsp::Message;
use crate::ls::response::ErrorResponse;
use crate::ls::response::Response;
use crate::ls::tasks::Task;
use crate::ls::tasks::Tasks;
use crate::ls::tasks::try_schedule;
use crate::ls::workspace::FileIndex;
use crate::protocol::DidChangeTextDocumentParams;
use crate::protocol::ErrorCodes;
use crate::protocol::ResponseError;
use crate::protocol::TextDocumentItem;

pub fn process_doc_change_notif(
    params: DidChangeTextDocumentParams,
    docs: &TextDocs,
    files: FileIndex,
    dirs: T32DefaultDirs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if !docs.is_open(&params.text_document.uri) {
        outgoing.push(Some(error_textdoc_not_open(&params.text_document.uri)));
        return Ok(());
    }

    let (doc, tree, ..) = docs.get_doc_data(&params.text_document.uri).unwrap();

    let doc = TextDoc {
        version: params.text_document.version,
        ..doc.clone()
    };
    try_schedule(
        &mut ts.runner,
        Task::TextDocEdit(
            TextDocData {
                doc,
                tree: tree.clone(),
            },
            files,
            dirs,
            params.content_changes,
            update_doc,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )
}

pub fn process_doc_open_notif(
    doc: TextDocumentItem,
    files: FileIndex,
    dirs: T32DefaultDirs,
    ts: &mut Tasks,
) -> Result<(), ReturnCode> {
    try_schedule(
        &mut ts.runner,
        Task::TextDocNew(doc, files, dirs, import_doc),
        &mut ts.ongoing,
        &mut ts.blocked,
    )
}

pub fn process_doc_close_notif(
    uri: &str,
    docs: &mut TextDocs,
    outgoing: &mut Vec<Option<Message>>,
) {
    if !docs.is_open(uri) {
        outgoing.push(Some(error_textdoc_cannot_close(uri)));
        return;
    }
    docs.close(uri);
}

fn error_textdoc_not_open(uri: &str) -> Message {
    Message::Response(Response::ErrorResponse(ErrorResponse {
        id: None,
        error: ResponseError {
            code: ErrorCodes::InvalidRequest as i64,
            message: format!(
                "ERROR: Text document \"{}\" has not been opened, so it cannot be changed.",
                uri
            ),
            data: None,
        },
    }))
}

fn error_textdoc_cannot_close(uri: &str) -> Message {
    Message::Response(Response::ErrorResponse(ErrorResponse {
        id: None,
        error: ResponseError {
            code: ErrorCodes::InvalidRequest as i64,
            message: format!(
                "ERROR: Text document \"{}\" has not been opened, so it cannot be closed.",
                uri
            ),
            data: None,
        },
    }))
}
