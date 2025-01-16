// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::process::{Child, Command, Stdio};
use t32_language_server;

pub fn start_ls(args: &[&str]) -> Child {
    let mut params = vec!["run", "--quiet", "--"];
    params.extend_from_slice(&args);

    Command::new("cargo")
        .args(params)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Must be able to start language server.")
}
