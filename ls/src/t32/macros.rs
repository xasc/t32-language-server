// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use tree_sitter::{Tree, TreeCursor};

use crate::{protocol::Uri, utils::BRange};

use super::{
    FindMacroRefsLangContext,
    ast::{
        KEYWORD_SUBROUTINE_ENTRY, KEYWORD_SUBROUTINE_PARAMETERS, NodeKind, get_block_opener_ids,
        get_control_flow_block_ids, get_macro_container_expr_ids, get_subroutine_ids,
    },
    cache::find_subroutine_for_call,
    expressions::{
        CallExpression, CallSites, MacroDefResolution, MacroDefinition, MacroDefinitions,
        MacroDefinitionsImplicit, MacroScope, ParameterDeclaration,
        RECURSION_BREAKER_SUBROUTINE_SCAN, Subroutine, SubscriptCalls, extract_assign_lhs_macro,
        find_docstring, goto_subroutine,
    },
};

pub struct MacroReferencesBlockCaptures<'a> {
    pub references: Vec<BRange>,
    pub subroutines: Vec<&'a CallExpression>,
    pub scripts: Vec<&'a Uri>,
}

struct MacroDefinitionsCutoff {
    privates: usize,
    locals: usize,
    globals: usize,
    implicit: MacroDefinitionsImplicitCutoff,
}

struct MacroDefinitionsImplicitCutoff {
    privates: usize,
    locals: usize,
}

impl MacroDefinitionsCutoff {
    pub fn set(macros: &MacroDefinitions) -> Self {
        Self {
            privates: macros.privates.len(),
            locals: macros.locals.len(),
            globals: macros.globals.len(),
            implicit: MacroDefinitionsImplicitCutoff {
                privates: macros.implicit.privates.len(),
                locals: macros.implicit.locals.len(),
            },
        }
    }

