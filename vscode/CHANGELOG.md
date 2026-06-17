<!--
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>

SPDX-License-Identifier: EUPL-1.2
-->

Changelog
=========

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


[0.6.0] - 2026-06-17
--------------------

### Added

-  Switch to *t32-language-server* v0.14.0.


[0.5.0] - 2026-05-25
--------------------

### Added

-  Switch to *t32-language-server* v0.13.0.
-  Update feature section in readme.


[0.4.0] - 2026-05-22
--------------------

### Added

-  Add configuration new settings for directory configuration:
    -  `t32ls.t32.systemDirectory`: TRACE32 system directory
    -  `t32ls.t32.temporaryDirectory`: TRACE32 temporary directory

-  Update feature section in readme.
-  Switch to *t32-language-server* v0.12.2.

### Changed

-  Server is started with process ID of Node.js client process in
   `--clientProcessId` flag.


[0.3.3] - 2026-05-18
--------------------

### Added

-  Switch to *t32-language-server* v0.11.0.

### Fixed

-  Fix parent process status detection for Windows builds.


[0.3.2] - 2026-05-17
--------------------

### Fixed

-  Fix path to server executable on Windows.


[0.3.1] - 2026-05-17
--------------------

### Fixed

-  Fix image link in readme.


[0.3.0] - 2026-05-17
--------------------

### Added

-  Switch to *t32-language-server* v0.10.0.
-  Improve syntax highlighting for commands and their parameters.


[0.2.1] - 2026-05-15
--------------------

### Added

-  Add `SUPPORT.md` and `LICENSE` file to extension.
-  Add link to Codeberg issue tracker.

### Changed

-  The extension repository is now pointing to GitHub where the extension is
   packaged.


[0.2.0] - 2026-05-14
--------------------

### Changed

-  The extension settings and channel names are using the same display name as
   the extension.


[0.1.0] - 2026-05-14
--------------------

### Added

-  Initial release with *t32-language-server* v0.9.0.
