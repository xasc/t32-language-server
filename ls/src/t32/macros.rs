// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use tree_sitter::{Tree, TreeCursor};

use crate::{protocol::Uri, utils::BRange};

use super::{
    FindMacroRefsLangContext,
    ast::{
        KEYWORD_SUBROUTINE_ENTRY, KEYWORD_SUBROUTINE_PARAMETERS, NodeKind,
        get_control_flow_block_ids, get_macro_container_expr_ids,
    },
    cache::find_subroutine_for_call,
    expressions::{
        CallExpression, MacroDefResolution, MacroDefinition, MacroDefinitions, MacroScope,
        ParameterDeclaration, SubscriptCalls,
    },
};

pub struct MacroReferencesBlockCaptures<'a> {
    pub references: Vec<BRange>,
    pub subroutines: Vec<&'a CallExpression>,
    pub scripts: Vec<&'a Uri>,
}

impl<'a> MacroReferencesBlockCaptures<'a> {
    pub fn new() -> Self {
        MacroReferencesBlockCaptures {
            references: Vec::new(),
            subroutines: Vec::new(),
            scripts: Vec::new(),
        }
    }
}

pub fn find_any_macro_references(tree: &Tree) -> Vec<BRange> {
    let mut cursor = tree.walk();

    let mut refs: Vec<BRange> = Vec::new();

    if !cursor.goto_first_child() {
        return refs;
    }

    let r#macro = NodeKind::Macro.into_id(&tree.language());

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == r#macro {
            refs.push(node.byte_range().into());
        } else if cursor.goto_first_child() {
            continue;
        }

        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'outer;
            }
        }
    }
    refs
}

pub fn find_macro_references_at_offset(
    text: &str,
    tree: &Tree,
    t32: &FindMacroRefsLangContext,
    name: &str,
    scope: MacroScope,
    offset: usize,
) -> (Vec<BRange>, Vec<Uri>) {
    let Some(mut captures) = find_block_local_macro_references(text, tree, t32, name, offset)
    else {
        return (Vec::new(), Vec::new());
    };

    let mut refs: Vec<BRange> = Vec::new();
    let mut scripts: Vec<Uri> = Vec::new();

    refs.append(&mut captures.references);
    for script in captures.scripts {
        if !scripts.contains(script) {
            scripts.push(script.clone());
        }
    }

    if scope == MacroScope::Private {
        return (refs, scripts);
    }

    let mut visited: Vec<usize> = Vec::with_capacity(t32.subroutines.len());
    let mut next: Vec<&CallExpression> = Vec::with_capacity(t32.subroutines.len());

    let mut calls = captures.subroutines;
    loop {
        for &call in calls.iter() {
            let Some(sub) = find_subroutine_for_call(text, call, &t32.subroutines) else {
                continue;
            };

            if visited.contains(&sub.definition.start) {
                continue;
            }

            if let Some(mut captures) =
                find_block_local_macro_references(text, tree, t32, name, sub.definition.start)
            {
                next.append(&mut captures.subroutines);

                captures.references.retain(|r| !refs.contains(r));
                if !captures.references.is_empty() {
                    refs.append(&mut captures.references);
                }

                for script in captures.scripts {
                    if !scripts.contains(script) {
                        scripts.push(script.clone());
                    }
                }
            }
            visited.push(sub.definition.start);
        }

        if next.is_empty() {
            break;
        }
        calls.clear();
        calls.append(&mut next);
    }
    (refs, scripts)
}

