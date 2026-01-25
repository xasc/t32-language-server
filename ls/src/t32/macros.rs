// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Macro Definitions On-Assignment
//! ======================================
//!
//! TRACE32 adds an implicit `LOCAL` definition for macros on first assignment,
//! if no explicit definition is available on the call stack. We are only
//! tracking implicit definitions in the file where the macro reference is
//! located. On the other hand, implicit definitions in scripts that call the
//! file containing the macro reference are not determined.
//! However, implicit macro definitions in calling files are not monitored,
//! because the order in which call sequences are evaluated might result in
//! false-positive results. Calling files are only visited once, so implicit
//! definitions cannot be reliably overruled.
//!
//! ```ignore
//!   Files
//!     - a.cmm, b.cmm: Files with macro reference
//!     - c.cmm: File with explicit definition
//!     - d.cmm: File with macro assignment
//!
//!   Call chain evaluation order
//!
//!     First iteration    Second iteration
//!
//!     a.cmm              b.cmm
//!     v                  v
//!     c.cmm              d.cmm < : 'c.cmm' is not visited anymore. Assignment
//!                        v       : is erroneously detected as implicit macro
//!                        c.cmm   : definition, because the explicit definition
//!                                : is not seen anymore during evaluation.
//! ```
//!
//! Regardless of whether or not there are calling files, we must always assume
//! that any script can be used independently.
//! Assignments and the command `ENTRY` will define an implicit `LOCAL` macro,
//! if the macros has not been defined previously. The command `PARAMETERS`
//! will create a `PRIVATE` macro definition
//!
//!
//! [Note] Ambiguous Macro Definitions
//! ==================================
//!
//! This corner case has two matches for `&a` in the subroutine:
//!   ```ignore
//!   PRIVATE &a
//!   ENTRY &a
//!
//!   If "&a"==""
//!   (
//!     LOCAL &a
//!     &a = "inner"
//!
//!     GOSUB subA
//!     ENDDO
//!   )
//!
//!   subA:
//!   (
//!     PRINT "&a"
//!   )
//!   ```
//! The `PRIVATE` macro definition is used as soon as the body of the if block
//! is not executed. Otherwise, the `LOCAL` macro definition shadows and is
//! active during the subroutine call.
//! This only works, because `subA` is missing a proper return, so one might
//! argue, that this example is malformed.
//!
//!
//! [Note] `GLOBAL` Macro Definitions
//! =================================
//!
//! Once defined, `GLOBAL` macros move to the bottom of the PRACTICE stack.
//! The scope at which their definition is placed does not matter. The only
//! factor is whether a `GLOBAL` macro is already defined or not.
//! Determining whether a `GLOBAL` macro is defined, is not possible by static
//! analysis alone. This is something that can only reliably be done at runtime.
//! To work around this problem we assume that any `GLOBAL` macro with matching
//! name is a possible candidate.
//!
//!
//! [Note] Block-Global vs. Block-Local Macro Definitions
//! =====================================================
//!
//! `LOCAL` macros are block-global. They have local visibility in
//! the current block and global visibility in subroutines and subscripts.
//! Macros with `PRIVATE` lifetime are block-local. They only have local visibility
//! in the current block. `GLOBAL` macros have global scope.
//!

use std::ops::Range;

use tree_sitter::{Node, Tree, TreeCursor};

use crate::{protocol::Uri, utils::BRange};

use super::{
    FindMacroRefsLangContext, GotoDefLangContext, MacroDefinitionResult,
    ast::{
        KEYWORD_SUBROUTINE_ENTRY, KEYWORD_SUBROUTINE_PARAMETERS, NodeKind, get_block_opener_ids,
        get_control_flow_block_ids, get_macro_container_expr_ids, get_subroutine_ids,
    },
    cache::find_subroutine_for_call,
    expressions::{
        CallExpression, MacroDefResolution, MacroDefinition, MacroScope, ParameterDeclaration,
        RECURSION_BREAKER_SUBROUTINE_SCAN, Subroutine, SubscriptCalls, assign_lhs_matches_macro,
        extract_assign_lhs_macro, find_docstring, goto_subroutine, resides_in_subroutine,
    },
};

