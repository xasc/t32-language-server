// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{error::Category, Error};

use crate::protocol::{ErrorCodes, RequestMessage, ResponseError};

pub fn parse_request(buf: &[u8]) -> Result<RequestMessage, ResponseError> {
    println!("{:?}", String::from_utf8(buf.to_vec()));
    match serde_json::from_slice(buf) {
        Ok(val) => Ok(val),
        Err(err) => {
            match err.classify() {
                Category::Io => unreachable!(),  // Byte buffer must be valid.
                Category::Syntax => return Err(error_syntax(err, buf)),
                Category::Data => return Err(error_data(err, buf)),
                Category::Eof => return Err(error_incomplete(buf.len())),
            }
        }
    }

}

fn error_syntax(err: Error, buf: &[u8]) -> ResponseError {
    let offset = get_error_offset(err, buf);

    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Unexpected data in message content at offset \"{}\".",
            offset
        )),
        data: None,
    }
}

fn error_data(err: Error, buf: &[u8]) -> ResponseError {
    let offset = get_error_offset(err, buf);

    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Data error: Semantically incorrect data in message content at offset \"{}\".",
            offset
        )),
        data: None,
    }
}

fn error_incomplete(len: usize) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Data error: Message content is incomplete. Expected a total length of \"{}\" bytes.",
            len
        )),
        data: None,
    }
}

fn get_error_offset(err: Error, buf: &[u8]) -> usize {
    let mut line: usize = 1;
    let mut col: usize  = 1;

    let mut offset: usize = 0;
    if err.line() <= 0 {
        return offset;
    }

    for ch in buf {
        if line == err.line() {
            if err.column() <= 0 || col == err.column() {
                break;
            }
            col += 1;
        }
        else if (*ch as char) == '\n' {
            line += 1;
        }
        offset += 1;
    }
    offset
}
