// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later
//
mod config;
mod parser;
mod protocol;
mod transport;

pub use config::Config;
use std::io::{BufRead, Write};

#[derive(Debug, PartialEq)]
pub enum ReturnCode {
    OkExit = 0,
    DataErr = 64,
    UsageErr = 65,
    IoErr = 74,
    ProtocolErr = 76,
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
    let cfg = match config::Config::build(&args, stdio.reader, stdio.writer, stdio.error) {
        Ok(conf) => conf,
        Err(rc) => return rc,
    };
    let channel = transport::build_channel(cfg, stdio.writer);

    serve(channel)
}

fn serve<R: BufRead, W: Write>(mut channel: transport::Channel<R, W>) -> ReturnCode {
    loop {
        let _ = channel.read_msg();
        break;
    }

    // let _ = eval(buf);
    ReturnCode::OkExit
}

pub fn error(message: &str) {
    println!("Error: {message}");
}