pub struct MacroReferencesBlockCaptures<'a> {
    pub references: Vec<BRange>,
    pub subroutines: Vec<&'a CallExpression>,
    pub scripts: Vec<&'a Uri>,
}

#[derive(Debug)]
struct CallSites {
    pub origins: Vec<BRange>,
    pub targets: Vec<BRange>,
}

#[derive(Clone, Debug)]
pub struct MacroDefinitions {
    pub privates: Vec<MacroDefinition>,
    pub locals: Vec<MacroDefinition>,
    pub globals: Vec<MacroDefinition>,
    pub implicit: MacroDefinitionsImplicit,
}

#[derive(Clone, Debug)]
pub struct MacroDefinitionsImplicit {
    pub privates: Vec<BRange>,
    pub locals: Vec<BRange>,
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

impl CallSites {
    pub fn new(locations: Vec<BRange>, targets: Vec<BRange>) -> Self {
        Self {
            origins: locations,
            targets,
        }
    }

    #[expect(unused)]
    pub fn get(&self, location: &BRange) -> Option<(&BRange, &BRange)> {
        if let Some(idx) = self.origins.iter().position(|o| *o == *location) {
            Some((&self.origins[idx], &self.targets[idx]))
        } else {
            None
        }
    }

    pub fn get_target(&self, location: &BRange) -> Option<&BRange> {
        if let Some(idx) = self.origins.iter().position(|o| *o == *location) {
            Some(&self.targets[idx])
        } else {
            None
        }
    }

    pub fn get_targets(&self) -> &[BRange] {
        &self.targets
    }
}

impl MacroDefinitions {
    pub fn new() -> Self {
        MacroDefinitions {
            privates: Vec::new(),
            locals: Vec::new(),
            globals: Vec::new(),
            implicit: MacroDefinitionsImplicit::new(),
        }
    }

    pub fn add(&mut self, other: MacroDefinitions) {
        for def in other.privates {
            if !self.privates.contains(&def) {
                self.privates.push(def);
            }
        }

        for def in other.locals {
            if !self.locals.contains(&def) {
                self.locals.push(def);
            }
        }

        for def in other.globals {
            if !self.globals.contains(&def) {
                self.globals.push(def);
            }
        }
        self.implicit.add(other.implicit);
    }

    pub fn sort(&mut self) {
        self.privates.sort();
        self.locals.sort();
        self.globals.sort();
        self.implicit.sort();
    }
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

impl MacroDefinitionsImplicit {
    pub fn new() -> Self {
        Self {
            privates: Vec::new(),
            locals: Vec::new(),
        }
    }

    pub fn add(&mut self, other: MacroDefinitionsImplicit) {
        for def in other.privates {
            if !self.privates.contains(&def) {
                self.privates.push(def);
            }
        }

        for def in other.locals {
            if !self.locals.contains(&def) {
                self.locals.push(def);
            }
        }
    }

