// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use core::{char, str};
use serde_json::{Result, Value};
use std::{collections, io::BufRead};

use crate::{parser::Token, ErrorCode};

pub fn scan(buf: &mut impl BufRead) -> Option<ErrorCode> {
    // let mut line = 1;

    // let tokens = header::scan(buf, &mut line);
    // println!("tokens: {:#?}", tokens);

    // let mut buffer = [0; 183];
    // let val = buf.read_exact(&mut buffer);

    // let repr = str::from_utf8(&buffer);
    // let v: Value = serde_json::from_str(repr.unwrap()).unwrap();

    // println!("{:#?}", repr);
    // println!("{:#?}", v);

    None
}
