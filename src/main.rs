// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{env, io, process};

use t32_language_server::Stdio;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 2 {
        println!("Usage: {} [script]", args[0]);
        process::exit(64);
    }

    let streams = Stdio {
        reader: io::stdin().lock(),
        writer: &mut io::stdout(),
        error: &mut io::stderr(),
    };

    let rc = t32_language_server::run(args, streams);
    process::exit(rc as i32)
}
