// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use tree_sitter::{Parser, Tree};
use tree_sitter_t32;

/// Use same language ID as [PRACTICE extension for Visual Studio
/// Code](https://marketplace.visualstudio.com/items?itemName=lauterbach.practice) for Visual
/// Studio Code.
pub const LANGUAGE_ID: &'static str = "practice";

pub const SUFFIXES: [&'static str; 2] = ["cmm", "cmmt"];

pub fn lang_id_supported(lang_id: &str) -> bool {
    lang_id == LANGUAGE_ID
}

pub fn parse(text: &[u8], incremental: Option<&Tree>) -> Tree {
    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_t32::LANGUAGE.into())
        .expect("Cannot load t32 grammar.");

    parser
        .parse(text, incremental)
        .expect("TRACE32 script parser must not fail.")
}
