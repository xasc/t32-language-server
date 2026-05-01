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

-  On serialization errors the complete path to the node that triggers the
   error is printed. Only active for debug builds.
-  Trigger server shutdown if any of the task queue workers aborts.

### Changed

-  Setting "--clientProcessId" to the value 0 does neither trigger the warning
   that the option is missing nor the check for inconsistent parent process
   IDs.
-  Switch to *tree-sitter-t32* v8.0.0.

### Fixed

-  Fix type definitions for initialization request.
-  Fix handling of LSP messages with large payload. The read loop ended up in a
   deadlock.
-  Fix aborts when parsing scripts containing a `RETURNVALUES` command.
-  Fix support of `ENTRY` commands with `%LINE%` directive in the parameter
   list.
-  Fix parsing of subroutine calls that use a macro target instead of an
   identifier.
-  Fix detection of macro definitions that end on comment.


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
-  Fix test execution on Windows machines. EOL conversion to CRLF was breaking
   tests that are checking byte offsets.
-  Accept alternative exit status if parent process ID does not exist.


[0.6.1] - 2026-04-18
--------------------

### Fixed

-  Readme and changelog were missing in published crate. They are now included.


[0.6.0] - 2026-04-18
--------------------

### Added

-  Initial release
