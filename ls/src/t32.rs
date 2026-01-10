// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

mod ast;
mod cache;
mod expressions;
mod macros;
mod path;

use tree_sitter::{Language, Parser, Tree, TreeCursor};
use tree_sitter_t32;

pub use ast::{NodeKind, id_into_node};
pub use cache::{get_macro_scope, locate_calls_to_file_target};
pub use expressions::{
    CallExpression, CallExpressions, CallLocations, Label, MacroDefinition, MacroDefinitions,
    MacroScope, ParameterDeclaration, Subroutine, SubscriptCalls,
};

pub use expressions::{
    find_all_call_expressions as find_call_expressions,
    find_all_parameter_declarations as find_parameter_declarations,
    find_all_subroutines_and_labels as find_subroutines_and_labels,
    find_external_macro_definition as goto_external_macro_definition,
};

pub use macros::{
    defines_named_macro, find_all_macro_definitions as find_macro_definitions,
    find_any_macro_references,
};

#[cfg(test)]
pub use expressions::MacroDefinitionsImplicit;

use std::ops::Range;

use crate::{ls::FileIndex, protocol::Uri, utils::BRange};

use expressions::{
    find_all_references_for_label, find_all_references_for_subroutine, find_call_target_definition,
    find_file_target, find_label, find_macro_definition, find_subroutine, locate_subscript,
};

use macros::find_macro_references_at_offset;

pub enum MacroDefinitionResult {
    Final(Vec<MacroDefinition>),
    Partial(Vec<MacroDefinition>),
    Indeterminate,
}

#[derive(Clone, Debug)]
pub struct LangExpressions {
    pub macros: MacroDefinitions,
    pub macro_refs: Vec<BRange>,
    pub subroutines: Vec<Subroutine>,
    pub calls: CallExpressions,
    pub parameters: Vec<ParameterDeclaration>,
    pub labels: Vec<Label>,
}

#[derive(Clone, Debug)]
pub struct FindMacroRefsLangContext {
    pub macros: MacroDefinitions,
    pub subroutines: Vec<Subroutine>,
    pub calls: CallExpressions,
    pub parameters: Vec<ParameterDeclaration>,
    pub labels: Vec<Label>,
}

#[derive(Clone, Debug)]
pub struct FindRefsLangContext {
    pub macros: MacroDefinitions,
    pub subroutines: Vec<Subroutine>,
    pub calls: CallExpressions,
    pub parameters: Vec<ParameterDeclaration>,
    pub labels: Vec<Label>,
}

#[derive(Clone, Debug)]
pub struct GotoDefLangContext {
    pub macros: MacroDefinitions,
    pub subroutines: Vec<Subroutine>,
    pub calls: CallExpressions,
    pub parameters: Vec<ParameterDeclaration>,
}

/// Use same language ID as [PRACTICE extension for Visual Studio
/// Code](https://marketplace.visualstudio.com/items?itemName=lauterbach.practice) for Visual
/// Studio Code.
pub const LANGUAGE_ID: &'static str = "practice";

