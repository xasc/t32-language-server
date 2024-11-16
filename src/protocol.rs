// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize};
use serde_json::Value;

type Uri = String;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceOperationKind {
    Create,
    Rename,
    Delete,
}

#[derive(Deserialize)]
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

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkupKind {
    PlainText,
    Markdown,
}

pub enum CompletionItemTag {
    Deprecated = 1,
}

pub enum InsertTextMode {
    AsIs = 1,
    AdjustIndentation = 2,
}

pub enum CompletionItemKind {
    Text = 1,
    Method = 2,
    Function = 3,
    Constructor = 4,
    Field = 5,
    Variable = 6,
    Class = 7,
    Interface = 8,
    Module = 9,
    Property = 10,
    Unit = 11,
    Value = 12,
    Enum = 13,
    Keyword = 14,
    Snippet = 15,
    Color = 16,
    File = 17,
    Reference = 18,
    Folder = 19,
    EnumMember = 20,
    Constant = 21,
    Struct = 22,
    Event = 23,
    Operator = 24,
    TypeParameter = 25,
}

pub enum CodeActionKind {
    Empty,
    QuickFix,
    Refactor,
    RefactorExtract,
    RefactorInline,
    RefactorMove,
    RefactorRewrite,
    Source,
    SourceOrganizeImports,
    SourceFixAll,
    Notebook,
}

pub enum CodeActionTag {
    LlmGenerated = 1,
}

pub enum PrepareSupportDefaultBehavior {
    Identifier = 1,
}

