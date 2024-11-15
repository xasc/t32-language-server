// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use serde_json::Value;

type Uri = String;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceOperationKind {
    Create,
    Rename,
    Delete,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureHandlingKind {
    Abort,
    Transactional,
    Undo,
    TextOnlyTransactional,
}

pub enum SymbolKind {
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,
    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,
    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,
    String = 15,
    Number = 16,
    Boolean = 17,
    Array = 18,
    Object = 19,
    Key = 20,
    Null = 21,
    EnumMember = 22,
    Struct = 23,
    Event = 24,
    Operator = 25,
    TypeParameter = 26,
}

pub enum SymbolTag {
    Deprecated = 1,
}

pub struct InitializeParams {
    process_id: Option<u32>,
    client_info: Option<ClientInfo>,
    locale: String,
    root_path: Option<String>,
    root_uri: Uri,
    initialization_options: Option<Value>,
    capabilities: ClientCapabilities,
}

pub struct ClientInfo {
    name: String,
    version: Option<String>,
}

pub struct ClientCapabilities {
    workspace: Option<WorkspaceClientCapabilities>,
}

pub struct WorkspaceClientCapabilities {
    apply_edit: Option<bool>,
    workspace_edit: Option<WorkspaceEditClientCapabilities>,
    did_change_configuration: Option<DidChangeConfigurationClientCapabilities>,
    did_change_watched_files: Option<DidChangeWatchedFilesClientCapabilities>,
    symbol: Option<WorkspaceSymbolClientCapabilities>,
    executeCommand: Option<ExecuteCommandClientCapabilities>,
}

pub struct WorkspaceEditClientCapabilities {
    document_changes: Option<bool>,
    resource_operations: Option<Vec<ResourceOperationKind>>,
    failure_handling: Option<FailureHandlingKind>,
    normalizes_line_endings: Option<bool>,
    change_annotation_support: Option<ChangeAnnotationWorkspaceEditClientCapabilities>,
}

pub struct ChangeAnnotationWorkspaceEditClientCapabilities {
    groups_on_label: Option<bool>,
}

pub struct DidChangeConfigurationClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct DidChangeWatchedFilesClientCapabilities {
    dynamic_registration: Option<bool>,
    relative_pattern_support: Option<bool>,
}

pub struct WorkspaceSymbolClientCapabilities {
    dynamic_registration: Option<bool>,
    symbol_kind: Option<SymbolKindWorkspaceSymbolClientCapabilities>,
    tag_support: Option<TagSupportWorkspaceSymbolClientCapabilities>,
    resolve_support: Option<ResolveSupportWorkspaceSymbolClientCapabilities>,
}

pub struct SymbolKindWorkspaceSymbolClientCapabilities {
    value_set: Option<Vec<SymbolKind>>,
}

pub struct TagSupportWorkspaceSymbolClientCapabilities {
    value_set: Vec<SymbolTag>,
}

pub struct ResolveSupportWorkspaceSymbolClientCapabilities {
    properties: Vec<String>,
}

pub struct ExecuteCommandClientCapabilities {

}
