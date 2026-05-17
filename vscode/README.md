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

### Cross-references

"Go to Definition" and "Find All References" locates macro, subroutines, and
scripts in your project.

![Sample screenshot for cross-references](https://raw.githubusercontent.com/xasc/t32-language-server/main/vscode/images/sample_xrefs.png)

### Semantic tokens

Semantic tokens augment the editor syntax highlighting.


Extension settings
------------------

This extension contributes the following language:

*  `practice`: Lauterbach TRACE32® script language
    *  Display name: `PRACTICE`
    *  File extensions: `cmm`, `cmmt`

This extension contributes the following settings:

*  `t32ls.trace.server`: Trace server communication with VS Code.