pub const SUFFIXES: [&'static str; 2] = ["cmm", "cmmt"];

impl From<LangExpressions> for FindMacroRefsLangContext {
    fn from(t32: LangExpressions) -> Self {
        FindMacroRefsLangContext {
            macros: t32.macros,
            subroutines: t32.subroutines,
            calls: t32.calls,
            parameters: t32.parameters,
            labels: t32.labels,
        }
    }
}

impl From<LangExpressions> for FindRefsLangContext {
    fn from(t32: LangExpressions) -> Self {
        FindRefsLangContext {
            macros: t32.macros,
            subroutines: t32.subroutines,
            calls: t32.calls,
            parameters: t32.parameters,
            labels: t32.labels,
        }
    }
}

impl From<FindRefsLangContext> for GotoDefLangContext {
    fn from(t32: FindRefsLangContext) -> Self {
        GotoDefLangContext {
            macros: t32.macros,
            subroutines: t32.subroutines,
            calls: t32.calls,
            parameters: t32.parameters,
        }
    }
}

impl From<LangExpressions> for GotoDefLangContext {
    fn from(t32: LangExpressions) -> Self {
        GotoDefLangContext {
            macros: t32.macros,
            subroutines: t32.subroutines,
            calls: t32.calls,
            parameters: t32.parameters,
        }
    }
}

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

pub fn get_goto_def_ids(lang: &Language) -> [u16; 3] {
    let mut ids = [0u16; 3];
    for (ii, &node) in ast::GOTO_DEF_SOURCES.iter().enumerate() {
        ids[ii] = node.into_id(&lang);
    }
    ids
}

pub fn get_find_ref_ids(lang: &Language) -> [u16; 5] {
    let mut ids = [0u16; 5];
    for (ii, &node) in ast::FIND_REF_SOURCES.iter().enumerate() {
        ids[ii] = node.into_id(&lang);
    }
    ids
}

pub fn goto_infile_macro_definition(
    text: &str,
    tree: &Tree,
    t32: &GotoDefLangContext,
    r#macro: TreeCursor,
) -> Option<MacroDefinitionResult> {
    debug_assert_eq!(
        r#macro.node().kind_id(),
        NodeKind::Macro.into_id(&r#macro.node().language()),
    );

    if r#macro.node().end_byte() >= text.len() {
        return None;
    }
    let node = r#macro.node();

    debug_assert!(node.end_byte() < text.len());
    debug_assert_eq!(node.kind_id(), NodeKind::Macro.into_id(&node.language()),);

    let name = &text[node.start_byte()..node.end_byte()];
    if !defines_named_macro(text, &t32.macros, &t32.parameters, name) {
        return Some(MacroDefinitionResult::Indeterminate);
    }
    find_macro_definition(text, tree, t32, node)
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
    find_call_target_definition(text, subroutines, call)
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

pub fn find_subroutine_call_references(
    text: &str,
    subroutines: &Vec<Subroutine>,
    call: TreeCursor,
    tree: &Tree,
) -> Option<Vec<Range<usize>>> {
    debug_assert_eq!(
        call.node().kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&call.node().language()),
    );

    if call.node().end_byte() >= text.len() {
        return None;
    }

    let Some(def) = find_call_target_definition(text, subroutines, call) else {
        return None;
    };
    Some(find_all_references_for_subroutine(text, &def, tree))
}

pub fn find_subroutine_references(
    text: &str,
    subroutines: &Vec<Subroutine>,
    subroutine: &TreeCursor,
    tree: &Tree,
) -> Option<Vec<Range<usize>>> {
    debug_assert!(
        subroutine.node().kind_id()
            == NodeKind::SubroutineBlock.into_id(&subroutine.node().language())
            || subroutine.node().kind_id()
                == NodeKind::LabeledExpression.into_id(&subroutine.node().language())
    );

    if subroutine.node().end_byte() >= text.len() {
        return None;
    }

    let Some(def) = find_subroutine(subroutines, &subroutine) else {
        return None;
    };
    Some(find_all_references_for_subroutine(text, &def, tree))
}

pub fn find_label_references(
    text: &str,
    labels: &Vec<Label>,
    label: &TreeCursor,
    tree: &Tree,
) -> Option<Vec<Range<usize>>> {
    debug_assert_eq!(
        label.node().kind_id(),
        NodeKind::LabeledExpression.into_id(&label.node().language())
    );

    if label.node().end_byte() >= text.len() {
        return None;
    }

    let Some(label) = find_label(labels, label) else {
        return None;
    };
    Some(find_all_references_for_label(text, label, tree))
}

pub fn find_macro_definition_references(
    text: &str,
    tree: &Tree,
    t32: &FindMacroRefsLangContext,
    name: &str,
    scope: MacroScope,
    range: BRange,
) -> (Vec<BRange>, Vec<Uri>) {
    let span = range.to_inner();
    if span.start >= text.len() {
        return (Vec::new(), Vec::new());
    }

    let (mut refs, mut callees) =
        find_macro_references_at_offset(text, tree, t32, name, scope, span.start);

    if !refs.iter().any(|r| *r == span) {
        refs.push(BRange::from(span));
    }

    if !callees.is_empty() && scope == MacroScope::Private {
        callees.clear();
    }

    refs.sort_by_key(|a| {
        let span = a.inner();
        (span.start, span.end)
    });
    (refs, callees)
}

pub fn find_stack_macro_references(
    text: &str,
    tree: &Tree,
    t32: &FindMacroRefsLangContext,
    scope: MacroScope,
    name: &str,
) -> (Vec<BRange>, Vec<Uri>) {
    let (mut refs, callees) = find_macro_references_at_offset(text, tree, t32, name, scope, 0);

    refs.sort_by_key(|a| {
        let span = a.inner();
        (span.start, span.end)
    });
    (refs, callees)
}
