// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io;

use t32_language_server;

fn build_msg(header: &str, content: &str) -> String {
    format!("{}{}", header, content)
}

#[test]
fn lifecycle_initialize_req() {
    let content = r#"
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": 31
            }
        }

    "#;

    let header = format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r
Content-Length: {}\r\n\r\n",
        content.len()
    );

    let msg = build_msg(&header, content);

    let streams = t32_language_server::Stdio {
        reader: io::BufReader::new(msg.as_bytes()),
        writer: &mut io::stdout(),
        error: &mut io::stderr(),
    };
    t32_language_server::run(vec!["t32-language-server".to_string()], streams);
    assert!(false);
}