pub enum DiagnosticTag {
    Unnecessary = 1,
    Deprecated = 2,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoldingRangeKind {
    Comment,
    Imports,
    Region,
}

pub enum SemanticTokenFullRequestsCapabilities {
    Bool(bool),
    Delta { delta: Option<bool> },
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenFormat {
    Relative,
}

pub enum PositionEncodingKind {
    Utf8,
    Utf16,
    Utf32,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceValue {
    Off,
    Messages,
    Verbose
}

pub enum ProgressTokenKind {
    Number(i32),
    String(String),
}

pub enum Id {
    Number(i64),
    String(String),
    Null,
}

pub enum ErrorCode {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
    ServerNotInitialized = -32002,
    UnknownErrorCode = -32001,
    RequestFailed = -32803,
    ServerCancelled = -32802,
    ContentModified = -32801,
    RequestCancelled = -32800,
}

pub struct RequestMessage {
    jsonrpc: String,
    id: Id,
    method: String,
    params: Option<Value>,
}

pub struct ResponseMessage {
    jsonrpc: String,
    id: Id,
    result: Option<Value>,
    error: Option<ResponseError>,
}

pub struct ResponseError {
    code: i64,
    message: i64,
    data: Option<Value>,
}

pub struct NotificationMessage {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
}

pub struct InitializeParams {
    process_id: Option<u32>,
    client_info: Option<ClientInfo>,
    locale: String,
    root_path: Option<String>,
    root_uri: Uri,
    initialization_options: Option<Value>,
    capabilities: ClientCapabilities,
    trace: Option<TraceValue>,
    workspace_folders: Option<Vec<WorkspaceFolder>>,
    work_done_token: Option<WorkDoneProgressParams>,
}

pub struct ClientInfo {
    name: String,
    version: Option<String>,
}

pub struct ClientCapabilities {
    workspace: Option<WorkspaceClientCapabilities>,
    text_document: Option<TextDocumentClientCapabilities>,
    notebook_document: Option<NotebookDocumentClientCapabilities>,
    window: Option<WindowClientCapabilities>,
    general: Option<GeneralClientCapabilities>,
}

pub struct WorkspaceClientCapabilities {
    apply_edit: Option<bool>,
    workspace_edit: Option<WorkspaceEditClientCapabilities>,
    did_change_configuration: Option<DidChangeConfigurationClientCapabilities>,
    did_change_watched_files: Option<DidChangeWatchedFilesClientCapabilities>,
    symbol: Option<WorkspaceSymbolClientCapabilities>,
    execute_command: Option<ExecuteCommandClientCapabilities>,
}

pub struct WorkspaceEditClientCapabilities {
    document_changes: Option<bool>,
    resource_operations: Option<Vec<ResourceOperationKind>>,
    failure_handling: Option<FailureHandlingKind>,
    normalizes_line_endings: Option<bool>,
    change_annotation_support: Option<ChangeAnnotation>,
    workspace_folders: Option<bool>,
    configuration: Option<bool>,
    semantic_tokens: Option<SemanticTokensWorkspaceClientCapabilities>,
    code_lens: Option<CodeLensWorkspaceClientCapabilities>,
    file_operations: Option<FileOperationsWorkspaceClientCapabilities>,
    inline_value: Option<InlineValueWorkspaceClientCapabilities>,
    inlay_hint: Option<InlineValueWorkspaceClientCapabilities>,
    diagnostics: Option<DiagnosticWorkspaceClientCapabilities>,
}

pub struct ChangeAnnotation {
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
    symbol_kind: Option<SymbolKindCapabilities>,
    tag_support: Option<SymbolTagSupportCapabilities>,
    resolve_support: Option<ResolveSupportWorkspaceSymbolClientCapabilities>,
}

pub struct SymbolKindCapabilities {
    value_set: Option<Vec<SymbolKind>>,
}

pub struct SymbolTagSupportCapabilities {
    value_set: Vec<SymbolTag>,
}

pub struct ResolveSupportWorkspaceSymbolClientCapabilities {
    properties: Vec<String>,
}

pub struct ExecuteCommandClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct SemanticTokensWorkspaceClientCapabilities {
    refresh_support: Option<bool>,
}

pub struct CodeLensWorkspaceClientCapabilities {
    refresh_support: Option<bool>,
}

pub struct FileOperationsWorkspaceClientCapabilities {
    dynamic_registration: Option<bool>,
    did_create: Option<bool>,
    will_create: Option<bool>,
    did_rename: Option<bool>,
    will_rename: Option<bool>,
    did_delete: Option<bool>,
    will_delete: Option<bool>,
}

pub struct InlineValueWorkspaceClientCapabilities {
    refresh_support: Option<bool>,
}

pub struct InlayHintWorkspaceClientCapabilities {
    refresh_support: Option<bool>,
}

pub struct DiagnosticWorkspaceClientCapabilities {
    refresh_support: Option<bool>,
}

pub struct TextDocumentClientCapabilities {
    synchronization: Option<TextDocumentSyncClientCapabilities>,
    filters: Option<TextDocumentFilterClientCapabilities>,
    completion: Option<CompletionClientCapabilities>,
    hover: Option<HoverClientCapabilities>,
    signature_help: Option<SignatureHelpClientCapabilities>,
    declaration: Option<DeclarationClientCapabilities>,
    definition: Option<DefinitionClientCapabilities>,
    type_definition: Option<TypeDefinitionClientCapabilities>,
    implementation: Option<ImplementationClientCapabilities>,
    references: Option<ReferenceClientCapabilities>,
    document_highlight: Option<DocumentHighlightClientCapabilities>,
    document_symbol: Option<DocumentSymbolClientCapabilities>,
    code_action: Option<CodeActionClientCapabilities>,
    code_lens: Option<CodeLensClientCapabilities>,
    document_link: Option<DocumentLinkClientCapabilities>,
    color_provider: Option<DocumentColorClientCapabilities>,
    formatting: Option<DocumentFormattingClientCapabilities>,
    range_formatting: Option<DocumentRangeFormattingClientCapabilities>,
    on_type_formatting: Option<DocumentOnTypeFormattingClientCapabilities>,
    rename: Option<RenameClientCapabilities>,
    publish_diagnostics: Option<PublishDiagnosticsClientCapabilities>,
    folding_range: Option<FoldingRangeClientCapabilities>,
    selection_range: Option<SelectionRangeClientCapabilities>,
    linked_editing_range: Option<LinkedEditingRangeClientCapabilities>,
    call_hierarchy: Option<CallHierarchyClientCapabilities>,
    semantic_tokens: Option<SemanticTokensClientCapabilities>,
    moniker: Option<MonikerClientCapabilities>,
    type_hierarchy: Option<TypeHierarchyClientCapabilities>,
    inline_value: Option<InlineValueClientCapabilities>,
    inlay_hint: Option<InlayHintClientCapabilities>,
    diagnostic: Option<DiagnosticClientCapabilities>,
    inline_completion: Option<InlineCompletionClientCapabilities>,
}

pub struct TextDocumentSyncClientCapabilities {
    dynamic_registration: Option<bool>,
    will_save: Option<bool>,
    will_save_wait_until: Option<bool>,
    did_save: Option<bool>,
}

pub struct TextDocumentFilterClientCapabilities {
    relative_pattern_support: Option<bool>,
}

pub struct CompletionClientCapabilities {
    dynamic_registration: Option<bool>,
    completion_item: Option<CompletionItemCompletionClientCapabilities>,
    completion_item_kind: Option<CompletionItemKindCompletionClientCapabilities>,
    context_support: Option<bool>,
    insert_text_mode: InsertTextMode,
    completion_list: Option<CompletionListCompletionClientCapabilities>,
}

pub struct CompletionItemCompletionClientCapabilities {
    snippet_support: Option<bool>,
    commit_characters_support: Option<bool>,
    documentation_format: Option<Vec<MarkupKind>>,
    deprecated_support: Option<bool>,
    preselect_support: Option<bool>,
    tag_support: Option<TagSupportCompletionItemCompletionClientCapabilities>,
    insert_replace_support: Option<bool>,
    resolve_support: Option<ResolveSupportCompletionItemCompletionClientCapabilities>,
    insert_text_mode_support: Option<InsertTextModeSupportCompletionItemCompletionClientCapabilities>,
    label_details_support: Option<bool>,
}

pub struct TagSupportCompletionItemCompletionClientCapabilities {
    value_set: Vec<CompletionItemTag>,
}

pub struct ResolveSupportCompletionItemCompletionClientCapabilities {
    properties: Vec<String>,
}

pub struct InsertTextModeSupportCompletionItemCompletionClientCapabilities {
    value_set: Vec<InsertTextMode>,
}

pub struct CompletionItemKindCompletionClientCapabilities {
    value_set: Vec<CompletionItemKind>,
}

pub struct CompletionListCompletionClientCapabilities {
    item_defaults: Option<String>,
    apply_kind_support: Option<bool>,
}

pub struct HoverClientCapabilities {
    dynamic_registration: Option<bool>,
    content_format: Option<MarkupKind>,
}

pub struct SignatureHelpClientCapabilities {
    dynamic_registration: Option<bool>,
    signature_information: Option<SignatureInformationCapabilities>,
    context_support: Option<bool>,
}

pub struct SignatureInformationCapabilities {
    documentation_format: Option<Vec<MarkupKind>>,
    parameter_information: Option<ParameterInformationCapabilities>,
    active_parameter_support: Option<bool>,
    no_active_parameter_support: Option<bool>,
}

pub struct ParameterInformationCapabilities {
    label_offset_support: Option<bool>,
}

pub struct DeclarationClientCapabilities {
    dynamic_registration: Option<bool>,
    link_support: Option<bool>,
}

pub struct DefinitionClientCapabilities {
    dynamic_registration: Option<bool>,
    link_support: Option<bool>,
}

pub struct TypeDefinitionClientCapabilities {
    dynamic_registration: Option<bool>,
    link_support: Option<bool>,
}

pub struct ImplementationClientCapabilities {
    dynamic_registration: Option<bool>,
    link_support: Option<bool>,
}

pub struct ReferenceClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct DocumentHighlightClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct DocumentSymbolClientCapabilities {
    dynamic_registration: Option<bool>,
    symbol_kind: Option<SymbolKindCapabilities>,
    hierarchical_document_symbol_support: Option<bool>,
    tag_support: Option<SymbolTagSupportCapabilities>,
    label_support: Option<bool>,
}

pub struct CodeActionClientCapabilities {
    dynamic_registration: Option<bool>,
    code_action_literal_support: Option<CodeActionLiteralSupportCapabilities>,
    is_preferred_support: Option<bool>,
    disabled_support: Option<bool>,
    data_support: Option<bool>,
    resolve_support: Option<ResolveSupportCapabilities>,
    honors_change_annotations: Option<bool>,
    documentation_support: Option<bool>,
    tag_support: Option<CodeActionTagSupportCapabilities>,
}

pub struct CodeActionLiteralSupportCapabilities {
    code_action_kind: CodeActionKindCapabilities,
}

pub struct CodeActionKindCapabilities {
    value_set: Vec<CodeActionKind>,
}

pub struct ResolveSupportCapabilities {
    properties: Vec<String>,
}

pub struct CodeActionTagSupportCapabilities {
    value_set: Vec<CodeActionTag>,
}

pub struct CodeLensClientCapabilities {
    dynamic_registration: Option<bool>,
    resolve_support: Option<ClientCodeLensResolveOptions>,
}

pub struct ClientCodeLensResolveOptions {
    properties: Vec<String>,
}

pub struct DocumentLinkClientCapabilities {
    dynamic_registration: Option<bool>,
    tooltip_support: Option<bool>,
}

pub struct DocumentColorClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct DocumentFormattingClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct DocumentRangeFormattingClientCapabilities {
    dynamic_registration: Option<bool>,
    ranges_support: Option<bool>,
}

pub struct DocumentOnTypeFormattingClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct RenameClientCapabilities {
    dynamic_registration: Option<bool>,
    prepare_support: Option<bool>,
    prepare_support_default_behavior: Option<PrepareSupportDefaultBehavior>,
    honors_change_annotations: Option<bool>,
}

pub struct PublishDiagnosticsClientCapabilities {
    related_information: Option<bool>,
    tag_support: Option<DiagnosticTagSupportCapability>,
    version_support: Option<bool>,
    code_description_support: Option<bool>,
    data_support: Option<bool>,
}

pub struct DiagnosticTagSupportCapability {
    value_set: Vec<DiagnosticTag>,
}

pub struct FoldingRangeClientCapabilities {
    dynamic_registration: Option<bool>,
    range_limit: Option<u32>,
    line_folding_only: Option<bool>,
    folding_range_kind: Option<FoldingRangeKindCapabilities>,
    folding_range: Option<FoldingRangeCapabilities>,
}

pub struct FoldingRangeKindCapabilities {
    value_set: Vec<FoldingRangeKind>,
}

pub struct FoldingRangeCapabilities {
    collapsed_text: Option<bool>,
}

pub struct SelectionRangeClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct LinkedEditingRangeClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct CallHierarchyClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct SemanticTokensClientCapabilities {
    dynamic_registration: Option<bool>,
    requests: Option<SemanticTokenRequestsCapabilities>,
}

pub struct SemanticTokenRequestsCapabilities {
    range: Option<bool>,
    full: Option<SemanticTokenFullRequestsCapabilities>,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
    formats: Vec<TokenFormat>,
    overlapping_token_support: Option<bool>,
    multiline_token_support: Option<bool>,
    server_cancel_support: Option<bool>,
    augments_syntax_tokens: Option<bool>,
}

pub struct MonikerClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct TypeHierarchyClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct InlineValueClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct InlayHintClientCapabilities {
    dynamic_registration: Option<bool>,
    resolve_support: Option<ResolveSupportCapabilities>,
}

pub struct DiagnosticClientCapabilities {
    dynamic_registration: Option<bool>,
    related_document_support: Option<bool>,
    markup_message_support: Option<bool>,
}

pub struct InlineCompletionClientCapabilities {
    dynamic_registration: Option<bool>,
}

pub struct NotebookDocumentClientCapabilities {
    synchronization: NotebookDocumentSyncClientCapabilities,
}

pub struct NotebookDocumentSyncClientCapabilities {
    dynamic_registration: Option<bool>,
    execution_summary_support: Option<bool>,
}

pub struct WindowClientCapabilities {
    work_done_progress: Option<bool>,
    show_message: Option<ShowMessageRequestClientCapabilities>,
    show_document: Option<ShowDocumentClientCapabilities>,
}

pub struct ShowMessageRequestClientCapabilities {
    message_action_item: Option<MessageActionItemClientCapabilities>,
}

pub struct MessageActionItemClientCapabilities {
    additional_properties_support: Option<bool>,
}

pub struct ShowDocumentClientCapabilities {
    support: bool,
}

pub struct GeneralClientCapabilities {
    stale_request_support: Option<StaleRequestSupportClientCapabilities>,
    regular_expressions: Option<RegularExpressionsClientCapabilities>,
    markdown: Option<MarkdownClientCapabilities>,
    position_encodings: Option<Vec<PositionEncodingKind>>,
    experimental: Option<Value>,
}

pub struct StaleRequestSupportClientCapabilities {
    cancel: bool,
    retry_on_content_modified: Vec<String>,
}

pub struct RegularExpressionsClientCapabilities {
    engine: String,
    version: Option<bool>,
}

pub struct MarkdownClientCapabilities {
    parser: String,
    version: Option<String>,
    allowed_tags: Option<Vec<String>>,
}

pub struct WorkspaceFolder {
    uri: Uri,
    name: String,
}

pub struct WorkDoneProgressParams {
    work_done_token: Option<ProgressToken>,
}

pub struct ProgressToken {
    token: ProgressTokenKind,
    value: Value,
}