pub fn find_block_local_macro_references<'a>(
    text: &str,
    tree: &Tree,
    t32: &'a FindMacroRefsLangContext,
    name: &str,
    offset: usize,
) -> Option<MacroReferencesBlockCaptures<'a>> {
    if offset >= text.len() {
        return None;
    }

    let mut cursor = tree.walk();
    if cursor.goto_first_child_for_byte(offset).is_none() {
        return None;
    }
    let lang = tree.language();

    let macro_def = NodeKind::MacroDefinition.into_id(&lang);
    let parameters = NodeKind::ParameterDeclaration.into_id(&lang);

    let labeled_expr = NodeKind::LabeledExpression.into_id(&lang);
    let script = NodeKind::Script.into_id(&lang);
    let subroutine = NodeKind::SubroutineBlock.into_id(&lang);

    // Move past entry points
    let kind = cursor.node().kind_id();
    if kind == macro_def || kind == parameters {
        if !cursor.goto_next_sibling() {
            return None;
        }
    } else if kind == labeled_expr || kind == subroutine || kind == script {
        if !cursor.goto_first_child() {
            return None;
        }
    }
    Some(find_macro_references_and_call_transitions(
        text, &t32, name, cursor,
    ))
}

pub fn defines_named_macro(
    text: &str,
    macros: &MacroDefinitions,
    parameters: &[ParameterDeclaration],
    macro_refs: &[BRange],
    name: &str,
) -> bool {
    if let Some(defs) = &macros.privates
        && defs.iter().any(|m| text[m.r#macro.clone()] == *name)
    {
        return true;
    }

    if let Some(macros) = &macros.locals
        && macros.iter().any(|m| text[m.r#macro.clone()] == *name)
    {
        return true;
    }

    if !parameters.is_empty() && parameters.iter().any(|m| text[m.r#macro.clone()] == *name) {
        return true;
    }

    if !macro_refs.is_empty() && macro_refs.iter().any(|r| text[r.inner().clone()] == *name) {
        return true;
    }

    if let Some(defs) = &macros.globals
        && defs.iter().any(|m| text[m.r#macro.clone()] == *name)
    {
        true
    } else {
        false
    }
}

pub fn defines_any_macro(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefinition> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::MacroDefinition.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            NodeKind::Macro.into_id(&r#macro.language())
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }

        if &text[range] == name {
            cursor.goto_parent();
            return Some(MacroDefinition {
                cmd: cursor.node().byte_range(),
                r#macro: r#macro.byte_range(),
                docstring: None,
            });
        }
    }
    cursor.goto_parent();
    None
}

pub fn defines_block_global_macro(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefinition> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::MacroDefinition.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );
    if MacroScope::from(&text[cursor.node().byte_range()]) == MacroScope::Private {
        cursor.goto_parent();
        return None;
    }

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            NodeKind::Macro.into_id(&r#macro.language())
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }

        if &text[range] == name {
            cursor.goto_parent();
            return Some(MacroDefinition {
                cmd: cursor.node().byte_range(),
                r#macro: r#macro.byte_range(),
                docstring: None,
            });
        }
    }
    cursor.goto_parent();
    None
}

pub fn may_define_macro_implicitly(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefResolution> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::ParameterDeclaration.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    let command = &text[cursor.node().byte_range()];

    if ![KEYWORD_SUBROUTINE_PARAMETERS, KEYWORD_SUBROUTINE_ENTRY]
        .iter()
        .any(|&c| c.eq_ignore_ascii_case(&command))
    {
        cursor.goto_parent();
        return None;
    }

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            NodeKind::Macro.into_id(&r#macro.language())
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }

        if &text[range] == name {
            cursor.goto_parent();

            let def = MacroDefinition {
                cmd: cursor.node().byte_range(),
                r#macro: r#macro.byte_range(),
                docstring: None,
            };
            return Some(match command {
                KEYWORD_SUBROUTINE_ENTRY => MacroDefResolution::Overridable(def),
                KEYWORD_SUBROUTINE_PARAMETERS => MacroDefResolution::Final(def),
                _ => unreachable!("Must not catch other commands. Aborts early."),
            });
        }
    }
    cursor.goto_parent();
    None
}

pub fn defines_global_macro_implicitly(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefResolution> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::ParameterDeclaration.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    let command = &text[cursor.node().byte_range()];

    if !KEYWORD_SUBROUTINE_ENTRY.eq_ignore_ascii_case(&command) {
        cursor.goto_parent();
        return None;
    }

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            NodeKind::Macro.into_id(&r#macro.language())
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }

        if &text[range] == name {
            cursor.goto_parent();

            let def = MacroDefinition {
                cmd: cursor.node().byte_range(),
                r#macro: r#macro.byte_range(),
                docstring: None,
            };
            return Some(match command {
                KEYWORD_SUBROUTINE_ENTRY => MacroDefResolution::Overridable(def),
                _ => unreachable!("Must not catch other commands. Aborts early."),
            });
        }
    }
    cursor.goto_parent();
    None
}

pub fn find_macro_scope(text: &str, cursor: &mut TreeCursor) -> Option<MacroScope> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::MacroDefinition.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );
    let scope = Some(MacroScope::from(&text[cursor.node().byte_range()]));

    cursor.goto_parent();
    scope
}

