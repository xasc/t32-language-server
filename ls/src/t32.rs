// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use tree_sitter::{Language, Parser, Tree, TreeCursor};
use tree_sitter_t32;

mod ast;
mod expressions;

pub use ast::{NodeKind, id_into_node};
pub use expressions::{MacroDefinition, MacroDefinitions, Subroutine};

use expressions::{find_all_global_macro_definitions, find_all_subroutines, find_macro_definition};

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

pub fn get_goto_ref_ids(lang: &Language) -> [u16; 3] {
    let mut ids = [0u16; 3];
    for (ii, &node) in ast::GOTO_REF_SOURCES.iter().enumerate() {
        ids[ii] = ast::node_into_id(&lang, node);
    }
    ids
}

pub fn goto_macro_definition(
    text: &str,
    tree: &Tree,
    r#macro: TreeCursor,
) -> Option<MacroDefinition> {
    let lang = tree.language();
    debug_assert!(matches!(
        lang.node_kind_for_id(r#macro.node().kind_id()),
        Some(ast::NODE_MACRO)
    ));

    if r#macro.node().end_byte() >= text.len() {
        return None;
    }
    find_macro_definition(text, tree, r#macro)
}

pub fn find_global_macro_definitions(text: &str, tree: &Tree) -> MacroDefinitions {
    find_all_global_macro_definitions(text, tree)
}

pub fn find_subroutines(text: &str, tree: &Tree) -> Option<Vec<Subroutine>> {
    find_all_subroutines(text, tree)
}
