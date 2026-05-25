<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

t32 Language Server
===================

Language server for the Lauterbach TRACE32® script language.
It is available for Linux, Windows, and macOS.

The extension packages [t32-language-server] to make LSP features available in
VS Code and VSCodium.

[t32-language-server]: https://codeberg.org/xasc/t32-language-server


Features
--------

### Code Folding

Long multi-line comments and PRACTICE blocks can be collapsed.

![Sample screenshot for code folds](https://raw.githubusercontent.com/xasc/t32-language-server/main/vscode/images/folds.png)

### Cross-references

"Go to Definition" and "Find All References" locates macro, subroutines,
commands, and scripts in your project.

![Sample screenshot for cross-references](https://raw.githubusercontent.com/xasc/t32-language-server/main/vscode/images/sample_xrefs.png)

### Semantic tokens

Semantic tokens augment the editor syntax highlighting.

![Sample screenshot for semantic highlighting](https://raw.githubusercontent.com/xasc/t32-language-server/main/vscode/images/semantic_tokens.png)


Extension settings
------------------

This extension contributes the following language:

-  `practice`: Lauterbach TRACE32® script language
    -  Display name: `PRACTICE`
    -  File extensions: `cmm`, `cmmt`

This extension contributes the following settings:

-  `t32ls.t32.systemDirectory`: TRACE32 system directory
-  `t32ls.t32.temporaryDirectory`: TRACE32 temporary directory
-  `t32ls.trace.server`: Trace server communication with VS Code.