pub fn extract_macro_defs(text: &str, cursor: &mut TreeCursor, macros: &mut Vec<MacroDefinition>) {
    let def = cursor.node();
    debug_assert_eq!(
        def.kind_id(),
        NodeKind::MacroDefinition.into_id(&def.language())
    );

    if !cursor.goto_first_child() {
        return;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&def.language())
    );
    debug_assert!(
        [MacroScope::Private, MacroScope::Local, MacroScope::Global]
            .contains(&MacroScope::from(&text[cursor.node().byte_range()]))
    );

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();

        debug_assert_eq!(
            r#macro.kind_id(),
            NodeKind::Macro.into_id(&r#macro.language())
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }
        macros.push(MacroDefinition {
            cmd: def.byte_range(),
            r#macro: r#macro.byte_range(),
            docstring: None,
        });
    }
    cursor.goto_parent();
}

pub fn extract_params(
    text: &str,
    cursor: &mut TreeCursor,
    declarations: &mut Vec<ParameterDeclaration>,
) {
    let decl = cursor.node();
    debug_assert_eq!(
        decl.kind_id(),
        NodeKind::ParameterDeclaration.into_id(&decl.language())
    );

    if !cursor.goto_first_child() {
        return;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&decl.language())
    );

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();

        debug_assert_eq!(
            r#macro.kind_id(),
            NodeKind::Macro.into_id(&r#macro.language())
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }
        declarations.push(ParameterDeclaration {
            cmd: decl.byte_range(),
            r#macro: r#macro.byte_range(),
            docstring: None,
        });
    }
    cursor.goto_parent();
}

