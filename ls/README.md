<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

t32-language-server
===================

Language server for the Lauterbach TRACE32® script language.
It is available for Linux, Windows, macOS, and WebAssembly.

<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#features">Features</a>
      <ul>
        <li><a href="#quick-start">Quick start</a></li>
        <ul>
            <li><a href="#dependencies">Dependencies</a></li>
            <li><a href="#installation">Installation</a></li>
        </ul>
      </ul>
    </li>
    <li>
      <a href="#usage">Usage</a>
      <ul>
        <li><a href="#command-line-interface">Command line interface</a></li>
      </ul>
    </li>
    <li><a href="#packages">Packages</a></li>
    <li><a href="#mirrors">Mirrors</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#language-server-protocol-support">Language server protocol support</a></li>
  </ol>
</details>


Features
--------

-  Go to definition for PRACTICE macros and subroutines.
-  Locates PRACTICE macros and file references across all scripts in a project.
-  Semantic token detection for improved syntax highlighting.


Quick start
-----------

### Dependencies

Builds require [Rust] version **1.95** or newer.
These additional dependencies are required:
 -  [libc] [Unix]
 -  [serde]
 -  [serde_json]
 -  [serde_repr]
 -  [tree-sitter]
 -  [tree-sitter-t32]
 -  [url]
 -  [wasi-sdk] [WebAssembly]
 -  [windows-sys] [Windows]

[Rust]: https://rust-lang.org/tools/install
[libc]: https://github.com/rust-lang/libc
[serde]: https://github.com/serde-rs/serde
[serde_json]: https://github.com/serde-rs/json
[serde_repr]: https://github.com/dtolnay/serde-repr
[tree-sitter]: https://github.com/tree-sitter/tree-sitter
[tree-sitter-t32]: https://codeberg.org/xasc/tree-sitter-t32
[url]: https://github.com/servo/rust-url
[wasi-sdk]: https://github.com/WebAssembly/wasi-sdk
[windows-sys]: https://github.com/microsoft/windows-rs

### Installation

#### Using cargo

1.  Install [Rust]
2.  Install from [crates.io]:
    ~~~~ text
    cargo install t32-language-server
    ~~~~~

[crates.io]: https://crates.io/crates/t32-language-server

#### Precompiled binaries

Binary releases for Linux, Windows, macOS, and WebAssembly are available on the
project's releases page.

> [!NOTE]
> Binary releases are not yet available.


Usage
-----

### Command line interface

~~~~ bash
t32ls [OPTIONS]
~~~~
#### General options

~~~~ text
  -h, --help
    Show this help message and exit.

  -c PID, --clientProcessId=PID
    Process ID of the client that started the server. The server can use the
    PID to monitor the client process and shut itself down if the client
    process dies.

  -t LEVEL, --trace=LEVEL
    Set the initial logging level of the server's execution trace. LEVEL must
    be one of 'off,messages,verbose'.

  -V, --version
    Print version info and exit.
~~~~
#### Example

~~~~ bash
t32ls --clientProcessId=42 -t messages
~~~~


Packages
--------

| Registry      | Package                                                              | Download                                              |
| ------------- | -------------------------------------------------------------------- | ----------------------------------------------------- |
| crates.io     | [t32-language-server](https://crates.io/crates/t32-language-server)  |                                                       |


Mirrors
-------

This repository is mirrored to https://github.com/xasc/t32-language-server.
The main repository is https://codeberg.org/xasc/t32-language-server.


License
-------

Distributed under the [European Union Public Licence version 1.2].
[REUSE](https://reuse.software/) is used for managing licensing information throughout the project.
For more accurate licensing information, please check the individual files.

[European Union Public Licence version 1.2]: https://interoperable-europe.ec.europa.eu/sites/default/files/custom-page/attachment/2020-03/EUPL-1.2%20EN.txt


Language server protocol support
--------------------------------

This language server implements [version 3.18 of the language server protocol].

[version 3.18 of the language server protocol]: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.18/specification

### Lifecycle messages

| Method                        | Support status |
| ----------------------------- | -------------- |
| `initialize`                  | ✅             |
| `initialized`                 | ✅             |
| `client/registerCapability`   | ➖             |
| `client/unregisterCapability` | ➖             |
| `$/setTrace`                  | ✅             |
| `$/logTrace`                  | ✅             |
| `shutdown`                    | ✅             |
| `exit`                        | ✅             |

### Document synchronization

| Method                           | Support status |
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

### Language features

| Method                                   | Support status |
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

### Workspace features

| Method                                  | Support status |
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

### Window features

| Method                            | Support status |
| --------------------------------- | -------------- |
| `window/showMessage`              | ➖             |
| `window/showMessageRequest`       | ➖             |
| `window/showDocument`             | ➖             |
| `window/logMessage`               | ➖             |
| `window/workDoneProgress/create`  | ➖             |
| `window/workDoneProgress/cancel`  | ➖             |
| `telemetry/event`                 | ➖             |
