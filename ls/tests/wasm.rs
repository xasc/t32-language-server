#! /bin/sh
//usr/bin/env rustc $0 -o ${0}x && ./${0}x; rm -f ${0}x ; exit

// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::process;

fn main() {
    let msgs = [
        "{\"jsonrpc\": \"2.0\", \"method\": \"initialize\", \"id\": 1, \"params\": {\"capabilities\": {}}}",
        "{\"jsonrpc\": \"2.0\", \"method\": \"initialized\", \"params\": {}}",
        "{\"jsonrpc\": \"2.0\", \"method\": \"textDocument/definition\", \"id\": 2, \"params\": {\"textDocument\": {\"uri\": \"file://C:/test.cmm\"}, \"position\": {\"line\": 17, \"character\": 9}}}",
        "{\"jsonrpc\": \"2.0\", \"method\": \"shutdown\", \"id\": 2, \"params\": {}}",
        "{\"jsonrpc\": \"2.0\", \"method\": \"exit\", \"params\": {}}",
    ];

    for payload in msgs {
        let header = format!("Content-Length: {}\r\n\r\n", payload.len());

        print!("{}{}", header, payload);
    }
    process::exit(0)
}
