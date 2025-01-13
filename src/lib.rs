// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later
//

mod config;
mod ls;
mod lsp;
mod proc;
mod protocol;
mod request;
mod response;
mod transport;

pub use config::Config;

use std::io::{BufRead, Write};

#[derive(Debug, PartialEq)]
pub enum ReturnCode {
    OkExit = 0,
    ErrExit = 1,
    UsageErr = 65,
}

pub fn run(args: Vec<String>) -> ReturnCode {
    let cfg = match config::Config::build(&args) {
        Ok(conf) => conf,
        Err(rc) => return rc,
    };
    let channel = transport::build_channel(cfg);

    ls::serve(channel)
}
