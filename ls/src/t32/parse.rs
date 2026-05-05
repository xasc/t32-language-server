// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use tree_sitter::{Parser, Tree};
use tree_sitter_t32;
pub fn parse_full(text: &[u8]) -> Tree {
    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_t32::LANGUAGE.into())
        .expect("Cannot load t32 grammar.");

    parser
        .parse(text, None)
        .expect("TRACE32 script parser must not fail.")
}

pub fn parse_incremental(text: &[u8], incremental: Option<&Tree>) -> Tree {
    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_t32::LANGUAGE.into())
        .expect("Cannot load t32 grammar.");

    parser
        .parse(text, incremental)
        .expect("TRACE32 script parser must not fail.")
}