    pub fn restore(&self, macros: &mut MacroDefinitions) {
        macros.privates.truncate(self.privates);
        macros.locals.truncate(self.locals);
        macros.globals.truncate(self.globals);

        macros.implicit.privates.truncate(self.implicit.privates);
        macros.implicit.locals.truncate(self.implicit.locals);
    }
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

/// Finds all PRACTICE macros with `PRIVATE`, `LOCAL` and `GLOBAL` scope.
/// Implicit definitions are detected both in main script body and subroutines.
///
/// TODO: Visit explicit definitions of subroutines without callers
pub fn find_all_macro_definitions(
    text: &str,
    subroutines: &[Subroutine],
    calls: &[CallExpression],
    tree: &Tree,
) -> MacroDefinitions {
    let mut cursor = tree.walk();

    let (macro_def, assign, call, declaration, id_subroutines, block_openers): (
        u16,
        u16,
        u16,
        u16,
        [u16; 2],
        [u16; 8],
    ) = {
        let lang = tree.language();

        let id_macro_def = NodeKind::MacroDefinition.into_id(&lang);
        let id_assign = NodeKind::AssignmentExpression.into_id(&lang);
        let id_call = NodeKind::SubroutineCallExpression.into_id(&lang);
        let id_declaration = NodeKind::ParameterDeclaration.into_id(&lang);

        let subroutines = get_subroutine_ids(&lang);
        let block_openers = get_block_opener_ids(&lang);

        (
            id_macro_def,
            id_assign,
            id_call,
            id_declaration,
            subroutines,
            block_openers,
        )
    };

    let calls: CallSites = {
        let mut spans: Vec<BRange> = Vec::with_capacity(calls.len());
        let mut targets: Vec<BRange> = Vec::with_capacity(calls.len());

        for CallExpression { call, target, .. } in calls {
            spans.push(call.clone().into());
            targets.push(target.clone().into());
        }
        CallSites::new(spans, targets)
    };

    // Subroutines that have no corresponding subroutine call need to
    // be visited separately. They only have the block-global
    // definitions from the main script body available.
    let subroutines_with_callers: Vec<usize> = subroutines
        .iter()
        .filter(|&s| {
            calls
                .get_targets()
                .iter()
                .any(|t| &text[t.inner().clone()] == &text[s.name.clone()])
        })
        .map(|s| s.definition.start)
        .collect();

    let mut privates: Vec<MacroDefinition> = Vec::new();
    let mut locals: Vec<MacroDefinition> = Vec::new();
    let mut globals: Vec<MacroDefinition> = Vec::new();

    let mut implicit_main: MacroDefinitionsImplicit = MacroDefinitionsImplicit::new();
    let mut implicit_subroutines: MacroDefinitionsImplicit = MacroDefinitionsImplicit::new();

    if !cursor.goto_first_child() {
        return MacroDefinitions::new();
    }

    // Capture all explicit (`PRIVATE`, `LOCAL`, and `GLOBAL`) and implicit
    // macro definitions. All definitions in subroutines are captured in the
    // called sub functions.
    'outer: loop {
        let node = cursor.node();

        let id = node.kind_id();
        let span = BRange::from(node.byte_range());

        if id == macro_def {
            if let Some(scope) = find_macro_scope(text, &mut cursor) {
                let (num, macros) = match scope {
                    MacroScope::Local => (locals.len(), &mut locals),
                    MacroScope::Global => (globals.len(), &mut globals),
                    MacroScope::Private => (privates.len(), &mut privates),
                };

                extract_macro_defs(text, &mut cursor, macros);
                if macros.len() != num {
                    let docstring = find_docstring(&mut cursor);
                    if docstring.is_some() {
                        for def in macros[num..].iter_mut() {
                            def.docstring = docstring.clone();
                        }
                    }
                }
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::MacroDefinition.into_id(&cursor.node().language())
            );
        } else if id == call
            && let Some(target) = calls.get_target(&span)
        {
            let subroutine = subroutines
                .iter()
                .find(|s| text[s.name.clone()] == text[target.inner().clone()]);

            if let Some(sub) = subroutine {
                // `PRIVATE` macros are block-local macros. Other types propagate
                // across subroutine calls.
                let mut macros = MacroDefinitions {
                    privates: Vec::new(),
                    locals: locals.clone(),
                    globals: globals.clone(),
                    implicit: implicit_main.clone(),
                };

                let defs = find_macro_defs_in_subroutine_call_chain(
                    text,
                    tree,
                    subroutines,
                    &calls,
                    BRange::from(sub.definition.clone()),
                    &mut macros,
                    0,
                );

                for def in defs.privates {
                    if !privates.contains(&def) {
                        privates.push(def);
                    }
                }

                for def in defs.locals {
                    if !locals.contains(&def) {
                        locals.push(def);
                    }
                }

                for def in defs.globals {
                    if !globals.contains(&def) {
                        globals.push(def);
                    }
                }
                implicit_subroutines.add(defs.implicit);
            }
        } else if id == assign
            && let Some(span) = extract_assign_lhs_macro(&mut cursor)
        {
            // Macro assignment can create implicit `LOCAL` definitions for
            // macros.
            let name: &str = &text[span.inner().clone()];

            // Check whether the macro is already defined.
            if !(privates
                .iter()
                .chain(locals.iter())
                .chain(globals.iter())
                .any(|d| text[d.r#macro.clone()] == *name)
                || implicit_main
                    .privates
                    .iter()
                    .chain(implicit_main.locals.iter())
                    .any(|d| text[d.inner().clone()] == *name))
            {
                implicit_main.locals.push(span);
            }
        } else if id == declaration {
            let mut parameters: Vec<ParameterDeclaration> = Vec::new();

            extract_params(text, &mut cursor, &mut parameters);

            let ext_block_global_defs_ignored =
                params_ignore_block_global_definitions(text, &mut cursor);

            for param in parameters {
                let name: &str = &text[param.r#macro.clone()];

                // The `PARAMETERS` command can create an implicit `PRIVATE`
                // macro definition. The `ENTRY` command can create implicit
                // `LOCAL` definitions for macros.
                if privates
                    .iter()
                    .chain(locals.iter())
                    .chain(globals.iter())
                    .any(|d| text[d.r#macro.clone()] == *name)
                {
                    continue;
                }

                if implicit_main
                    .privates
                    .iter()
                    .chain(implicit_main.locals.iter())
                    .any(|d| text[d.inner().clone()] == *name)
                {
                    continue;
                }

                if ext_block_global_defs_ignored {
                    implicit_main.privates.push(BRange::from(param.r#macro));
                } else {
                    implicit_main.locals.push(BRange::from(param.r#macro));
                }
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::ParameterDeclaration.into_id(&cursor.node().language())
            );
        } else if id_subroutines.contains(&id)
            && !subroutines_with_callers.contains(&span.inner().start)
        {
            // Subroutines that have no corresponding subroutine call need to
            // be visited separately. They only have the block-global
            // definition context from the main script body available.

            let mut macros = MacroDefinitions {
                privates: Vec::new(),
                locals: locals.clone(),
                globals: globals.clone(),
                implicit: implicit_main.clone(),
            };

            let defs = find_macro_defs_in_subroutine_call_chain(
                text,
                tree,
                subroutines,
                &calls,
                BRange::from(span),
                &mut macros,
                0,
            );

            for def in defs.privates {
                if !privates.contains(&def) {
                    privates.push(def);
                }
            }

            for def in defs.locals {
                if !locals.contains(&def) {
                    locals.push(def);
                }
            }

            for def in defs.globals {
                if !globals.contains(&def) {
                    globals.push(def);
                }
            }
            implicit_subroutines.add(defs.implicit);
        } else if block_openers.contains(&id) {
            if cursor.goto_first_child() {
                continue;
            }
        }

        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'outer;
            }
        }
    }

    let defs: MacroDefinitions = {
        implicit_main.add(implicit_subroutines);

        let mut macros = MacroDefinitions {
            privates,
            locals,
            globals,
            implicit: implicit_main,
        };
        macros.sort();

        macros
    };
    defs
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
    name: &str,
) -> bool {
    if macros
        .privates
        .iter()
        .any(|m| text[m.r#macro.clone()] == *name)
    {
        return true;
    }

    if macros
        .locals
        .iter()
        .any(|m| text[m.r#macro.clone()] == *name)
    {
        return true;
    }

    if parameters.iter().any(|m| text[m.r#macro.clone()] == *name) {
        return true;
    }

    if macros
        .implicit
        .privates
        .iter()
        .chain(macros.implicit.locals.iter())
        .any(|m| text[m.inner().clone()] == *name)
    {
        return true;
    }

    macros
        .globals
        .iter()
        .any(|m| text[m.r#macro.clone()] == *name)
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

pub fn params_ignore_block_global_definitions(text: &str, cursor: &mut TreeCursor) -> bool {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::ParameterDeclaration.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return false;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );
    let command = &text[cursor.node().byte_range()];

    cursor.goto_parent();

    if KEYWORD_SUBROUTINE_ENTRY.eq_ignore_ascii_case(&command) {
        false
    } else {
        debug_assert_eq!(*command, *KEYWORD_SUBROUTINE_PARAMETERS);
        true
    }
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

fn find_macro_defs_in_subroutine_call_chain(
    text: &str,
    tree: &Tree,
    subroutines: &[Subroutine],
    calls: &CallSites,
    body: BRange,
    macros: &mut MacroDefinitions,
    mut level: usize,
) -> MacroDefinitions {
    // Break recursion loops
    level += 1;
    if level > RECURSION_BREAKER_SUBROUTINE_SCAN {
        return MacroDefinitions::new();
    }

    let mut cursor: TreeCursor = goto_subroutine(tree, body.inner().start);

    if !cursor.goto_first_child() {
        return MacroDefinitions::new();
    }

    let (macro_def, assign, call, declaration, block_openers): (u16, u16, u16, u16, [u16; 8]) = {
        let lang = tree.language();

        let id_macro_def = NodeKind::MacroDefinition.into_id(&lang);
        let id_assign = NodeKind::AssignmentExpression.into_id(&lang);
        let id_call = NodeKind::SubroutineCallExpression.into_id(&lang);
        let id_declaration = NodeKind::ParameterDeclaration.into_id(&lang);

        let block_openers = get_block_opener_ids(&lang);

        (
            id_macro_def,
            id_assign,
            id_call,
            id_declaration,
            block_openers,
        )
    };

    let mut privates: Vec<MacroDefinition> = Vec::new();
    let mut locals: Vec<MacroDefinition> = Vec::new();
    let mut globals: Vec<MacroDefinition> = Vec::new();
    let mut implicit: MacroDefinitionsImplicit = MacroDefinitionsImplicit::new();

    let mut defs_subroutines: MacroDefinitions = MacroDefinitions::new();

    'outer: loop {
        let node = cursor.node();
        if node.start_byte() > body.inner().end {
            break 'outer;
        }
        let id = node.kind_id();
        let span = BRange::from(node.byte_range());

        if id == macro_def {
            if let Some(scope) = find_macro_scope(text, &mut cursor) {
                let (num, macros) = match scope {
                    MacroScope::Local => (locals.len(), &mut locals),
                    MacroScope::Global => (globals.len(), &mut globals),
                    MacroScope::Private => (privates.len(), &mut privates),
                };

                extract_macro_defs(text, &mut cursor, macros);
                if macros.len() != num {
                    let docstring = find_docstring(&mut cursor);
                    if docstring.is_some() {
                        for def in macros[num..].iter_mut() {
                            def.docstring = docstring.clone();
                        }
                    }
                }
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::MacroDefinition.into_id(&cursor.node().language())
            );
        } else if id == call
            && let Some(target) = calls.get_target(&span)
        {
            let subroutine = subroutines
                .iter()
                .find(|s| text[s.name.clone()] == text[target.inner().clone()]);

            if let Some(sub) = subroutine {
                let cutoff = MacroDefinitionsCutoff::set(macros);

                // `PRIVATE` macros are block-local macros. Other types propagate
                // across subroutine calls.
                macros.locals.append(&mut locals.clone());
                macros.globals.append(&mut globals.clone());
                macros.implicit.locals.append(&mut implicit.locals.clone());

                let defs = find_macro_defs_in_subroutine_call_chain(
                    text,
                    tree,
                    subroutines,
                    &calls,
                    BRange::from(sub.definition.clone()),
                    macros,
                    level,
                );
                defs_subroutines.add(defs);

                cutoff.restore(macros);
            }
        } else if id == assign
            && let Some(span) = extract_assign_lhs_macro(&mut cursor)
        {
            // Macro assignment can create implicit `LOCAL` definitions for
            // macros.
            let name: &str = &text[span.inner().clone()];

            // Check whether the macro is already defined.
            if !(privates
                .iter()
                .chain(locals.iter())
                .chain(globals.iter())
                .chain(macros.locals.iter())
                .chain(macros.globals.iter())
                .any(|d| text[d.r#macro.clone()] == *name)
                || macros
                    .implicit
                    .locals
                    .iter()
                    .chain(implicit.privates.iter())
                    .chain(implicit.locals.iter())
                    .any(|d| text[d.inner().clone()] == *name))
            {
                implicit.locals.push(span);
            }
        } else if id == declaration {
            let mut parameters: Vec<ParameterDeclaration> = Vec::new();

            extract_params(text, &mut cursor, &mut parameters);

            let ext_block_global_defs_ignored =
                params_ignore_block_global_definitions(text, &mut cursor);

            for param in parameters {
                let name: &str = &text[param.r#macro.clone()];

                if ext_block_global_defs_ignored {
                    // `PARAMETERS` command ignores external block-global macro
                    // definitions. It can create implicit `PRIVATE` definition
                    // for macros.

                    if privates
                        .iter()
                        .chain(locals.iter())
                        .chain(globals.iter())
                        .chain(macros.globals.iter())
                        .any(|d| text[d.r#macro.clone()] == *name)
                    {
                        continue;
                    }

                    if implicit
                        .privates
                        .iter()
                        .chain(implicit.locals.iter())
                        .any(|d| text[d.inner().clone()] == *name)
                    {
                        continue;
                    }
                    implicit.privates.push(BRange::from(param.r#macro));
                } else {
                    // `ENTRY` command Can create implicit `LOCAL` definition
                    // for macros.

                    if privates
                        .iter()
                        .chain(locals.iter())
                        .chain(globals.iter())
                        .chain(macros.locals.iter())
                        .chain(macros.globals.iter())
                        .any(|d| text[d.r#macro.clone()] == *name)
                    {
                        continue;
                    }

                    if implicit
                        .privates
                        .iter()
                        .chain(implicit.locals.iter())
                        .chain(macros.implicit.locals.iter())
                        .any(|d| text[d.inner().clone()] == *name)
                    {
                        continue;
                    }
                    implicit.locals.push(BRange::from(param.r#macro));
                }
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::ParameterDeclaration.into_id(&cursor.node().language())
            );
        } else if block_openers.contains(&id) {
            // Subroutine definitions cannot be nested.
            if cursor.goto_first_child() {
                continue;
            }
        }

        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'outer;
            }
        }
    }

    let mut defs = MacroDefinitions {
        privates,
        locals,
        globals,
        implicit,
    };
    defs.add(defs_subroutines);

    defs
}

fn filter_macro_defs_by_name(
    text: &str,
    name: &str,
    macros: &MacroDefinitions,
) -> Vec<Range<usize>> {
    let mut have_name: Vec<Range<usize>> = Vec::new();

    for MacroDefinition { r#macro, .. } in &macros.privates {
        if text[r#macro.clone()] == *name {
            have_name.push(r#macro.clone())
        }
    }

    for MacroDefinition { r#macro, .. } in &macros.locals {
        if text[r#macro.clone()] == *name {
            have_name.push(r#macro.clone())
        }
    }

    for MacroDefinition { r#macro, .. } in &macros.globals {
        if text[r#macro.clone()] == *name {
            have_name.push(r#macro.clone())
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
