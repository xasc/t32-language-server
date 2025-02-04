// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::protocol::{
    InitializeResult, NumberOrString, PositionEncodingKind, ResponseError, ServerCapabilities,
    ServerInfo, TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncServerCapabilities,
};
use serde::{Deserialize, Serialize};

// Responses sent from server to client
#[derive(Debug, Deserialize, Serialize)]
pub enum Response {
    ErrorResponse(ErrorResponse),
    InitializeResponse(InitializeResponse),
    NullResponse(NullResponse),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ErrorResponse {
    pub id: Option<NumberOrString>,
    pub error: ResponseError,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitializeResponse {
    pub id: NumberOrString,
    pub result: InitializeResult,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NullResponse {
    pub id: NumberOrString,
}

impl ServerCapabilities {
    pub fn build() -> Self {
        ServerCapabilities {
            position_encoding: Some(PositionEncodingKind::Utf16),
            text_document_sync: Some(TextDocumentSyncServerCapabilities::TextDocumentSyncOptions(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::Incremental),
                    will_save: None,
                    will_save_wait_until: None,
                    save: None,
                },
            )),
            notebook_document_sync: None,
            completion_provider: None,
            hover_provider: None,
            signature_help_provider: None,
            declaration_provider: None,
            definition_provider: None,
            type_definition_provider: None,
            implementation_provider: None,
            references_provider: None,
            document_highlight_provider: None,
            document_symbol_provider: None,
            code_action_provider: None,
            code_lens_provider: None,
            document_link_provider: None,
            color_provider: None,
            document_formatting_provider: None,
            document_range_formatting_provider: None,
            document_on_type_formatting_provider: None,
            rename_provider: None,
            folding_range_provider: None,
            execute_command_provider: None,
            selection_range_provider: None,
            linked_editing_range_provider: None,
            call_hierarchy_provider: None,
            semantic_tokens_provider: None,
            moniker_provider: None,
            type_hierarchy_provider: None,
            inline_value_provider: None,
            inlay_hint_provider: None,
            diagnostic_provider: None,
            workspace_symbol_provider: None,
            inline_completion_provider: None,
            text_document: None,
            workspace: None,
            experimental: None,
        }
    }
}

impl InitializeResult {
    pub fn build(capabilities: ServerCapabilities) -> Self {
        Self {
            capabilities,
            server_info: Some(ServerInfo {
                name: "t32-language-server".to_string(),
                version: None,
            }),
        }
    }
}