pub fn find_macro_references_and_call_transitions<'a>(
    text: &str,
    t32: &'a FindMacroRefsLangContext,
    name: &str,
    mut cursor: TreeCursor,
) -> MacroReferencesBlockCaptures<'a> {
    let mut captures = MacroReferencesBlockCaptures::new();

    let node = cursor.node();
    let lang = node.language();

    let block = NodeKind::Block.into_id(&lang);
    let ctrl_flow_blocks = get_control_flow_block_ids(&lang);
    let macro_container = get_macro_container_expr_ids(&lang);
    let labeled_expr = NodeKind::LabeledExpression.into_id(&lang);

    let cmd = NodeKind::CommandExpression.into_id(&lang);
    let macro_node = NodeKind::Macro.into_id(&lang);
    let macro_def = NodeKind::MacroDefinition.into_id(&lang);
    let param_decl = NodeKind::ParameterDeclaration.into_id(&lang);
    let subroutine_call = NodeKind::SubroutineCallExpression.into_id(&lang);

    let labels: Vec<Range<usize>> = t32.labels.iter().map(|l| l.name.clone()).collect();
    let defs: Vec<Range<usize>> = filter_macro_defs_by_name(text, name, &t32.macros);
    let params: Vec<Range<usize>> = filter_param_declarations_by_name(text, name, &t32.parameters);

    let subroutine_call_ranges: Vec<Range<usize>> = t32
        .calls
        .subroutines
        .iter()
        .map(|c| c.call.clone())
        .collect();

    let script_call_ranges: (Vec<Range<usize>>, Vec<&Uri>) = match &t32.calls.scripts {
        Some(SubscriptCalls { locations, targets }) => {
            let (mut spans, mut files): (Vec<Range<usize>>, Vec<&Uri>) = (Vec::new(), Vec::new());
            for (span, target) in locations.iter().zip(targets) {
                if target.is_some() {
                    spans.push(span.call.clone());
                    files.push(&target.as_ref().unwrap());
                }
            }
            (spans, files)
        }
        None => (Vec::new(), Vec::new()),
    };

    let mut nest_level: i32 = 0;
    'outer: loop {
        let node = cursor.node();
        let (kind, range) = (node.kind_id(), node.byte_range());

        if ctrl_flow_blocks.contains(&kind) || kind == block {
            if cursor.goto_first_child() {
                nest_level += 1;
                continue;
            }
        } else if kind == macro_node {
            if text[range.clone()] == *name {
                captures.references.push(BRange::from(range));
            }
        } else if kind == macro_def
            && defs
                .iter()
                .any(|d| range.contains(&d.start) && range.contains(&d.end))
        {
            // Macro is redefined → Leave the current block
            if !cursor.goto_parent() {
                break;
            }
            nest_level -= 1;
        } else if macro_container.contains(&kind) {
            debug_assert!(macro_container.contains(&cmd));
            if kind == cmd
                && let Some(idx) = script_call_ranges.0.iter().position(|s| *s == range)
            {
                captures.scripts.push(&script_call_ranges.1[idx]);
            }

            if cursor.goto_first_child() {
                nest_level += 1;
                continue;
            }
        } else if kind == labeled_expr && labels.contains(&range) {
            // Subroutines are not automatically checked for references. This
            // needs to happen in a separate iteration.
            if cursor.goto_first_child() {
                nest_level += 1;
                continue;
            }
        } else if kind == subroutine_call {
            if let Some(idx) = subroutine_call_ranges.iter().position(|c| *c == range) {
                captures.subroutines.push(&t32.calls.subroutines[idx]);
            }
        } else if kind == param_decl {
            // This function assumes there is already a valid macro definition.
            // During execution we will only encounter parameter declarations
            // with matching macro name, if there is no prior macro
            // redefinition. Furthermore, parameter declarations will never
            // define macros implicitly, because the macro is already defined.
            // Parameter declarations can only add new macro references.

            for param in params
                .iter()
                .filter(|p| range.contains(&p.start) && range.contains(&p.end))
            {
                captures.references.push(BRange::from(param.clone()));
            }
        }

        while !cursor.goto_next_sibling() {
            if nest_level < 0 || !cursor.goto_parent() {
                break 'outer;
            }
            nest_level -= 1;
        }
    }
    captures
}

fn filter_macro_defs_by_name(
    text: &str,
    name: &str,
    macros: &MacroDefinitions,
) -> Vec<Range<usize>> {
    let mut have_name: Vec<Range<usize>> = Vec::new();

    if let Some(macros) = &macros.privates {
        for MacroDefinition { r#macro, .. } in macros {
            if text[r#macro.clone()] == *name {
                have_name.push(r#macro.clone())
            }
        }
    }

    if let Some(macros) = &macros.locals {
        for MacroDefinition { r#macro, .. } in macros {
            if text[r#macro.clone()] == *name {
                have_name.push(r#macro.clone())
            }
        }
    }

    if let Some(macros) = &macros.globals {
        for MacroDefinition { r#macro, .. } in macros {
            if text[r#macro.clone()] == *name {
                have_name.push(r#macro.clone())
            }
        }
    }
    have_name
}

fn filter_param_declarations_by_name(
    text: &str,
    name: &str,
    params: &Vec<ParameterDeclaration>,
) -> Vec<Range<usize>> {
    debug_assert!(!params.is_empty());

    let mut have_name: Vec<Range<usize>> = Vec::new();

    for ParameterDeclaration { r#macro, .. } in params {
        if text[r#macro.clone()] == *name {
            have_name.push(r#macro.clone())
        }
    }
    have_name
}
