// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use serde::{Deserialize, Serialize};

use crate::{
    protocol::{
        DefinitionOptions, DefinitionProvider, DocumentFilter, FileOperationFilter,
        FileOperationPattern, FileOperationPatternKind, FileOperationPatternOptions,
        FileOperationRegistrationOptions, InitializeResult, Location, LocationLink, NumberOrString,
        PositionEncodingKind, ReferenceOptions, ReferencesProvider, ResponseError, SemanticTokens,
        SemanticTokensFullDocumentCapabilities, SemanticTokensLegend, SemanticTokensProvider,
        SemanticTokensRegistrationOptions, ServerCapabilities, ServerInfo, TextDocumentSyncKind,
        TextDocumentSyncOptions, TextDocumentSyncServerCapabilities, WorkspaceFileOperations,
        WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
    },
    t32::{LANGUAGE_ID, SUFFIXES},
};

// Responses sent from server to client
#[derive(Debug, Deserialize, Serialize)]
pub enum Response {
    ErrorResponse(ErrorResponse),
    FindReferencesResponse(FindReferencesResponse),
    GoToDefinitionResponse(GoToDefinitionResponse),
    InitializeResponse(InitializeResponse),
    NullResponse(NullResponse),
    SemanticTokensFullResponse(SemanticTokensFullResponse),
    SemanticTokensRangeResponse(SemanticTokensRangeResponse),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ErrorResponse {
    pub id: Option<NumberOrString>,
    pub error: ResponseError,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct FindReferencesResponse {
    pub id: NumberOrString,
    pub result: Option<Vec<Location>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoToDefinitionResponse {
    pub id: NumberOrString,
    pub result: Option<LocationResult>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticTokensFullResponse {
    pub id: NumberOrString,
    pub result: Option<SemanticTokens>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticTokensRangeResponse {
    pub id: NumberOrString,
    pub result: Option<SemanticTokens>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LocationResult {
    Single(Location),
    Multi(Vec<Location>),
    ExtMeta(Vec<LocationLink>),
}

impl ServerCapabilities {
    pub fn build(semantic_tok_legend: SemanticTokensLegend) -> Self {
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
            definition_provider: Some(DefinitionProvider::DefinitionOptions(DefinitionOptions {
                work_done_progress: None,
            })),
            type_definition_provider: None,
            implementation_provider: None,
            references_provider: Some(ReferencesProvider::ReferenceOptions(ReferenceOptions {
                work_done_progress: None,
            })),
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
            semantic_tokens_provider: if semantic_tok_legend.is_empty() {
                None
            } else {
                Some(SemanticTokensProvider::SemanticTokensRegistrationOptions(
                    SemanticTokensRegistrationOptions {
                        document_selector: Some(vec![DocumentFilter {
                            language: Some(LANGUAGE_ID.to_string()),
                            scheme: Some("file".to_string()),
                            pattern: None,
                        }]),
                        legend: semantic_tok_legend,
                        range: Some(true),
                        full: Some(SemanticTokensFullDocumentCapabilities::Bool(true)),
                        work_done_progress: None,
                        id: None,
                    },
                ))
            },
            moniker_provider: None,
            type_hierarchy_provider: None,
            inline_value_provider: None,
            inlay_hint_provider: None,
            diagnostic_provider: None,
            workspace_symbol_provider: None,
            inline_completion_provider: None,
            text_document: None,
            workspace: Some(WorkspaceServerCapabilities {
                workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                    supported: Some(true),
                    change_notifications: None,
                }),
                file_operations: Some(WorkspaceFileOperations {
                    did_create: None,
                    will_create: None,
                    did_rename: Some(FileOperationRegistrationOptions {
                        filters: vec![
                            FileOperationFilter {
                                scheme: Some("file".to_string()),
                                pattern: FileOperationPattern {
                                    glob: format!("**/*.{{{}}}", SUFFIXES.join(",")),
                                    matches: Some(FileOperationPatternKind::File),
                                    options: Some(FileOperationPatternOptions {
                                        ignore_case: Some(true),
                                    }),
                                },
                            },
                            FileOperationFilter {
                                scheme: Some("file".to_string()),
                                pattern: FileOperationPattern {
                                    glob: "**/*".to_string(),
                                    matches: Some(FileOperationPatternKind::Folder),
                                    options: None,
                                },
                            },
                        ],
                    }),
                    will_rename: None,
                    did_delete: None,
                    will_delete: None,
                }),
            }),
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
