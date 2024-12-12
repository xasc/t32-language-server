// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later
//
mod config;
mod parser;
mod protocol;

use std::io::{BufRead, Write};
pub use config::Config as Config;

#[derive(Debug, PartialEq)]
pub enum ReturnCode {
    OkExit = 0,
    DataErr = 64,
    UsageErr = 65,
    IoErr = 74,
    ProtocolErr = 76,
}

pub struct Stdio<R: BufRead, W: Write, E: Write> {
    pub reader: R,
    pub writer: W,
    pub error: E,
}

pub fn run<R, W, E>(args: Vec<String>, streams: &mut Stdio<R, W, E>) -> ReturnCode
where
    R: BufRead,
    W: Write,
    E: Write,
{
    let cfg = config::Config::build(&args, &mut streams.writer);
    if let Err(rc) = cfg {
        return rc;
    }
    serve(&mut streams.reader);
    ReturnCode::OkExit
}

fn serve(buf: &mut impl BufRead) -> Option<ReturnCode> {
    let _ = eval(buf);
    None
}

fn eval(buf: &mut impl BufRead) -> Result<(), ReturnCode> {
    parser::parse(buf);
    Ok(())
}

pub fn error(message: &str) {
    println!("Error: {message}");
}
