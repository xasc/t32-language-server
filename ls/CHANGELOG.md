<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

Changelog
=========

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


[Unreleased]
------------

### Added

-  Progress reporting can be initiated by the server.
-  Progress reporting for workspace indexing.

### Changed

-  The server can process client requests while workspace indexing is still
   ongoing. The server is only blocking to queue workspace indexing. Indexing
   results are resolved during normal server operation.


[0.13.0] - 2026-05-25
---------------------

### Added

-  Add support for code folding.


[0.12.2] - 2026-05-22
---------------------

### Fixed

-  Make sure to strip quotes around command line arguments.


[0.12.1] - 2026-05-22
---------------------

### Fixed

-  Fix handling of macro definition and parameter declaration keywords with
   lowercase or mixed capitalization.


[0.12.0] - 2026-05-22
---------------------

### Added

-  Add support for default path prefixes in script paths:
    -  `~` is the user home directory.
    -  `~~` specifies the TRACE32 system directory.
    -  `~~~` sets the TRACE32 temporary directory.
    -  `~~~~` is an alias for the active script directory.

   The path prefixes are used to resolve the targets of ambiguous file paths.
-  The command line flag `--t32SystemDir=DIR` specifies the location of the
   TRACE32 system directory.
-  The command line flag `--t32TempDir=DIR` sets the selected temporary
   directory of TRACE32.


[0.11.0] - 2026-05-18
---------------------

### Fixed

-  Skip process status detection if not parent PID is available.


[0.10.0] - 2026-05-17
---------------------

### Added

-  Publish extension for VS Code.
-  Update readme
-  Set new semantic token scopes:
    -  `entity.name.function.practice` for subroutine calls
    -  `storage.modifier.macro.practice` for `PRIVATE`, `LOCAL`, and `GLOBAL`
       commands.
    -  `constant.language.format.practice` for command format parameters.
    -  `constant.language.option.practice` for command options parameters.
    -  `keyword.control.practice` for if-then, loops, and return keywords.

### Changed

-  Build release binaries using older OS images for better compatibility.

### Fixed

-  Fix semantic tokens for `GOSUB`, `ENTRY`, `PARAMETERS`, and `RETURNVALUES`
   commands.
   They were displayed as variables.


[0.9.0] - 2026-05-14
--------------------

### Added

-  Make precompiled binaries for Linux AArch64, macOS x86_64, and Windows
   AArch64 available.


[0.8.0] - 2026-05-14
--------------------

### Added

-  Switch to *tree-sitter-t32* v9.0.0.
-  On serialization errors the complete path to the node that triggers the
   error is printed.
   Only active for debug builds.
-  Trigger server shutdown if any of the task queue workers aborts.
-  Set new semantic token scopes:
    -  `function.defaultLibrary` for built-in PRACTICE functions
    -  `variable.other.macro.definition.practice` for macro definitions
    -  `variable.parameter.practice` for parameter declarations
    -  `keyword.control.practice` for control flow keywords and command
       expressions
    -  `keyword.operator.practice` for operators
    -  `string.other.path.practice` for unquoted paths
    -  `comment.practice` for comments
    -  `constant.numeric.practice` for numbers

### Changed

-  Setting "--clientProcessId" to the value 0 does neither trigger the warning
   that the option is missing nor the check for inconsistent parent process
   IDs.
-  Switch to *tree-sitter-t32* v8.0.0.

### Fixed

-  Fix type definitions for initialization request.
-  Fix handling of LSP messages with large payload.
   The read loop ended up in a deadlock.
-  Fix aborts when parsing scripts containing a `RETURNVALUES` command.
-  Fix support of `ENTRY` commands with `%LINE%` directive in the parameter
   list.
-  Fix parsing of subroutine calls that use a macro target instead of an
   identifier.
-  Fix detection of macro definitions that end on comment.
-  Fix update of text document contents.
-  Fix incremental update of abstract syntax trees.
-  Fix semantic token detection.
-  Fix semantic token conversion if client has no support for multi-line tokens.
-  Fix macro reference retrieval in subroutines.
-  Fix host architecture for macOS release artifacts.
   They are built for `AArch64`.


[0.7.1] - 2026-04-21
--------------------

### Added

-  Make precompiled binaries for WebAssembly with WASI SDK available.


[0.7.0] - 2026-04-19
--------------------

### Added

-  Make precompiled binaries for Linux, macOS, and Windows available.

### Fixed

-  Fix parent process status detection for Windows builds.
-  Fix test execution on Windows machines.
   EOL conversion to CRLF was breaking tests that are checking byte offsets.
-  Accept alternative exit status if parent process ID does not exist.


[0.6.1] - 2026-04-18
--------------------

### Fixed

-  Readme and changelog were missing in published crate.
   They are now included.


[0.6.0] - 2026-04-18
--------------------

### Added

-  Initial release
