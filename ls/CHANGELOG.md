<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

Changelog
=========

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


[0.7.1] - 2026-04-20
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
