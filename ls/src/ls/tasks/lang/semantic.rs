// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2
//

use crate::{
    ReturnCode,
    config::SemanticTokenSupport,
    ls::{
        TextDocs,
        doc::TextDocData,
        language::{capture_semantic_tokens_doc_range, capture_semantic_tokens_full_doc},
        lsp::Message,
        request::Notification,
        response::{NullResponse, Response},
        tasks::{Task, Tasks, trace_doc_unknown, try_schedule},
    },
    protocol::{
        LogTraceParams, NumberOrString, SemanticTokensParams, SemanticTokensRangeParams, TraceValue,
    },
};

pub fn process_semantic_tokens_full_req(
    id: NumberOrString,
    params: SemanticTokensParams,
    trace_level: TraceValue,
    capabilities: SemanticTokenSupport,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_semantic_tok_full_req(id.clone())));
    }

    let (doc, tree) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, _)) => (doc, tree),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut ts.runner,
        Task::SemanticTokensFull(
            id,
            capabilities.legend,
            capabilities.encoding,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            capture_semantic_tokens_full_doc,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

pub fn process_semantic_tokens_range_req(
    id: NumberOrString,
    params: SemanticTokensRangeParams,
    trace_level: TraceValue,
    capabilities: SemanticTokenSupport,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_semantic_tok_range_req(id.clone())));
    }

    let (doc, tree) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, _)) => (doc, tree),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut ts.runner,
        Task::SemanticTokensRange(
            id,
            capabilities.legend,
            capabilities.encoding,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            params.range,
            capture_semantic_tokens_doc_range,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

fn log_semantic_tok_full_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received semantic tokens for full file request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}

fn log_semantic_tok_range_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received semantic tokens for file range request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}