    pub fn sort(&mut self) {
        self.privates.sort();
        self.locals.sort();
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
    // macro definitions. All definitions in subroutines are captured
    // separately.
    'outer: loop {
        let node = cursor.node();

        let id = node.kind_id();
        let span = BRange::from(node.byte_range());

        if id == macro_def {
            if let Some(scope) = extract_macro_scope(text, &mut cursor) {
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
                // across subroutine calls. See
                // [Note: Block-Global vs. Block-Local Macro Definitions].
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
            // macros. See [Note: Macro Definitions On-Assignment].
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
                // `LOCAL` definitions for macros. See
                // [Note: Macro Definitions On-Assignment].
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

pub fn find_macro_definition(
    text: &str,
    tree: &Tree,
    t32: &GotoDefLangContext,
    r#macro: Node,
) -> Option<MacroDefinitionResult> {
    debug_assert!(r#macro.end_byte() < text.len());
    debug_assert_eq!(
        r#macro.kind_id(),
        NodeKind::Macro.into_id(&r#macro.language()),
    );

    let mut cursor = tree.walk();
    if !cursor.goto_first_child() {
        return None;
    }

    let name = &text[r#macro.start_byte()..r#macro.end_byte()];

    let defs =
        locate_all_macro_definitions(text, t32, &BRange::from(r#macro.byte_range()), name, tree);
    eval_macro_definition_result(defs)
}

pub fn find_external_macro_definition(
    text: &str,
    tree: &Tree,
    t32: &GotoDefLangContext,
    r#macro: &String,
    callees: Vec<BRange>,
) -> Option<MacroDefinitionResult> {
    let mut defs: Vec<MacroDefResolution> = Vec::new();
    for callee in callees {
        defs.append(&mut locate_all_macro_definitions(
            text, t32, &callee, &r#macro, tree,
        ));
    }
    eval_macro_definition_result(defs)
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

/// Find PRACTICE macro definitions for origins that are located is in a
/// subroutine, but it is externally defined. This is only possible for `LOCAL`
/// and `GLOBAL` macros. Subroutine definitions cannot be nested.
pub fn find_definition_for_macro_in_subroutine(
    text: &str,
    t32: &GotoDefLangContext,
    origin: &Range<usize>,
    subroutine: &Subroutine,
    name: &str,
    tree: &Tree,
) -> Option<Vec<MacroDefResolution>> {
    debug_assert!(!t32.subroutines.is_empty());

    if t32.subroutines.is_empty() {
        return None;
    }

    let calls = if t32.calls.subroutines.len() > 0 {
        &t32.calls.subroutines
    } else {
        return None;
    };

    // See [Note: `GLOBAL` Macro Definitions]
    let mut globals: Vec<MacroDefResolution> = Vec::new();
    if !t32.macros.globals.is_empty() {
        for r#macro in t32
            .macros
            .globals
            .iter()
            .filter(|m| text[m.r#macro.clone()] == *name)
        {
            globals.push(MacroDefResolution::Final(r#macro.clone()));
        }
    }

    // Macro definitions for subroutines can be ambiguous
    // (see [Note: Ambiguous Macro Definitions]), so we need to cover both all
    // paths out of the subroutine and from the outside in.
    //
    // 1. Check for macro definition in subroutine body.
    // 2. Find all macro definitions for subroutine calls. The `PARAMETERS`
    //    command cannot define macros for called subroutines.
    // 3. Check for a global macro definition that is covering the subroutine.
    let inner = find_any_macro_def_in_subroutine_body(
        text,
        origin,
        name,
        goto_subroutine(&tree, subroutine.definition.start),
    );

    if let Some(MacroDefResolution::Final(_)) = inner {
        return Some(vec![inner.unwrap()]);
    }

    let mut outer = find_macro_def_covering_subroutine_call(
        0,
        text,
        &subroutine.name,
        name,
        calls,
        &t32.subroutines,
        tree,
    );

    let num = outer.len();
    outer.retain(|m| *m != MacroDefResolution::Indeterminate);

    if num != outer.len() {
        if let Some(MacroDefResolution::Overridable(_)) = inner {
            // The subroutine has an `ENTRY` command. It defines the macro if no
            // other global definition is present.
            outer.push(inner.unwrap());
        } else if let Some(MacroDefResolution::Implicit(_)) = inner {
            // The subroutine features an implicit macro definition. It defines
            // the macro if any of the higher levels on the call tree is
            // lacking a corresponding definition.
            outer.push(inner.unwrap());
        }
    }

    if let Some(definition) =
        find_explicit_or_implicit_macro_def_in_script(text, origin, name, tree)
    {
        if !outer.contains(&definition) {
            outer.push(definition)
        }
    }

    if outer.len() > 0 {
        Some(outer)
    } else if globals.len() > 0 {
        Some(globals)
    } else {
        None
    }
}

fn find_any_macro_def_in_subroutine_body(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    subroutine: TreeCursor,
) -> Option<MacroDefResolution> {
    find_any_macro_def_in_outer_block(
        text,
        origin,
        name,
        subroutine,
        defines_any_macro,
        may_define_macro_implicitly,
    )
}

fn find_block_global_macro_def_in_subroutine_body(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    subroutine: TreeCursor,
) -> Option<MacroDefResolution> {
    find_any_macro_def_in_outer_block(
        text,
        origin,
        name,
        subroutine,
        defines_block_global_macro,
        defines_global_macro_implicitly,
    )
}

/// Recursively resolves subroutine calls from innermost to outermost until all
/// matching macro definitions are found. Implicit declarations via `ENTRY` can
/// only be resolved once outer scopes have been checked. If there is a
/// matching macro definition in one of the outer scopes, the `ENTRY` definition
/// will remain inactive. Otherwise (one path without definition is sufficient),
/// the `ENTRY` command defines a `LOCAL` macro.
fn find_macro_def_covering_subroutine_call(
    mut level: usize,
    text: &str,
    origin: &Range<usize>,
    name: &str,
    calls: &Vec<CallExpression>,
    subroutines: &Vec<Subroutine>,
    tree: &Tree,
) -> Vec<MacroDefResolution> {
    // Break recursion loops
    level += 1;
    if level > RECURSION_BREAKER_SUBROUTINE_SCAN {
        return vec![MacroDefResolution::Aborted];
    }

    let mut defs: Vec<MacroDefResolution> = Vec::new();
    for target in calls
        .iter()
        .filter(|&c| text[c.target.clone()] == text[origin.clone()])
    {
        let subroutine = subroutines
            .iter()
            .find(|s| s.definition.contains(&target.call.start));

        let root: TreeCursor = match subroutine {
            Some(sub) => goto_subroutine(&tree, sub.definition.start),
            None => tree.walk(),
        };

        let inner = find_block_global_macro_def_in_subroutine_body(text, &target.call, name, root);

        if let Some(MacroDefResolution::Final(definition)) = inner {
            // Macro definition was found in subroutine body.
            defs.push(MacroDefResolution::Final(definition));
        } else if let Some(sub) = subroutine {
            // Call is nested in another subroutine. Find all macro definitions
            // in outer scopes. Returns all macro definitions on higher levels
            // of the call tree.
            let mut macros = find_macro_def_covering_subroutine_call(
                level,
                text,
                &sub.name,
                name,
                calls,
                subroutines,
                tree,
            );
            debug_assert!(macros.len() > 0);

            if let Some(MacroDefResolution::Overridable(definition)) = inner {
                // The subroutine has an `ENTRY` command. We need to check
                // whether it defines the macro.
                if let Some(idx) = macros
                    .iter()
                    .position(|m| *m == MacroDefResolution::Indeterminate)
                {
                    macros[idx] = MacroDefResolution::Overridable(definition);
                }
            } else if let Some(MacroDefResolution::Implicit(span)) = inner {
                // The subroutine has may have an implicit macro definition. It
                // becomes active, if it is not overruled by all callers.
                if let Some(idx) = macros
                    .iter()
                    .position(|m| *m == MacroDefResolution::Indeterminate)
                {
                    macros[idx] = MacroDefResolution::Implicit(span);
                }
            }
            defs.append(&mut macros);
        } else if let Some(MacroDefResolution::Implicit(span)) = inner {
            // Call is not in a subroutine and only an implicit macro
            // definition could be found.
            defs.push(MacroDefResolution::Implicit(span));
        } else if let None = inner {
            // Subroutine call is not in a subroutine and no matching macro
            // definition was found. `ENTRY` commands on lower scopes might now
            // become active.
            defs.push(MacroDefResolution::Indeterminate);
        }
    }
    defs
}

fn find_explicit_or_implicit_macro_def_in_script(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<MacroDefResolution> {
    find_explicit_or_implicit_macro_def_in_block(text, origin, name, tree.walk(), defines_any_macro)
}

/// Find PRACTICE macro definitions in a parent block relative to the origin
/// node. Any explicit macro type (`PRIVATE`, `LOCAL`, `GLOBAL`) works. If no
/// explicit definition can be found, we locate the place where the macro is
/// used first.
pub fn find_explicit_or_implicit_macro_def(
    text: &str,
    globals: &[MacroDefinition],
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<Vec<MacroDefResolution>> {
    if let Some(def) = find_explicit_or_implicit_macro_def_in_block(
        text,
        origin,
        name,
        tree.walk(),
        defines_any_macro,
    ) {
        Some(vec![def])
    } else if !globals.is_empty() {
        // See [Note: `GLOBAL` Macro Definitions]
        let mut defs = Vec::new();
        for r#macro in globals.iter().filter(|m| text[m.r#macro.clone()] == *name) {
            defs.push(MacroDefResolution::Final(r#macro.clone()));
        }
        if defs.len() > 0 { Some(defs) } else { None }
    } else {
        None
    }
}

fn locate_all_macro_definitions(
    text: &str,
    t32: &GotoDefLangContext,
    origin: &BRange,
    name: &str,
    tree: &Tree,
) -> Vec<MacroDefResolution> {
    let mut defs: Vec<MacroDefResolution> = Vec::new();
    if let Some(subroutine) = resides_in_subroutine(&t32.subroutines, origin.inner().start) {
        if let Some(mut macros) = find_definition_for_macro_in_subroutine(
            text,
            t32,
            origin.inner(),
            subroutine,
            name,
            tree,
        ) {
            defs.append(&mut macros);
        }
    } else if let Some(mut macros) =
        find_explicit_or_implicit_macro_def(text, &t32.macros.globals, origin.inner(), name, tree)
    {
        defs.append(&mut macros);
    }
    defs
}

fn eval_macro_definition_result(
    mut defs: Vec<MacroDefResolution>,
) -> Option<MacroDefinitionResult> {
    if defs.len() <= 0 {
        return Some(MacroDefinitionResult::Indeterminate);
    }

    if defs.len() == 1 && defs[0] == MacroDefResolution::Aborted {
        return None;
    }
    defs.retain(|m| *m != MacroDefResolution::Aborted);

    let num = defs.len();
    if num <= 0 {
        Some(MacroDefinitionResult::Indeterminate)
    } else {
        // Other files are only checked for macro definitions if none is
        // found in the current file. This reduces the effort for finding
        // a matching definition.
        defs.retain(|m| *m != MacroDefResolution::Indeterminate);
        if defs.len() <= 0 {
            return Some(MacroDefinitionResult::Indeterminate);
        }

        let contains_unresolved = defs.len() != num
            || defs.iter().any(|d| {
                if let MacroDefResolution::Implicit(_) = d {
                    true
                } else {
                    false
                }
            });

        let mut gotos: Vec<MacroDefinition> = Vec::with_capacity(defs.len());
        for def in defs {
            gotos.push(match def {
                MacroDefResolution::Final(d)
                | MacroDefResolution::Overridable(d)
                | MacroDefResolution::Implicit(d) => d,
                MacroDefResolution::Indeterminate | MacroDefResolution::Aborted => unreachable!(),
            });
        }

        if !contains_unresolved {
            Some(MacroDefinitionResult::Final(gotos))
        } else {
            Some(MacroDefinitionResult::Partial(gotos))
        }
    }
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

    let params: Vec<Range<usize>> = {
        if t32.parameters.is_empty() {
            Vec::new()
        } else {
            filter_param_declarations_by_name(text, name, &t32.parameters)
        }
    };

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

// TODO: Check whether `ENTRY` or `PARAMETERS` in the main path of scripts are handled properly.
fn find_explicit_or_implicit_macro_def_in_block(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    root: TreeCursor,
    select_macro: fn(text: &str, def: &mut TreeCursor, name: &str) -> Option<MacroDefinition>,
) -> Option<MacroDefResolution> {
    let (macro_def, assign): (u16, u16) = {
        let node = root.node();
        let lang = node.language();
        (
            NodeKind::MacroDefinition.into_id(&lang),
            NodeKind::AssignmentExpression.into_id(&lang),
        )
    };
    let mut definition: Option<MacroDefResolution> = None;

    let mut cursor = root;
    loop {
        let node = cursor.node();
        if node.start_byte() > origin.end {
            break;
        }
        let id = node.kind_id();

        if id == macro_def {
            if let Some(mut def) = select_macro(&text, &mut cursor, name) {
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    def.docstring = docstring;
                }
                definition = Some(MacroDefResolution::Final(def));
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::MacroDefinition.into_id(&cursor.node().language())
            );
        } else if definition.is_none()
            && id == assign
            && let Some(span) = assign_lhs_matches_macro(text, name, &mut cursor)
        {
            // On the left hand side of assignment expressions, `LOCAL` macros
            // can be defined implicitly.
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::AssignmentExpression.into_id(&cursor.node().language())
            );
            definition = Some(MacroDefResolution::Implicit(MacroDefinition {
                cmd: cursor.node().byte_range(),
                r#macro: span.into(),
                docstring: find_docstring(&mut cursor),
            }));
        } else if node.byte_range().contains(&origin.start) {
            if cursor.goto_first_child() {
                continue;
            }
        }

        if !cursor.goto_next_sibling() {
            if !(cursor.goto_parent() && cursor.goto_next_sibling()) {
                break;
            }
        }
    }
    definition
}

fn find_any_macro_def_in_outer_block(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    root: TreeCursor,
    select_macro: fn(text: &str, def: &mut TreeCursor, name: &str) -> Option<MacroDefinition>,
    select_params: fn(text: &str, def: &mut TreeCursor, name: &str) -> Option<MacroDefResolution>,
) -> Option<MacroDefResolution> {
    let (macro_def, parameter, assign): (u16, u16, u16) = {
        let node = root.node();
        let lang = node.language();

        let macro_def = NodeKind::MacroDefinition.into_id(&lang);
        let parameter = NodeKind::ParameterDeclaration.into_id(&lang);
        let assign = NodeKind::AssignmentExpression.into_id(&lang);

        (macro_def, parameter, assign)
    };
    let mut definition: Option<MacroDefResolution> = None;

    let mut cursor = root;
    loop {
        let node = cursor.node();
        if node.start_byte() > origin.end {
            break;
        }
        let id = node.kind_id();

        if id == macro_def {
            if let Some(mut def) = select_macro(&text, &mut cursor, name) {
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    def.docstring = docstring;
                }
                definition = Some(MacroDefResolution::Final(def));
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::MacroDefinition.into_id(&cursor.node().language())
            );
        } else if definition.as_ref().is_none_or(|d| {
            if let MacroDefResolution::Implicit(_) = d {
                true
            } else {
                false
            }
        }) && id == parameter
        {
            if let Some(mut def) = select_params(&text, &mut cursor, name) {
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    match &mut def {
                        MacroDefResolution::Final(m) | MacroDefResolution::Overridable(m) => {
                            m.docstring = docstring
                        }
                        _ => (),
                    }
                }
                definition = Some(def);
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::ParameterDeclaration.into_id(&cursor.node().language())
            );
        } else if definition.is_none()
            && id == assign
            && let Some(span) = assign_lhs_matches_macro(text, name, &mut cursor)
        {
            // On the left hand side of assignment expressions, `LOCAL` macros
            // can be defined implicitly.
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::AssignmentExpression.into_id(&cursor.node().language())
            );
            definition = Some(MacroDefResolution::Implicit(MacroDefinition {
                cmd: cursor.node().byte_range(),
                r#macro: span.into(),
                docstring: find_docstring(&mut cursor),
            }));
        } else if node.byte_range().contains(&origin.start) {
            if cursor.goto_first_child() {
                continue;
            }
        }

        if !cursor.goto_next_sibling() {
            if !(cursor.goto_parent() && cursor.goto_next_sibling()) {
                break;
            }
        }
    }
    definition
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
            if let Some(scope) = extract_macro_scope(text, &mut cursor) {
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

fn extract_macro_scope(text: &str, cursor: &mut TreeCursor) -> Option<MacroScope> {
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
