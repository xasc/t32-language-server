// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_repr::{Deserialize_repr, Serialize_repr};

type DocumentUri = String;
type DocumentSelector = Vec<DocumentFilter>;
type ProgressToken = NumberOrString;
type Uri = String;

pub enum ErrorCodes {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,

    #[allow(dead_code)]
    InternalError = -32603,
    ServerNotInitialized = -32002,

    #[allow(dead_code)]
    UnknownErrorCode = -32001,

    #[allow(dead_code)]
    RequestFailed = -32803,

    #[allow(dead_code)]
    ServerCancelled = -32802,

    #[allow(dead_code)]
    ContentModified = -32801,

    #[allow(dead_code)]
    RequestCancelled = -32800,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceOperationKind {
    Create,
    Rename,
    Delete,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureHandlingKind {
    Abort,
    Transactional,
    Undo,
    TextOnlyTransactional,
}

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
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

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum SymbolTag {
    Deprecated = 1,
}

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum CompletionItemTag {
    Deprecated = 1,
}

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum InsertTextMode {
    AsIs = 1,
    AdjustIndentation = 2,
}

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
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

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum CodeActionTag {
    LlmGenerated = 1,
}

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum PrepareSupportDefaultBehavior {
    Identifier = 1,
}

#[derive(Debug, Deserialize_repr)]
#[repr(u8)]
pub enum DiagnosticTag {
    Unnecessary = 1,
    Deprecated = 2,
}

#[allow(dead_code)]
#[derive(Serialize_repr)]
#[repr(u8)]
pub enum InitializeErrorCodes {
    UnknownProtocolVersion = 1,
}

#[allow(dead_code)]
#[derive(Serialize_repr)]
#[repr(u8)]
pub enum TextDocumentSyncKind {
    None = 0,
    Full = 1,
    Incremental = 2,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkupKind {
    PlainText,
    Markdown,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum CodeActionKind {
    #[serde(rename(deserialize = ""))]
    Empty,

    #[serde(rename(deserialize = "quickfix"))]
    QuickFix,

    #[serde(rename(deserialize = "refactor"))]
    Refactor,

    #[serde(rename(deserialize = "refactor.extract"))]
    RefactorExtract,

    #[serde(rename(deserialize = "refactor.inline"))]
    RefactorInline,

    #[serde(rename(deserialize = "refactor.move"))]
    RefactorMove,

    #[serde(rename(deserialize = "refactor.rewrite"))]
    RefactorRewrite,

    #[serde(rename(deserialize = "source"))]
    Source,

    #[serde(rename(deserialize = "source.organizeImports"))]
    SourceOrganizeImports,

    #[serde(rename(deserialize = "source.fixAll"))]
    SourceFixAll,

    #[serde(rename(deserialize = "notebook"))]
    Notebook,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoldingRangeKind {
    Comment,
    Imports,
    Region,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum SemanticTokenFullRequestsCapabilities {
    Bool(bool),
    Delta {
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<bool>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenFormat {
    Relative,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PositionEncodingKind {
    #[serde(rename = "utf-8")]
    Utf8,

    #[serde(rename = "utf-16")]
    Utf16,

    #[serde(rename = "utf-32")]
    Utf32,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceValue {
    Off,
    Messages,
    Verbose,
}

#[allow(dead_code)]
pub enum ProgressTokenKind {
    Number(i32),
    String(String),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum NumberOrString {
    Number(i64),
    String(String),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TextDocumentSyncServerCapabilities {
    TextDocumentSyncOptions,
    TextDocumentSyncKind,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NotebookDocumentSyncServerCapabilities {
    NotebookDocumentSyncOptions(NotebookDocumentSyncOptions),
    NotebookDocumentSyncRegistrationOptions(NotebookDocumentSyncRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NotebookSelector {
    NotebookSelectorByNotebook(NotebookSelectorByNotebook),
    NotebookSelectorByCell(NotebookSelectorByCell),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NotebookSelectorNotebook {
    String(String),
    NotebookDocumentFilter,
}

#[allow(dead_code)]
#[derive(Serialize)]
#[serde(untagged)]
pub enum NotebookDocumentFilter {
    NotebookDocumentFilterByType(NotebookDocumentFilterByType),
    NotebookDocumentFilterByScheme(NotebookDocumentFilterByScheme),
    NotebookDocumentFilterByPattern(NotebookDocumentFilterByPattern),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum GlobPattern {
    String(String),
    RelativePattern(RelativePattern),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum HoverProvider {
    Bool(bool),
    HoverOptions(HoverOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DeclarationProvider {
    Bool(bool),
    DeclarationOptions(DeclarationOptions),
    DeclarationRegistrationOptions(DeclarationRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DefinitionProvider {
    Bool(bool),
    DefinitionOptions(DefinitionOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TypeDefinitionProvider {
    Bool(bool),
    TypeDefinitionOptions(TypeDefinitionOptions),
    TypeDefinitionRegistrationOptions(TypeDefinitionRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ImplementationProvider {
    Bool(bool),
    ImplementationOptions(ImplementationOptions),
    ImplementationRegistrationOptions(ImplementationRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ReferencesProvider {
    Bool(bool),
    ReferenceOptions(ReferenceOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DocumentHighlightProvider {
    Bool(bool),
    DocumentHighlightOptions(DocumentHighlightOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DocumentSymbolProvider {
    Bool(bool),
    DocumentSymbolOptions(DocumentSymbolOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CodeActionProvider {
    Bool(bool),
    CodeActionOptions(CodeActionOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ColorProvider {
    Bool(bool),
    DocumentColorOptions(DocumentColorOptions),
    DocumentColorRegistrationOptions(DocumentColorRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DocumentFormattingProvider {
    Bool(bool),
    DocumentFormattingOptions(DocumentFormattingOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DocumentRangeFormattingProvider {
    Bool(bool),
    DocumentRangeFormattingOptions(DocumentRangeFormattingOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RenameProvider {
    Bool(bool),
    RenameOptions(RenameOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FoldingRangeProvider {
    Bool(bool),
    FoldingRangeOptions(FoldingRangeOptions),
    FoldingRangeRegistrationOptions(FoldingRangeRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SelectionRangeProvider {
    Bool(bool),
    SelectionRangeOptions(SelectionRangeOptions),
    SelectionRangeRegistrationOptions(SelectionRangeRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LinkedEditingRangeProvider {
    Bool(bool),
    LinkedEditingRangeOptions(LinkedEditingRangeOptions),
    LinkedEditingRangeRegistrationOptions(LinkedEditingRangeRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CallHierarchyProvider {
    Bool(bool),
    CallHierarchyOptions(CallHierarchyOptions),
    CallHierarchyRegistrationOptions(CallHierarchyRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SemanticTokensProvider {
    SemanticTokensOptions(SemanticTokensOptions),
    SemanticTokensRegistrationOptions(SemanticTokensRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SemanticTokensFullDocumentCapabilities {
    Bool(bool),
    Delta {
        #[serde(skip_serializing_if = "Option::is_none")]
        delta: Option<bool>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MonikerProvider {
    Bool(bool),
    MonikerOptions(MonikerOptions),
    MonikerRegistrationOptions(MonikerRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TypeHierarchyProvider {
    Bool(bool),
    TypeHierarchyOptions(TypeHierarchyOptions),
    TypeHierarchyRegistrationOptions(TypeHierarchyRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InlineValueProvider {
    Bool(bool),
    InlineValueOptions(InlineValueOptions),
    InlineValueRegistrationOptions(InlineValueRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InlayHintProvider {
    Bool(bool),
    InlayHintOptions(InlayHintOptions),
    InlayHintRegistrationOptions(InlayHintRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DiagnosticProvider {
    DiagnosticOptions(DiagnosticOptions),
    DiagnosticRegistrationOptions(DiagnosticRegistrationOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WorkspaceSymbolProvider {
    Bool(bool),
    WorkspaceSymbolOptions(WorkspaceSymbolOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InlineCompletionProvider {
    Bool(bool),
    InlineCompletionOptions(InlineCompletionOptions),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ChangeNotifications {
    String(String),
    Bool(bool),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileOperationPatternKind {
    File,
    Folder,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct InitializedParams {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub process_id: Option<i64>,

    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,

    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,

    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,

    #[allow(dead_code)]
    pub root_uri: Option<Uri>,

    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<Value>,

    #[allow(dead_code)]
    pub capabilities: ClientCapabilities,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceValue>,

    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,

    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_done_token: Option<WorkDoneProgressParams>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ClientInfo {
    name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_document: Option<TextDocumentClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_document: Option<NotebookDocumentClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub general: Option<GeneralClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    apply_edit: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_edit: Option<WorkspaceEditClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_change_configuration: Option<DidChangeConfigurationClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_change_watched_files: Option<DidChangeWatchedFilesClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<WorkspaceSymbolClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    execute_command: Option<ExecuteCommandClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_folders: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    configuration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_tokens: Option<SemanticTokensWorkspaceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    code_lens: Option<CodeLensWorkspaceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    file_operations: Option<FileOperationsWorkspaceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    inline_value: Option<InlineValueWorkspaceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    inlay_hint: Option<InlayHintWorkspaceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<DiagnosticWorkspaceClientCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEditClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    document_changes: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resource_operations: Option<Vec<ResourceOperationKind>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    failure_handling: Option<FailureHandlingKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    normalizes_line_endings: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    change_annotation_support: Option<ChangeAnnotation>,

    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    snippet_edit_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAnnotation {
    #[serde(skip_serializing_if = "Option::is_none")]
    groups_on_label: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeConfigurationClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeWatchedFilesClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    relative_pattern_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSymbolClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    symbol_kind: Option<SymbolKindCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tag_support: Option<SymbolTagSupportCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_support: Option<ResolveSupportWorkspaceSymbolClientCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolKindCapabilities {
    value_set: Option<Vec<SymbolKind>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolTagSupportCapabilities {
    value_set: Vec<SymbolTag>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ResolveSupportWorkspaceSymbolClientCapabilities {
    properties: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteCommandClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensWorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensWorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationsWorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_create: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_create: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_rename: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_rename: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_delete: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_delete: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineValueWorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintWorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticWorkspaceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    synchronization: Option<TextDocumentSyncClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    filters: Option<TextDocumentFilterClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    completion: Option<CompletionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    hover: Option<HoverClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    signature_help: Option<SignatureHelpClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    declaration: Option<DeclarationClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<DefinitionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    type_definition: Option<TypeDefinitionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    implementation: Option<ImplementationClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    references: Option<ReferenceClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    document_highlight: Option<DocumentHighlightClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    document_symbol: Option<DocumentSymbolClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    code_action: Option<CodeActionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    code_lens: Option<CodeLensClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    document_link: Option<DocumentLinkClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    color_provider: Option<DocumentColorClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    formatting: Option<DocumentFormattingClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    range_formatting: Option<DocumentRangeFormattingClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    on_type_formatting: Option<DocumentOnTypeFormattingClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    rename: Option<RenameClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    publish_diagnostics: Option<PublishDiagnosticsClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    folding_range: Option<FoldingRangeClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    selection_range: Option<SelectionRangeClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    linked_editing_range: Option<LinkedEditingRangeClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    call_hierarchy: Option<CallHierarchyClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_tokens: Option<SemanticTokensClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    moniker: Option<MonikerClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    type_hierarchy: Option<TypeHierarchyClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    inline_value: Option<InlineValueClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    inlay_hint: Option<InlayHintClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<DiagnosticClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    inline_completion: Option<InlineCompletionClientCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentSyncClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_save: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_save_wait_until: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_save: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentFilterClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    relative_pattern_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    completion_item: Option<CompletionItemCompletionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    completion_item_kind: Option<CompletionItemKindCompletionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    context_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    insert_text_mode: InsertTextMode,

    #[serde(skip_serializing_if = "Option::is_none")]
    completion_list: Option<CompletionListCompletionClientCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItemCompletionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    commit_characters_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    documentation_format: Option<Vec<MarkupKind>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    deprecated_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    preselect_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tag_support: Option<TagSupportCompletionItemCompletionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    insert_replace_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_support: Option<ResolveSupportCompletionItemCompletionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    insert_text_mode_support:
        Option<InsertTextModeSupportCompletionItemCompletionClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    label_details_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSupportCompletionItemCompletionClientCapabilities {
    value_set: Vec<CompletionItemTag>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ResolveSupportCompletionItemCompletionClientCapabilities {
    properties: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertTextModeSupportCompletionItemCompletionClientCapabilities {
    value_set: Vec<InsertTextMode>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItemKindCompletionClientCapabilities {
    value_set: Vec<CompletionItemKind>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionListCompletionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    item_defaults: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    apply_kind_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    content_format: Option<MarkupKind>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelpClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    signature_information: Option<SignatureInformationCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    context_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInformationCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation_format: Option<Vec<MarkupKind>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    parameter_information: Option<ParameterInformationCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    active_parameter_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    no_active_parameter_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterInformationCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    label_offset_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    link_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    link_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDefinitionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    link_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplementationClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    link_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentHighlightClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbolClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    symbol_kind: Option<SymbolKindCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    hierarchical_document_symbol_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tag_support: Option<SymbolTagSupportCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    label_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    code_action_literal_support: Option<CodeActionLiteralSupportCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    is_preferred_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    disabled_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    data_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_support: Option<ResolveSupportCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    honors_change_annotations: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    documentation_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tag_support: Option<CodeActionTagSupportCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionLiteralSupportCapabilities {
    code_action_kind: CodeActionKindCapabilities,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionKindCapabilities {
    value_set: Vec<CodeActionKind>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSupportCapabilities {
    properties: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionTagSupportCapabilities {
    value_set: Vec<CodeActionTag>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_support: Option<ClientCodeLensResolveOptions>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCodeLensResolveOptions {
    properties: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLinkClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentColorClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFormattingClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRangeFormattingClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    ranges_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentOnTypeFormattingClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    prepare_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    prepare_support_default_behavior: Option<PrepareSupportDefaultBehavior>,

    #[serde(skip_serializing_if = "Option::is_none")]
    honors_change_annotations: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDiagnosticsClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    related_information: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    tag_support: Option<DiagnosticTagSupportCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    version_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    code_description_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    data_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticTagSupportCapability {
    value_set: Vec<DiagnosticTag>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldingRangeClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    range_limit: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    line_folding_only: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    folding_range_kind: Option<FoldingRangeKindCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    folding_range: Option<FoldingRangeCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldingRangeKindCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    value_set: Vec<FoldingRangeKind>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldingRangeCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    collapsed_text: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionRangeClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedEditingRangeClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
    requests: Option<SemanticTokenRequestsCapabilities>,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
    formats: Vec<TokenFormat>,

    #[serde(skip_serializing_if = "Option::is_none")]
    overlapping_token_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    multiline_token_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    server_cancel_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    augments_syntax_tokens: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokenRequestsCapabilities {
    range: Option<bool>,
    full: Option<SemanticTokenFullRequestsCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonikerClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeHierarchyClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineValueClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_support: Option<ResolveSupportCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    related_document_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    markup_message_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineCompletionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentClientCapabilities {
    synchronization: NotebookDocumentSyncClientCapabilities,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookDocumentSyncClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    dynamic_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    execution_summary_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    show_message: Option<ShowMessageRequestClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    show_document: Option<ShowDocumentClientCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowMessageRequestClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    message_action_item: Option<MessageActionItemClientCapabilities>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageActionItemClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_properties_support: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowDocumentClientCapabilities {
    support: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    stale_request_support: Option<StaleRequestSupportClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    regular_expressions: Option<RegularExpressionsClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    markdown: Option<MarkdownClientCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    position_encodings: Option<Vec<PositionEncodingKind>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleRequestSupportClientCapabilities {
    cancel: bool,
    retry_on_content_modified: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegularExpressionsClientCapabilities {
    engine: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownClientCapabilities {
    parser: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceFolder {
    uri: Uri,
    name: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDoneProgressParams {
    work_done_token: Option<ProgressToken>,
}

#[allow(dead_code)]
pub struct ProgressParams {
    token: ProgressTokenKind,
    value: Value,
}

#[derive(Serialize)]
pub struct TextDocumentSyncOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    open_close: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    change: Option<TextDocumentSyncKind>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotebookDocumentSyncOptions {
    notebook_selector: Vec<NotebookSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    save: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotebookDocumentSyncRegistrationOptions {
    notebook_selector: Vec<NotebookSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    save: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotebookSelectorByNotebook {
    notebook: NotebookSelectorNotebook,

    #[serde(skip_serializing_if = "Option::is_none")]
    cells: Option<Vec<NotebookSelectorCell>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotebookSelectorByCell {
    #[serde(skip_serializing_if = "Option::is_none")]
    notebook: Option<NotebookSelectorNotebook>,

    cells: Vec<NotebookSelectorCell>,
}

#[derive(Serialize)]
pub struct NotebookDocumentFilterByType {
    notebook_type: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    scheme: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<GlobPattern>,
}

#[derive(Serialize)]
pub struct NotebookDocumentFilterByScheme {
    #[serde(skip_serializing_if = "Option::is_none")]
    notebook_type: Option<String>,

    scheme: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<GlobPattern>,
}

#[derive(Serialize)]
pub struct NotebookDocumentFilterByPattern {
    #[serde(skip_serializing_if = "Option::is_none")]
    notebook_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    scheme: Option<String>,
    pattern: GlobPattern,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RelativePattern {
    base_uri: WorkspaceFolder,
    pattern: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotebookSelectorCell {
    language: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompletionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger_characters: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    all_commit_characters: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    completion_item: Option<CompletionOptionsCompletionItemCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompletionOptionsCompletionItemCapability {
    label_details_support: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HoverOptions {
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SignatureHelpOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger_characters: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    retrigger_characters: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeclarationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeclarationRegistrationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    scheme: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<GlobPattern>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DefinitionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeDefinitionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeDefinitionRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImplementationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImplementationRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReferenceOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentHighlightOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentSymbolOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CodeActionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    code_action_kinds: Option<Vec<CodeActionKind>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<Vec<CodeActionKindDocumentation>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CodeActionKindDocumentation {
    kind: CodeActionKind,
    command: Command,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Command {
    title: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip: Option<String>,
    command: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CodeLensOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentLinkOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentColorOptions {
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentColorRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentFormattingOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentRangeFormattingOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    ranges_support: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DocumentOnTypeFormattingOptions {
    first_trigger_character: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    more_trigger_character: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RenameOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    prepare_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FoldingRangeOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FoldingRangeRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExecuteCommandOptions {
    commands: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SelectionRangeOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SelectionRangeRegistrationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LinkedEditingRangeOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LinkedEditingRangeRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CallHierarchyOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CallHierarchyRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticTokensOptions {
    legend: SemanticTokensLegend,

    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    full: Option<SemanticTokensFullDocumentCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticTokensLegend {
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SemanticTokensRegistrationOptions {
    document_selector: Option<DocumentSelector>,
    legend: SemanticTokensLegend,

    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    full: Option<SemanticTokensFullDocumentCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MonikerOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MonikerRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeHierarchyOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TypeHierarchyRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InlineValueOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InlineValueRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InlayHintOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InlayHintRegistrationOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiagnosticOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    identifier: Option<String>,
    inter_file_dependencies: bool,
    workspace_diagnostics: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiagnosticRegistrationOptions {
    document_selector: Option<DocumentSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    identifier: Option<String>,
    inter_file_dependencies: bool,
    workspace_diagnostics: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceSymbolOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    resolve_provider: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InlineCompletionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    work_done_progress: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TextDocumentServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic: Option<TextDocumentDiagnosticServerCapabilities>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TextDocumentDiagnosticServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    markup_message_support: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_folders: Option<WorkspaceFoldersServerCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    file_operations: Option<WorkspaceFileOperations>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceFoldersServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    supported: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    change_notifications: Option<ChangeNotifications>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceFileOperations {
    #[serde(skip_serializing_if = "Option::is_none")]
    did_create: Option<FileOperationRegistrationOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_create: Option<FileOperationRegistrationOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_rename: Option<FileOperationRegistrationOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_rename: Option<FileOperationRegistrationOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    did_delete: Option<FileOperationRegistrationOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    will_delete: Option<FileOperationRegistrationOptions>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileOperationRegistrationOptions {
    filters: Vec<FileOperationFilter>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileOperationFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    scheme: Option<String>,

    pattern: FileOperationPattern,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileOperationPattern {
    glob: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    matches: Option<FileOperationPatternKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<FileOperationPatternOptions>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileOperationPatternOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    ignore_case: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

#[derive(Serialize)]
pub struct InitializeError {
    pub retry: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerInfo {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_encoding: Option<PositionEncodingKind>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_document_sync: Option<TextDocumentSyncServerCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_document_sync: Option<NotebookDocumentSyncServerCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_provider: Option<CompletionOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover_provider: Option<HoverProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_help_provider: Option<SignatureHelpOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration_provider: Option<DeclarationProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_provider: Option<DefinitionProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_definition_provider: Option<TypeDefinitionProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementation_provider: Option<ImplementationProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_provider: Option<ReferencesProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_highlight_provider: Option<DocumentHighlightProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_symbol_provider: Option<DocumentSymbolProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_action_provider: Option<CodeActionProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_lens_provider: Option<CodeLensOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_link_provider: Option<DocumentLinkOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_provider: Option<ColorProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_formatting_provider: Option<DocumentFormattingProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_range_formatting_provider: Option<DocumentRangeFormattingProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_on_type_formatting_provider: Option<DocumentOnTypeFormattingOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename_provider: Option<RenameProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub folding_range_provider: Option<FoldingRangeProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub execute_command_provider: Option<ExecuteCommandOptions>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_range_provider: Option<SelectionRangeProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_editing_range_provider: Option<LinkedEditingRangeProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_hierarchy_provider: Option<CallHierarchyProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_tokens_provider: Option<SemanticTokensProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub moniker_provider: Option<MonikerProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_hierarchy_provider: Option<TypeHierarchyProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_value_provider: Option<InlineValueProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub inlay_hint_provider: Option<InlayHintProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_provider: Option<DiagnosticProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_symbol_provider: Option<WorkspaceSymbolProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_completion_provider: Option<InlineCompletionProvider>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_document: Option<TextDocumentServerCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceServerCapabilities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct SetTraceParams {
    pub value: TraceValue,
}

#[derive(Debug, Serialize)]
pub struct LogTraceParams {
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DidOpenTextDocumentParams {
    pub text_document: TextDocumentItem,
}

#[derive(Debug, Deserialize)]
pub struct TextDocumentItem {
    pub uri: DocumentUri,
    pub language_id: String,
    pub version: i64,
    pub text: String,
}
