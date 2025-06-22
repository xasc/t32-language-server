// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod ast;
mod expressions;
mod path;

use tree_sitter::{Language, Parser, Tree, TreeCursor};
use tree_sitter_t32;

use crate::{ls::FileIndex, protocol::Uri};

pub use ast::{NodeKind, id_into_node};
pub use expressions::{
    CallExpression, CallExpressions, CallLocations, MacroDefResolution, MacroDefinitions,
    ParameterDeclaration, Subroutine, SubscriptCalls,
};

pub use expressions::{
    find_all_call_expressions as find_call_expressions,
    find_all_global_macro_definitions as find_global_macro_definitions,
    find_all_parameter_declarations as find_parameter_declarations,
    find_all_subroutines as find_subroutines,
};

use expressions::{
    find_file_target, find_macro_definition, find_subroutine_definition, locate_subscript,
};

#[derive(Clone, Debug)]
pub struct LangExpressions {
    pub macros: MacroDefinitions,
    pub subroutines: Option<Vec<Subroutine>>,
    pub calls: CallExpressions,
    pub parameters: Option<Vec<ParameterDeclaration>>,
}

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
    t32: &LangExpressions,
    r#macro: TreeCursor,
) -> Option<Vec<MacroDefResolution>> {
    debug_assert_eq!(
        r#macro.node().kind_id(),
        NodeKind::Macro.into_id(&r#macro.node().language()),
    );

    if r#macro.node().end_byte() >= text.len() {
        return None;
    }
    find_macro_definition(text, tree, t32, r#macro)
}

pub fn goto_subroutine_definition(
    text: &str,
    subroutines: &Vec<Subroutine>,
    call: TreeCursor,
) -> Option<Subroutine> {
    debug_assert_eq!(
        call.node().kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&call.node().language()),
    );

    if call.node().end_byte() >= text.len() {
        return None;
    }
    find_subroutine_definition(text, subroutines, call)
}

pub fn goto_file(text: &str, calls: &SubscriptCalls, command: TreeCursor) -> Option<Uri> {
    debug_assert_eq!(
        command.node().kind_id(),
        NodeKind::CommandExpression.into_id(&command.node().language()),
    );

    if command.node().end_byte() >= text.len() {
        return None;
    }
    find_file_target(calls, command)
}

pub fn resolve_subscript_call_targets(
    text: &str,
    tree: &Tree,
    target: usize,
    files: &FileIndex,
) -> Option<Vec<Uri>> {
    if let Some(calls) = locate_subscript(text, tree, target, files) {
        let mut scripts: Vec<Uri> = Vec::with_capacity(1);

        calls.into_iter().for_each(|c| scripts.push(c));
        Some(scripts)
    } else {
        None
    }
}
