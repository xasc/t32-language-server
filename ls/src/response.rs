// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::protocol::{InitializeResult, PositionEncodingKind, ServerCapabilities, ServerInfo};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum ResponseResult {
    InitializeResult(InitializeResult),
}

impl ServerCapabilities {
    pub fn build() -> Self {
        ServerCapabilities {
            position_encoding: Some(PositionEncodingKind::Utf16),
            text_document_sync: None,
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

// pub trait Response {
//     fn serialize(self) -> ResponseMessage;
// }
//
// impl Response for InitializeResponse {
//     fn serialize(self) -> ResponseMessage {
//         make_response_msg(self.id, self.result)
//     }
// }
