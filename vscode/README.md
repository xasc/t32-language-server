<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

t32-language-server
===================

Language server for the Lauterbach TRACE32® script language.
It is available for Linux, Windows, and macOS.


Features
--------

-  Go to definition for PRACTICE macros and subroutines.
-  Locates PRACTICE macros and file references across all scripts in a project.
-  Semantic token detection for improved syntax highlighting.


Extension settings
------------------

This extension contributes the following language:

*  `practice`: Lauterbach TRACE32® script language
    *  Display name: `PRACTICE`
    *  File extensions: `cmm`, `cmmt`

This extension contributes the following settings:

*  `t32ls.trace.server`: Trace server communication with VS Code.
