<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

# t32-language-server

Language server for Lauterbach TRACE32® script language.


## Language Server Protocol Support

This language server implements version 3.18 of the language server protocol.

### Lifecycle Messages

| Request                       | Support Status |
| ----------------------------- | -------------- |
| `initialize`                  | ✅             |
| `initialized`                 | ✅             |
| `client/registerCapability`   | ➖             |
| `client/unregisterCapability` | ➖             |
| `$/setTrace`                  | ✅             |
| `$/logTrace`                  | ✅             |
| `shutdown`                    | ✅             |
| `exit`                        | ✅             |

### Document Synchronization

| Request                          | Support Status |
| -------------------------------- | -------------- |
| `textDocument/didOpen`           | ✅             |
| `textDocument/didChange`         | ✅             |
| `textDocument/willSave`          | ➖             |
| `textDocument/willSaveWaitUntil` | ➖             |
| `textDocument/didSave`           | ➖             |
| `textDocument/didClose`          | ✅             |
| `notebookDocument/didOpen`       | ➖             |
| `notebookDocument/didChange`     | ➖             |
| `notebookDocument/didSave`       | ➖             |
| `notebookDocument/didClose`      | ➖             |

### Language Features

| Request                                  | Support Status |
| ---------------------------------------- | -------------- |
| `textDocument/declaration`               | ➖             |
| `textDocument/definition`                | ✅             |
| `textDocument/implementation`            | ➖             |
| `textDocument/references`                | ✅             |
| `textDocument/prepareCallHierarchy`      | ➖             |
| `callHierarchy/incomingCalls`            | ➖             |
| `callHierarchy/outgoingCalls`            | ➖             |
| `textDocument/prepareTypeHierarchy`      | ➖             |
| `typeHierarchy/supertypes`               | ➖             |
| `typeHierarchy/subtypes`                 | ➖             |
| `textDocument/documentHighlight`         | ➖             |
| `documentLink/resolve`                   | ➖             |
| `textDocument/hover`                     | ➖             |
| `textDocument/codeLens`                  | ➖             |
| `codeLens/resolve`                       | ➖             |
| `workspace/codeLens/refresh`             | ➖             |
| `textDocument/foldingRange`              | ➖             |
| `workspace/foldingRange/refresh`         | ➖             |
| `textDocument/selectionRange`            | ➖             |
| `textDocument/documentSymbol`            | ➖             |
| `textDocument/semanticTokens/full`       | ✅             |
| `textDocument/semanticTokens/full/delta` | ➖             |
| `textDocument/semanticTokens/range`      | ✅             |
| `workspace/semanticTokens/refresh`       | ➖             |
| `textDocument/inlineValue`               | ➖             |
| `workspace/inlineValue/refresh`          | ➖             |
| `textDocument/inlayHint`                 | ➖             |
| `inlayHint/resolve`                      | ➖             |
| `workspace/inlayHint/refresh`            | ➖             |
| `textDocument/moniker`                   | ➖             |
| `textDocument/completion`                | ➖             |
| `completionItem/resolve`                 | ➖             |
| `textDocument/publishDiagnostics`        | ➖             |
| `textDocument/diagnostic`                | ➖             |
| `workspace/diagnostic`                   | ➖             |
| `workspace/diagnostic/refresh`           | ➖             |
| `textDocument/signatureHelp`             | ➖             |
| `textDocument/codeAction`                | ➖             |
| `codeAction/resolve`                     | ➖             |
| `textDocument/documentColor`             | ➖             |
| `textDocument/colorPresentation`         | ➖             |
| `textDocument/formatting`                | ➖             |
| `textDocument/rangeFormatting`           | ➖             |
| `textDocument/rangesFormatting`          | ➖             |
| `textDocument/onTypeFormatting`          | ➖             |
| `textDocument/rename`                    | ➖             |
| `textDocument/prepareRename`             | ➖             |
| `textDocument/linkedEditingRange`        | ➖             |
| `textDocument/inlineCompletion`          | ➖             |

### Workspace Features

| Request                                 | Support Status |
| --------------------------------------- | -------------- |
| `workspace/symbol`                      | ➖             |
| `workspaceSymbol/resolve`               | ➖             |
| `workspace/configuration`               | ➖             |
| `workspace/didChangeConfiguration`      | ➖             |
| `workspace/workspaceFolders`            | ➖             |
| `workspace/didChangeWorkspaceFolders`   | ➖             |
| `workspace/willCreateFiles`             | ➖             |
| `workspace/didCreateFiles`              | ➖             |
| `workspace/willRenameFiles`             | ➖             |
| `workspace/didRenameFiles`              | ✅             |
| `workspace/willDeleteFiles`             | ➖             |
| `workspace/didDeleteFiles`              | ➖             |
| `workspace/didChangeWatchedFiles`       | ➖             |
| `workspace/executeCommand`              | ➖             |
| `workspace/applyEdit`                   | ➖             |
| `workspace/textDocumentContent`         | ➖             |
| `workspace/textDocumentContent/refresh` | ➖             |

### Window Features

| Request                           | Support Status |
| --------------------------------- | -------------- |
| `window/showMessage`              | ➖             |
| `window/showMessageRequest`       | ➖             |
| `window/showDocument`             | ➖             |
| `window/logMessage`               | ➖             |
| `window/workDoneProgress/create`  | ➖             |
| `window/workDoneProgress/cancel`  | ➖             |
| `telemetry/event’`                | ➖             |
