// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{env, process};

use t32_language_server;

fn main() {
    let args: Vec<String> = env::args().collect();

    let rc = t32_language_server::run(args);
    process::exit(rc as i32)
}
