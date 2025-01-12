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

pub struct Stdio<'a, R: BufRead, W: Write, E: Write> {
    pub reader: R,
    pub writer: &'a mut W,
    pub error: &'a mut E,
}

pub fn run<R, W, E>(args: Vec<String>, stdio: Stdio<R, W, E>) -> ReturnCode
where
    R: BufRead,
    W: Write,
    E: Write,
{
    let cfg = match config::Config::build(&args, stdio.writer, stdio.error) {
        Ok(conf) => conf,
        Err(rc) => return rc,
    };
    let channel = transport::build_channel(cfg, stdio);

    ls::serve(channel)
}
