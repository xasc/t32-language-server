// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::BufRead;

mod parser;
mod protocol;

#[derive(Debug)]
pub enum ErrorCode {
    DataErr = 64,
    UsageErr = 65,
    IoErr = 74,
    ProtocolErr = 76,
}

pub fn run(args: Vec<String>, buf: &mut impl BufRead) -> Option<ErrorCode> {
    serve(buf);
    Option::None
}

fn serve(buf: &mut impl BufRead) -> Option<ErrorCode> {
    let _ = eval(buf);
    None
}

fn eval(buf: &mut impl BufRead) -> Result<(), ErrorCode> {
    parser::parse(buf);
    Ok(())
}

pub fn error(message: &str) {
    println!("Error: {message}");
}
