// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # [Note] Macro Definitions On-Assignment
//!
//! We ignore implicit definition of macros on first assignment. This only
//! happens if there is no other definition for the macro. TRACE32 uses
//! `LOCAL` macro definition on-assignment definitions.
//!
//!
//! # [Note] Ambiguous Macro Definitions
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
//! # [Note] `GLOBAL` Macro Definitions
//!
//! Once defined, `GLOBAL` macros move to the bottom of the PRACTICE stack.
//! The scope at which their definition is placed does not matter. The only
//! factor is whether a `GLOBAL` macro is already defined or not.

use std::ops::Range;

use tree_sitter::{Range as TRange, Tree, TreeCursor};

use crate::{
    protocol::Uri,
    t32::{
        FileIndex, LangExpressions, NodeKind,
        ast::{
            KEYWORD_SUBROUTINE_ENTRY, KEYWORD_SUBROUTINE_PARAMETERS, KEYWORDS_SCRIPT_CALL,
            KEYWORDS_SCRIPT_END, get_block_opener_ids, get_string_body, get_subroutine_ids,
            node_into_id, start_on_adjacent_lines,
        },
        path::locate_script,
    },
};

/// `ENTRY` implicitly defines a `LOCAL` macro, but only if no `LOCAL`, `ENTRY`
/// or `GLOBAL` definition is present in one of the calling scopes. On the
/// other hand, `PARAMETERS` ignores all external macro definitions.
#[derive(Debug, PartialEq)]
pub enum MacroDefResolution {
    /// Macro defined by `PRIVATE`, `LOCAL`, `GLOBAL`, or `PARAMETERS`.
    Final(MacroDefinition),
    /// Macro implicitly defined by `ENTRY`.
    Overridable(MacroDefinition),
    /// No macro definition in file scope found. Definition might be provided by caller.
    Indeterminate,
    /// No macro definition found.
    Unresolved,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MacroDefinition {
    pub cmd: Range<usize>,
    pub r#macro: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[derive(Clone, Debug)]
pub struct Subroutine {
    pub name: Range<usize>,
    pub definition: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[derive(Clone, Debug)]
pub struct CallExpression {
    pub target: Range<usize>,
    pub call: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[derive(Clone, Debug)]
pub struct ParameterDeclaration {
    #[allow(dead_code)]
    pub cmd: Range<usize>,

    pub r#macro: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[derive(Clone, Debug)]
pub struct MacroDefinitions {
    pub locals: Option<Vec<MacroDefinition>>,
    pub globals: Option<Vec<MacroDefinition>>,
}

#[derive(Clone, Debug)]
pub struct CallExpressions {
    pub subroutines: Option<Vec<CallExpression>>,
    pub scripts: Option<SubscriptCalls>,
}

#[derive(Clone, Debug)]
pub struct CallLocations {
    pub subroutines: Option<Vec<CallExpression>>,
    pub scripts: Option<Vec<CallExpression>>,
}

#[derive(Clone, Debug)]
pub struct SubscriptCalls {
    pub locations: Vec<CallExpression>,
    pub targets: Vec<Option<Uri>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MacroScope {
    Global,
    Local,
    Private,
}

impl MacroDefinitions {
    pub fn build(locals: Vec<MacroDefinition>, globals: Vec<MacroDefinition>) -> Self {
        let locals: Option<Vec<MacroDefinition>> = match locals {
            loc if loc.len() <= 0 => None,
            loc => Some(loc),
        };

        let globals: Option<Vec<MacroDefinition>> = match globals {
            g if g.len() <= 0 => None,
            g => Some(g),
        };
        MacroDefinitions { locals, globals }
    }
}

impl CallExpressions {
    pub fn build(
        subroutines: Option<Vec<CallExpression>>,
        scripts: Option<SubscriptCalls>,
    ) -> Self {
        let subroutines: Option<Vec<CallExpression>> = match subroutines {
            Some(calls) if calls.len() <= 0 => None,
            Some(calls) => Some(calls),
            None => None,
        };

        let scripts: Option<SubscriptCalls> = match scripts {
            Some(calls) if calls.locations.len() <= 0 => None,
            Some(calls) => Some(calls),
            None => None,
        };
        CallExpressions {
            subroutines,
            scripts,
        }
    }
}

impl CallLocations {
    pub fn build(subroutines: Vec<CallExpression>, scripts: Vec<CallExpression>) -> Self {
        let subroutines: Option<Vec<CallExpression>> = match subroutines {
            calls if calls.len() <= 0 => None,
            calls => Some(calls),
        };

        let scripts: Option<Vec<CallExpression>> = match scripts {
            calls if calls.len() <= 0 => None,
            calls => Some(calls),
        };

        CallLocations {
            subroutines,
            scripts,
        }
    }
}

impl SubscriptCalls {
    pub fn build(locations: Vec<CallExpression>, targets: Vec<Option<Uri>>) -> Self {
        SubscriptCalls { locations, targets }
    }
}

impl From<&str> for MacroScope {
    fn from(keyword: &str) -> Self {
        match keyword {
            "GLOBAL" => MacroScope::Global,
            "LOCAL" => MacroScope::Local,
            "PRIVATE" => MacroScope::Private,
            &_ => unreachable!("No other variant exists."),
        }
    }
}

pub fn find_macro_definition(
    text: &str,
    tree: &Tree,
    t32: &LangExpressions,
    r#macro: TreeCursor,
) -> Option<Vec<MacroDefResolution>> {
    let node = r#macro.node();

    debug_assert!(node.end_byte() < text.len());
    debug_assert_eq!(node.kind_id(), NodeKind::Macro.into_id(&node.language()),);

    let mut cursor = tree.walk();
    if !cursor.goto_first_child() {
        return None;
    }

    let name = &text[node.start_byte()..node.end_byte()];

    let mut defs: Vec<MacroDefResolution> = Vec::with_capacity(1);
    if let Some(subroutine) = resides_in_subroutine(&t32.subroutines, node.start_byte()) {
        if let Some(mut macros) = find_definition_for_macro_in_subroutine(
            text,
            t32,
            &node.byte_range(),
            subroutine,
            name,
            tree,
        ) {
            defs.append(&mut macros);
        }
    } else if let Some(mut macros) =
        find_explicit_macro_def(text, &t32.macros.globals, &node.byte_range(), name, tree)
    {
        defs.append(&mut macros);
    }

    if defs.len() > 0 { Some(defs) } else { None }
}

pub fn find_subroutine_definition(
    text: &str,
    subroutines: &Vec<Subroutine>,
    mut call: TreeCursor,
) -> Option<Subroutine> {
    let node = call.node();

    debug_assert!(node.end_byte() < text.len());
    debug_assert_eq!(
        node.kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&node.language()),
    );

    let CallExpression { target, .. } = extract_subroutine_call(&mut call)?;
    let name = &text[target];

    for subroutine in subroutines {
        if text[subroutine.name.clone()] == *name {
            return Some(subroutine.clone());
        }
    }
    None
}

pub fn find_file_target(calls: &SubscriptCalls, command: TreeCursor) -> Option<Uri> {
    debug_assert_eq!(
        command.node().kind_id(),
        NodeKind::CommandExpression.into_id(&command.node().language()),
    );
    let span = command.node().byte_range();

    for (loc, target) in calls
        .locations
        .iter()
        .zip(calls.targets.iter())
        .filter(|c| c.1.is_some())
    {
        if span.contains(&loc.target.start) {
            return Some(target.as_ref().unwrap().clone());
        }
    }
    None
}

/// Finds all PRACTICE macros with `LOCAL` and `GLOBAL` scope.
pub fn find_all_global_macro_definitions(text: &str, tree: &Tree) -> MacroDefinitions {
    let mut cursor = tree.walk();

    let lang = tree.language();
    let block_openers = get_block_opener_ids(&lang);

    let id_macro_def = node_into_id(&tree.language(), NodeKind::MacroDefinition);

    let mut locals: Vec<MacroDefinition> = Vec::new();
    let mut globals: Vec<MacroDefinition> = Vec::new();

    if !cursor.goto_first_child() {
        return MacroDefinitions::build(locals, globals);
    }

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == id_macro_def {
            if let Some(scope) = find_macro_scope(text, &mut cursor) {
                if scope != MacroScope::Private {
                    let (num, macros) = match scope {
                        MacroScope::Local => (locals.len(), &mut locals),
                        MacroScope::Global => (globals.len(), &mut globals),
                        MacroScope::Private => unreachable!(),
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
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
            );
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
    MacroDefinitions::build(locals, globals)
}

pub fn find_all_subroutines(_text: &str, tree: &Tree) -> Option<Vec<Subroutine>> {
    let id_subroutines = get_subroutine_ids(&tree.language());

    let mut cursor = tree.walk();
    if !cursor.goto_first_child() {
        return None;
    }

    let mut subroutines: Vec<Subroutine> = Vec::new();
    loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id_subroutines.contains(&id) {
            let subroutine = extract_subroutine_def(&mut cursor);
            debug_assert!(id_subroutines.contains(&cursor.node().kind_id()));

            if subroutine.is_some() {
                let mut subroutine = subroutine.unwrap();
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    subroutine.docstring = docstring;
                }
                subroutines.push(subroutine);
            }
            debug_assert!(id_subroutines.contains(&cursor.node().kind_id()));
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    if subroutines.len() > 0 {
        Some(subroutines)
    } else {
        None
    }
}

pub fn find_all_call_expressions(text: &str, tree: &Tree) -> CallLocations {
    let mut cursor = tree.walk();

    let mut subroutines: Vec<CallExpression> = Vec::new();
    let mut scripts: Vec<CallExpression> = Vec::new();

    if !cursor.goto_first_child() {
        return CallLocations::build(subroutines, scripts);
    }

    let lang = tree.language();

    let subroutine_call = NodeKind::SubroutineCallExpression.into_id(&lang);
    let script_call = NodeKind::CommandExpression.into_id(&lang);
    let block_openers = get_block_opener_ids(&lang);

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == subroutine_call {
            let call = extract_subroutine_call(&mut cursor);
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::SubroutineCallExpression.into_id(&lang)
            );

            if call.is_some() {
                let mut call = call.unwrap();

                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    call.docstring = docstring;
                }
                subroutines.push(call);
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::SubroutineCallExpression.into_id(&lang)
            );
        } else if id == script_call {
            let call = extract_script_call(text, &mut cursor);
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::CommandExpression.into_id(&lang)
            );

            if call.is_some() {
                let mut call = call.unwrap();

                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    call.docstring = docstring;
                }
                scripts.push(call);
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::CommandExpression.into_id(&lang)
            );
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
    CallLocations::build(subroutines, scripts)
}

pub fn find_all_parameter_declarations(text: &str, tree: &Tree) -> Option<Vec<ParameterDeclaration>> {
    let mut cursor = tree.walk();

    let mut parameters: Vec<ParameterDeclaration> = Vec::new();

    if !cursor.goto_first_child() {
        return None;
    }

    let lang = tree.language();

    let declaration = NodeKind::ParameterDeclaration.into_id(&lang);
    let block_openers = get_block_opener_ids(&lang);

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == declaration {
            let num = parameters.len();
            extract_params(text, &mut cursor, &mut parameters);
            if parameters.len() != num {
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    for def in parameters[num..].iter_mut() {
                        def.docstring = docstring.clone();
                    }
                }
            }
            debug_assert_eq!(
                cursor.node().kind_id(),
                NodeKind::ParameterDeclaration.into_id(&lang)
            );
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
    if parameters.len() > 0 { Some(parameters) } else { None }
}

pub fn locate_subscript(
    text: &str,
    tree: &Tree,
    target: usize,
    files: &FileIndex,
) -> Option<Vec<Uri>> {
    let mut cursor = tree.walk();

    let lang = tree.language();
    let cmd = NodeKind::CommandExpression.into_id(&lang);

    if cursor.goto_first_child_for_byte(target).is_none() || cursor.node().kind_id() != cmd {
        return None;
    }

    let args = NodeKind::ArgumentList.into_id(&lang);

    if cursor.goto_first_child_for_byte(target).is_none()
        || cursor.node().kind_id() != args
        || !cursor.goto_first_child()
    {
        return None;
    }

    let path: String = loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == NodeKind::Path.into_id(&lang) {
            break text[node.byte_range()].to_string();
        } else if id == NodeKind::String.into_id(&lang) {
            break get_string_body(&node, &text).to_string();
        }

        if !cursor.goto_next_sibling() {
            return None;
        }
    };
    locate_script(&path, &files)
}

/// Find PRACTICE macro definitions where the origin is in a subroutine, but
/// it is externally defined. This is only possible for `LOCAL` and `GLOBAL`
/// macros. Subroutine definitions cannot be nested.
pub fn find_definition_for_macro_in_subroutine(
    text: &str,
    t32: &LangExpressions,
    origin: &Range<usize>,
    subroutine: &Subroutine,
    name: &str,
    tree: &Tree,
) -> Option<Vec<MacroDefResolution>> {
    debug_assert!(t32.subroutines.is_some());

    let Some(subroutines) = &t32.subroutines else {
        return None;
    };

    let Some(calls) = &t32.calls.subroutines else {
        return None;
    };

    let mut locals: Vec<MacroDefinition> = Vec::new();
    if let Some(macros) = &t32.macros.locals {
        for r#macro in macros.iter().filter(|m| text[m.r#macro.clone()] == *name) {
            locals.push(r#macro.clone());
        }
    }

    // See [Note: `GLOBAL` Macro Definitions]
    let mut globals: Vec<MacroDefResolution> = Vec::new();
    if let Some(macros) = &t32.macros.globals {
        for r#macro in macros.iter().filter(|m| text[m.r#macro.clone()] == *name) {
            globals.push(MacroDefResolution::Final(r#macro.clone()));
        }
    }

    let mut params: Vec<ParameterDeclaration> = Vec::new();
    if let Some(parameters) = &t32.parameters {
        for decl in parameters.iter().filter(|m| text[m.r#macro.clone()] == *name) {
            params.push(decl.clone());
        }
    }

    if locals.len() <= 0 && globals.len() <= 0 && params.len() <= 0 {
        return None;
    }

    // Macro definitions for subroutines can be ambiguous
    // (see [Note: Ambiguous Macro Definitions]), so we need to cover both all
    // paths out of the subroutine and from the outside in.
    //
    // 1. Check for macro definition in subroutine body.
    // 2. Find all macro definitions for subroutine calls. `PARAMETERS` cannot
    //    define macros for called subroutines.
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

    let mut defs = find_macro_def_covering_subroutine_call(
        0,
        text,
        &subroutine.name,
        name,
        calls,
        subroutines,
        tree,
    );

    let num = defs.len();
    defs.retain(|m| *m != MacroDefResolution::Indeterminate);
    if let Some(MacroDefResolution::Overridable(_)) = inner {
        // The subroutine has an `ENTRY` command. It defines the macro if no
        // other global definition is present.
        if num != defs.len() {
            defs.push(inner.unwrap());
        }
    }

    if let Some(definition) = find_explicit_macro_def_in_script(text, origin, name, tree) {
        if !defs.contains(&definition) {
            defs.push(definition)
        }
    }

    if defs.len() > 0 {
        Some(defs)
    } else if globals.len() > 0 {
        Some(globals)
    } else {
        None
    }
}

/// Find PRACTICE macro definitions in a parent block relative to the origin node. Any
/// explicit macro type (`PRIVATE`, `LOCAL`, `GLOBAL`) works.
fn find_explicit_macro_def(
    text: &str,
    globals: &Option<Vec<MacroDefinition>>,
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<Vec<MacroDefResolution>> {
    if let Some(def) = find_explicit_macro_def_in_block(text, origin, name, tree.walk(), defines_any_macro) {
        Some(vec![def])
    } else if let Some(globals) = &globals {
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
        defines_macro_implicitly,
    )
}

fn find_global_macro_def_in_subroutine_body(
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
        defines_global_macro,
        defines_global_macro_implicitly,
    )
}

fn find_explicit_macro_def_in_script(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<MacroDefResolution> {
    find_explicit_macro_def_in_block(text, origin, name, tree.walk(), defines_any_macro)
}

fn find_explicit_macro_def_in_block(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    root: TreeCursor,
    select_macro: fn(text: &str, def: &mut TreeCursor, name: &str) -> Option<MacroDefinition>,
) -> Option<MacroDefResolution> {
    let macro_def = NodeKind::MacroDefinition.into_id(&root.node().language());

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
                node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
            );
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
    let macro_def = NodeKind::MacroDefinition.into_id(&root.node().language());
    let parameter = NodeKind::ParameterDeclaration.into_id(&root.node().language());

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
                node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
            );
        } else if definition.is_none() && id == parameter {
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
                node_into_id(&cursor.node().language(), NodeKind::ParameterDeclaration)
            );
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
    if level > 20 {
        return vec![MacroDefResolution::Unresolved];
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

        let inner = find_global_macro_def_in_subroutine_body(text, &target.call, name, root);

        if let Some(MacroDefResolution::Final(definition)) = inner {
            // Macro definition was found in subroutine body.
            defs.push(MacroDefResolution::Final(definition));
        } else if let Some(sub) = subroutine {
            // Call is nested in another subroutine. Find all macro definitions
            // in outer scopes.
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
                let num = macros.len();
                macros.retain(|m| *m != MacroDefResolution::Indeterminate);
                if num != macros.len() {
                    defs.push(MacroDefResolution::Overridable(definition));
                }
            }
            defs.append(&mut macros);
        } else if let None = inner {
            // Subroutine call is not in a subroutine and no matching macro
            // definition was found. `ENTRY` commands on lower scopes might now
            // become active.
            defs.push(MacroDefResolution::Indeterminate)
        }
    }
    defs
}

fn defines_any_macro(text: &str, cursor: &mut TreeCursor, name: &str) -> Option<MacroDefinition> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            node_into_id(&r#macro.language(), NodeKind::Macro)
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

fn defines_global_macro(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefinition> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );
    if MacroScope::from(&text[cursor.node().byte_range()]) == MacroScope::Private {
        cursor.goto_parent();
        return None;
    }

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            node_into_id(&r#macro.language(), NodeKind::Macro)
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

fn defines_macro_implicitly(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefResolution> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::ParameterDeclaration)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
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
            node_into_id(&r#macro.language(), NodeKind::Macro)
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

fn defines_global_macro_implicitly(
    text: &str,
    cursor: &mut TreeCursor,
    name: &str,
) -> Option<MacroDefResolution> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::ParameterDeclaration)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
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
            node_into_id(&r#macro.language(), NodeKind::Macro)
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

fn extract_macro_defs(text: &str, cursor: &mut TreeCursor, macros: &mut Vec<MacroDefinition>) {
    let def = cursor.node();
    debug_assert_eq!(
        def.kind_id(),
        node_into_id(&def.language(), NodeKind::MacroDefinition)
    );

    if !cursor.goto_first_child() {
        return;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&def.language(), NodeKind::Identifier)
    );
    debug_assert!(
        [MacroScope::Local, MacroScope::Global]
            .contains(&MacroScope::from(&text[cursor.node().byte_range()]))
    );

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();

        debug_assert_eq!(
            r#macro.kind_id(),
            node_into_id(&r#macro.language(), NodeKind::Macro)
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

fn extract_params(text: &str, cursor: &mut TreeCursor, declarations: &mut Vec<ParameterDeclaration>) {
    let decl = cursor.node();
    debug_assert_eq!(
        decl.kind_id(),
        node_into_id(&decl.language(), NodeKind::ParameterDeclaration)
    );

    if !cursor.goto_first_child() {
        return;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&decl.language(), NodeKind::Identifier)
    );

    while cursor.goto_next_sibling() {
        let r#macro = cursor.node();

        debug_assert_eq!(
            r#macro.kind_id(),
            node_into_id(&r#macro.language(), NodeKind::Macro)
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

fn extract_subroutine_def(cursor: &mut TreeCursor) -> Option<Subroutine> {
    let def = cursor.node();
    let lang = def.language();

    debug_assert!(get_subroutine_ids(&lang).contains(&cursor.node().kind_id()));

    if !cursor.goto_first_child() {
        return None;
    }

    if def.kind_id() == node_into_id(&lang, NodeKind::SubroutineBlock) {
        if !cursor.goto_next_sibling() {
            return None;
        }
    }

    let id_identifier = node_into_id(&lang, NodeKind::Identifier);
    if cursor.node().kind_id() != id_identifier {
        cursor.goto_parent();
        return None;
    }
    let name = cursor.node();

    cursor.goto_parent();
    Some(Subroutine {
        name: name.byte_range(),
        definition: def.byte_range(),
        docstring: None,
    })
}

fn extract_subroutine_call(cursor: &mut TreeCursor) -> Option<CallExpression> {
    let call = cursor.node();

    debug_assert_eq!(
        call.kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );

    if !cursor.goto_next_sibling() {
        cursor.goto_parent();
        return None;
    }

    let target = cursor.node();
    debug_assert_eq!(
        target.kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );

    cursor.goto_parent();

    Some(CallExpression {
        target: target.byte_range(),
        call: call.byte_range(),
        docstring: None,
    })
}

fn extract_script_call(text: &str, cursor: &mut TreeCursor) -> Option<CallExpression> {
    let call = cursor.node();

    debug_assert_eq!(
        call.kind_id(),
        NodeKind::CommandExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );

    let command = text[cursor.node().byte_range()].split(".").last()?;
    if !(KEYWORDS_SCRIPT_CALL
        .iter()
        .any(|k| k.eq_ignore_ascii_case(command))
        && cursor.goto_next_sibling())
    {
        cursor.goto_parent();
        return None;
    }

    let mut target = cursor.node().byte_range();
    while cursor.goto_next_sibling() {
        target.end = cursor.node().end_byte();
    }
    cursor.goto_parent();

    Some(CallExpression {
        target,
        call: call.byte_range(),
        docstring: None,
    })
}

fn find_macro_scope(text: &str, cursor: &mut TreeCursor) -> Option<MacroScope> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );
    let scope = Some(MacroScope::from(&text[cursor.node().byte_range()]));

    cursor.goto_parent();
    scope
}

fn find_docstring(cursor: &mut TreeCursor) -> Option<Range<usize>> {
    let target = cursor.node();

    if !(cursor.goto_parent() && cursor.goto_first_child()) {
        unreachable!("Target node must have a parent.");
    }

    let id_comment = node_into_id(&target.language(), NodeKind::Comment);
    let mut node = cursor.node();

    let mut docstring: Option<TRange> = None;

    while node != target {
        let id = node.kind_id();

        if id == id_comment {
            if let Some(comment) = &mut docstring {
                if start_on_adjacent_lines(&node.range(), comment) {
                    comment.end_point = node.end_position();
                    comment.end_byte = node.end_byte();
                } else {
                    docstring = Some(node.range());
                }
            } else {
                docstring = Some(node.range());
            }
        } else if docstring.is_some() {
            docstring = None;
        }

        if !cursor.goto_next_sibling() {
            unreachable!("Target must be included in siblings.");
        }
        node = cursor.node();
    }

    match docstring {
        Some(range) if start_on_adjacent_lines(&range, &target.range()) => Some(Range {
            start: range.start_byte,
            end: range.end_byte,
        }),
        _ => None,
    }
}

fn resides_in_subroutine(
    subroutines: &Option<Vec<Subroutine>>,
    offset: usize,
) -> Option<&Subroutine> {
    let Some(sub) = subroutines else {
        return None;
    };

    sub.iter().find(|s| s.definition.contains(&offset))
}

fn goto_subroutine(tree: &Tree, offset: usize) -> TreeCursor {
    let mut cursor = tree.walk();
    let ids = get_subroutine_ids(&tree.language());
    loop {
        if cursor.goto_first_child_for_byte(offset).is_none() {
            break tree.walk();
        }
        if ids.contains(&cursor.node().kind_id()) {
            break cursor;
        }
    }
}

#[allow(dead_code)]
fn terminates_script(text: &str, cursor: &mut TreeCursor) -> bool {
    let node = cursor.node();
    if node.kind_id() != NodeKind::CommandExpression.into_id(&node.language()) {
        return false;
    }

    if !cursor.goto_first_child() {
        return false;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        node_into_id(&cursor.node().language(), NodeKind::Identifier)
    );

    let Some(command) = text[cursor.node().byte_range()].split(".").last() else {
        cursor.goto_parent();
        return false;
    };

    cursor.goto_parent();

    KEYWORDS_SCRIPT_END
        .iter()
        .any(|k| k.eq_ignore_ascii_case(command))
}

#[allow(dead_code)]
pub fn skip_comments(cursor: &mut TreeCursor) {
    let node = cursor.node();
    let lang = node.language();

    let id = node_into_id(&lang, NodeKind::Comment);

    while cursor.node().kind_id() == id {
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}
