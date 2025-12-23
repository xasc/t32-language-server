// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! # [Note] Macro Definitions On-Assignment
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
//!
//! # [Note] `GLOBAL` Macro Definitions
//!
//! Once defined, `GLOBAL` macros move to the bottom of the PRACTICE stack.
//! The scope at which their definition is placed does not matter. The only
//! factor is whether a `GLOBAL` macro is already defined or not.
//! Determining whether a `GLOBAL` macro is defined, is not possible by static
//! analysis alone. This is something that can only reliably be done at runtime.
//! To work around this problem we assume that any `GLOBAL` macro with matching
//! name is a possible candidate.
//!
//! # [Note] Subroutines Definitions from `(labeled_expression)`
//!
//! `(labeled_expression)` can either start a subroutine or act as a plain
//! label. We are using a number of criteria for differentiating one from the
//! other:
//!   1. Having a `(block)` after a label always starts a subroutine.
//!   2. `(labeled_expression)` without `(block)` only starts a subroutine, if
//!      it ends with `RETURN` and contains no `GOTO` statement.
//!   3. Block or command indentation do not matter.
//!
//!
//! # [Note] Parameter Passing
//!
//! The grammar is classifying `RETURNVALUES` as `(parameter_declaration)`.
//! Check out this example:
//!
//! ```ignore
//!   PRIVATE &a &b
//!
//!   GOSUB subA
//!   RETURNVALUES &a
//!
//!   GOSUB subB "Oh, wow! What superb language design!"
//!
//!   ENDDO
//!
//!
//!   subA:
//!   RETURN "TEST"
//!
//!   subB:
//!   PRIVATE &b
//!   RETURNVALUES &b
//!
//!   PRINT "&b"
//!   RETURN
//! ```
//!
//! # [Note] Block-Global vs. Block-Local Macro Definitions
//!
//! `GLOBAL` and `LOCAL` macros are block-global. They have local visibility in
//! the current block and global visibility in subroutines and subscripts.
//! Macros with `PRIVATE` lifetime are block-local. They only have local visibility
//! in the current block.
//!

use std::ops::Range;

use tree_sitter::{Node, Range as TRange, Tree, TreeCursor};

use crate::{
    protocol::Uri,
    t32::{
        FileIndex, GotoDefLangContext, MacroDefinitionResult, NodeKind,
        ast::{
            KEYWORD_GOTO, KEYWORD_SUBROUTINE_RETURN, KEYWORDS_SCRIPT_CALL, KEYWORDS_SCRIPT_END,
            get_block_opener_ids, get_string_body, get_subroutine_ids, start_on_adjacent_lines,
        },
        macros::{
            defines_any_macro, defines_block_global_macro, defines_global_macro_implicitly,
            extract_macro_defs, extract_params, find_macro_scope, may_define_macro_implicitly,
        },
        path::locate_script,
    },
    utils::BRange,
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
    /// Without external definition the macro is implicitly defined as `LOCAL`
    /// on first use (e.g by an assignment). Definition might be overridden by
    /// callers.
    Implicit(MacroDefinition),
    /// No macro definition in file scope found. Definition might be provided
    /// by caller.
    Indeterminate,
    /// No macro definition found. Search was aborted.
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MacroScope {
    Global,
    Local,
    Private,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub name: Range<usize>,
    pub expression: Range<usize>,
    pub docstring: Option<Range<usize>>,
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
    #[expect(unused)]
    pub cmd: Range<usize>,

    pub r#macro: Range<usize>,
    pub docstring: Option<Range<usize>>, // TODO: Detect inline docstring after expression.
}

/// TODO: Remove `Option` wrapper
#[derive(Clone, Debug)]
pub struct MacroDefinitions {
    pub privates: Option<Vec<MacroDefinition>>,
    pub locals: Option<Vec<MacroDefinition>>,
    pub globals: Option<Vec<MacroDefinition>>,
}

#[derive(Clone, Debug)]
pub struct CallExpressions {
    pub subroutines: Vec<CallExpression>,
    pub scripts: Option<SubscriptCalls>,
}

#[derive(Clone, Debug)]
pub struct CallLocations {
    pub subroutines: Vec<CallExpression>,
    pub scripts: Vec<CallExpression>,
}

#[derive(Clone, Debug)]
pub struct SubscriptCalls {
    pub locations: Vec<CallExpression>,
    pub targets: Vec<Option<Uri>>,
}

impl MacroDefinitions {
    pub fn build(
        privates: Vec<MacroDefinition>,
        locals: Vec<MacroDefinition>,
        globals: Vec<MacroDefinition>,
    ) -> Self {
        let privates: Option<Vec<MacroDefinition>> = match privates {
            loc if loc.len() <= 0 => None,
            loc => Some(loc),
        };

        let locals: Option<Vec<MacroDefinition>> = match locals {
            loc if loc.len() <= 0 => None,
            loc => Some(loc),
        };

        let globals: Option<Vec<MacroDefinition>> = match globals {
            g if g.len() <= 0 => None,
            g => Some(g),
        };
        MacroDefinitions {
            privates,
            locals,
            globals,
        }
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

pub fn find_call_target_definition(
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

pub fn find_subroutine<'a>(
    subroutines: &'a Vec<Subroutine>,
    cursor: &TreeCursor,
) -> Option<&'a Subroutine> {
    let node = cursor.node();

    debug_assert!(
        node.kind_id() == NodeKind::SubroutineBlock.into_id(&node.language())
            || node.kind_id() == NodeKind::LabeledExpression.into_id(&node.language())
    );

    for subroutine in subroutines {
        if subroutine.definition.contains(&node.start_byte()) {
            return Some(subroutine);
        }
    }
    None
}

pub fn find_label<'a>(labels: &'a Vec<Label>, cursor: &TreeCursor) -> Option<&'a Label> {
    let node = cursor.node();

    debug_assert!(
        node.kind_id() == NodeKind::SubroutineBlock.into_id(&node.language())
            || node.kind_id() == NodeKind::LabeledExpression.into_id(&node.language())
    );

    for label in labels {
        if label.expression.contains(&node.start_byte()) {
            return Some(label);
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

/// Finds all PRACTICE macros with `PRIVATE`, `LOCAL` and `GLOBAL` scope.
pub fn find_all_macro_definitions(text: &str, tree: &Tree) -> MacroDefinitions {
    let mut cursor = tree.walk();

    let lang = tree.language();
    let block_openers = get_block_opener_ids(&lang);

    let id_macro_def = NodeKind::MacroDefinition.into_id(&tree.language());

    let mut privates: Vec<MacroDefinition> = Vec::new();
    let mut locals: Vec<MacroDefinition> = Vec::new();
    let mut globals: Vec<MacroDefinition> = Vec::new();

    if !cursor.goto_first_child() {
        return MacroDefinitions::build(privates, locals, globals);
    }

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == id_macro_def {
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
    MacroDefinitions::build(privates, locals, globals)
}

pub fn find_all_subroutines_and_labels(text: &str, tree: &Tree) -> (Vec<Subroutine>, Vec<Label>) {
    let id_subroutines = get_subroutine_ids(&tree.language());

    let lang = tree.language();

    let labeled_expr = NodeKind::LabeledExpression.into_id(&lang);
    let subroutine_block = NodeKind::SubroutineBlock.into_id(&lang);

    let mut subroutines: Vec<Subroutine> = Vec::new();
    let mut labels: Vec<Label> = Vec::new();

    let mut cursor = tree.walk();
    if !cursor.goto_first_child() {
        return (subroutines, labels);
    }

    loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id_subroutines.contains(&id) {
            let subroutine = if id == labeled_expr {
                try_extract_subroutine_def_from_label(text, &mut cursor)
            } else {
                debug_assert_eq!(id, subroutine_block);
                extract_subroutine_def(&mut cursor)
            };
            debug_assert!(id_subroutines.contains(&cursor.node().kind_id()));

            if let Some(mut subroutine) = subroutine {
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    subroutine.docstring = docstring;
                }
                subroutines.push(subroutine);
            } else if let Some(mut label) = extract_label(&mut cursor) {
                let docstring = find_docstring(&mut cursor);
                if docstring.is_some() {
                    label.docstring = docstring;
                }
                labels.push(label);
            }
            debug_assert!(id_subroutines.contains(&cursor.node().kind_id()));
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
    (subroutines, labels)
}

pub fn find_all_call_expressions(text: &str, tree: &Tree) -> CallLocations {
    let mut cursor = tree.walk();

    let mut subroutines: Vec<CallExpression> = Vec::new();
    let mut scripts: Vec<CallExpression> = Vec::new();

    if !cursor.goto_first_child() {
        return CallLocations {
            subroutines,
            scripts,
        };
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
    CallLocations {
        subroutines,
        scripts,
    }
}

pub fn find_all_parameter_declarations(text: &str, tree: &Tree) -> Vec<ParameterDeclaration> {
    let mut cursor = tree.walk();

    let mut parameters: Vec<ParameterDeclaration> = Vec::new();

    if !cursor.goto_first_child() {
        return parameters;
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
    parameters
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

    let block_openers = get_block_opener_ids(&lang);
    while cursor.goto_first_child_for_byte(target).is_some() {
        let node = cursor.node();
        if !node.byte_range().contains(&target) {
            return None;
        }

        let id = node.kind_id();
        if id == cmd {
            break;
        } else if !block_openers.contains(&id) {
            return None;
        }
    }
    debug_assert_eq!(cursor.node().kind_id(), cmd);

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
    if let Some(macros) = &t32.macros.globals {
        for r#macro in macros.iter().filter(|m| text[m.r#macro.clone()] == *name) {
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

pub fn find_all_references_for_subroutine(
    text: &str,
    subroutine: &Subroutine,
    tree: &Tree,
) -> Vec<Range<usize>> {
    let mut refs: Vec<Range<usize>> = vec![subroutine.name.clone()];

    let name = &text[subroutine.name.clone()];
    debug_assert!(name.len() > 0);

    let mut cursor = tree.walk();
    if !cursor.goto_first_child() {
        return refs;
    }

    let node = cursor.node();
    let lang = node.language();

    let block_openers = get_block_opener_ids(&lang);
    let id_call = NodeKind::SubroutineCallExpression.into_id(&lang);

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == id_call {
            if let Some(r#ref) = matches_call_to_subroutine(text, name, &mut cursor) {
                refs.push(r#ref);
            }
            debug_assert_eq!(cursor.node().kind_id(), id_call);
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
    refs
}

pub fn find_all_references_for_label(text: &str, label: &Label, tree: &Tree) -> Vec<Range<usize>> {
    let mut refs: Vec<Range<usize>> = vec![label.name.clone()];

    let name = &text[label.name.clone()];
    debug_assert!(name.len() > 0);

    let mut cursor = tree.walk();

    let node = cursor.node();
    let lang = node.language();

    let block_openers = get_block_opener_ids(&lang);
    let command = NodeKind::CommandExpression.into_id(&lang);

    if !cursor.goto_first_child() {
        return refs;
    }

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == command {
            // TODO: Add support for `ON ERROR GOTO <label>`
            if let Some(target) = extract_goto_target(text, &mut cursor)
                && text[target.clone()] == *name
            {
                refs.push(target);
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
    refs
}

/// Find PRACTICE macro definitions in a parent block relative to the origin
/// node. Any explicit macro type (`PRIVATE`, `LOCAL`, `GLOBAL`) works. If no
/// explicit definition can be found, we locate the place where the macro is
/// used first.
fn find_explicit_or_implicit_macro_def(
    text: &str,
    globals: &Option<Vec<MacroDefinition>>,
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

fn find_explicit_or_implicit_macro_def_in_script(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<MacroDefResolution> {
    find_explicit_or_implicit_macro_def_in_block(text, origin, name, tree.walk(), defines_any_macro)
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

fn extract_subroutine_def(cursor: &mut TreeCursor) -> Option<Subroutine> {
    let def = cursor.node();
    let lang = def.language();

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::SubroutineBlock.into_id(&lang)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    if !cursor.goto_next_sibling() {
        cursor.goto_parent();
        return None;
    }

    let name = cursor.node();
    if name.kind_id() != NodeKind::Identifier.into_id(&lang) {
        cursor.goto_parent();
        return None;
    }

    cursor.goto_parent();
    Some(Subroutine {
        name: name.byte_range(),
        definition: def.byte_range(),
        docstring: None,
    })
}

/// Differentiate labels starting a subroutine from plain labels.
/// See [Note: Subroutines Definitions from `(labeled_expression)`]
/// for the selection criteria.
///
fn try_extract_subroutine_def_from_label(
    text: &str,
    cursor: &mut TreeCursor,
) -> Option<Subroutine> {
    let start = cursor.clone();

    let def = start.node();
    let lang = def.language();

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::LabeledExpression.into_id(&lang)
    );

    if !cursor.goto_first_child() {
        return None;
    }

    if cursor.node().kind_id() != NodeKind::Identifier.into_id(&lang) {
        cursor.goto_parent();
        return None;
    }
    let name = cursor.node();

    if !cursor.goto_next_sibling() {
        cursor.goto_parent();
        return None;
    }
    let command = NodeKind::CommandExpression.into_id(&lang);

    // Check for block or command after colon.
    if cursor.goto_next_sibling() {
        if cursor.node().kind_id() == NodeKind::Block.into_id(&lang) {
            cursor.goto_parent();
            return Some(Subroutine {
                name: name.byte_range(),
                definition: def.byte_range(),
                docstring: None,
            });
        } else {
            // Command is on the same line as label.
            let node = cursor.node();
            if node.kind_id() == command
                && cursor.goto_first_child()
                && node.kind_id() == NodeKind::Identifier.into_id(&lang)
                && text[node.byte_range()].eq_ignore_ascii_case(KEYWORD_SUBROUTINE_RETURN)
            {
                cursor.goto_parent();
                cursor.goto_parent();
                return Some(Subroutine {
                    name: name.byte_range(),
                    definition: def.byte_range(),
                    docstring: None,
                });
            }
        }
        cursor.goto_parent();
        return None;
    }

    // Find return from subroutine command after label.
    cursor.goto_parent();
    if !cursor.goto_next_sibling() {
        return None;
    }

    let block = NodeKind::Block.into_id(&lang);
    let labeled_expr = NodeKind::LabeledExpression.into_id(&lang);
    let subroutine = NodeKind::SubroutineBlock.into_id(&lang);

    let mut nest_level: i32 = 0;
    let mut ii = 0;
    'outer: loop {
        let node = cursor.node();
        let kind = node.kind_id();

        if kind == command {
            if cursor.goto_first_child() {
                let Some(cmd) = text[cursor.node().byte_range()].split(".").last() else {
                    unreachable!("Command must not be empty.");
                };

                if cmd.eq_ignore_ascii_case(KEYWORD_SUBROUTINE_RETURN) {
                    cursor.goto_parent();
                    let end = cursor.node().end_byte();

                    cursor.clone_from(&start);
                    return Some(Subroutine {
                        name: name.byte_range(),
                        definition: Range {
                            start: def.start_byte(),
                            end,
                        },
                        docstring: None,
                    });
                } else if cmd.eq_ignore_ascii_case(KEYWORD_GOTO)
                    || KEYWORDS_SCRIPT_END
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(cmd))
                {
                    break;
                }
                cursor.goto_parent();
            }
        } else if kind == labeled_expr || kind == subroutine {
            break;
        } else if kind == block {
            if cursor.goto_first_child() {
                nest_level += 1;
                continue;
            }
        }

        while !cursor.goto_next_sibling() {
            if nest_level < 0 || !cursor.goto_parent() {
                break 'outer;
            }
            nest_level -= 1;
        }

        // TODO: Remove loop breaker!?
        ii += 1;
        if ii > 10 {
            break;
        }
    }
    cursor.clone_from(&start);
    None
}

fn extract_label(cursor: &mut TreeCursor) -> Option<Label> {
    let expression = cursor.node();

    debug_assert_eq!(
        expression.kind_id(),
        NodeKind::LabeledExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    let name = cursor.node().byte_range();
    cursor.goto_parent();

    Some(Label {
        name,
        expression: expression.byte_range(),
        docstring: None,
    })
}

fn extract_subroutine_call(cursor: &mut TreeCursor) -> Option<CallExpression> {
    let call = cursor.node();

    debug_assert_eq!(
        call.kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&cursor.node().language())
    );

    let Some(target) = goto_subroutine_call_target(cursor) else {
        return None;
    };
    let span = target.byte_range();

    debug_assert_eq!(
        target.kind_id(),
        NodeKind::Identifier.into_id(&target.language())
    );
    cursor.goto_parent();

    Some(CallExpression {
        target: span,
        call: call.byte_range(),
        docstring: None,
    })
}

fn assign_lhs_matches_macro(text: &str, name: &str, cursor: &mut TreeCursor) -> Option<BRange> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::AssignmentExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        cursor.goto_parent();
        return None;
    }

    let lhs = cursor.node();
    cursor.goto_parent();

    let id = {
        let node = cursor.node();
        let lang = node.language();

        NodeKind::Macro.into_id(&lang)
    };

    let span = lhs.byte_range();

    if lhs.kind_id() == id && text[span.clone()] == *name {
        Some(BRange::from(span))
    } else {
        None
    }
}

fn matches_call_to_subroutine(
    text: &str,
    name: &str,
    cursor: &mut TreeCursor,
) -> Option<Range<usize>> {
    let call = cursor.node();

    debug_assert_eq!(
        call.kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&cursor.node().language())
    );

    let Some(target) = goto_subroutine_call_target(cursor) else {
        return None;
    };
    let span = target.byte_range();

    debug_assert_eq!(
        target.kind_id(),
        NodeKind::Identifier.into_id(&target.language())
    );

    cursor.goto_parent();

    if &text[span.clone()] != name {
        return None;
    }
    Some(span)
}

fn goto_subroutine_call_target<'a>(cursor: &'a mut TreeCursor) -> Option<Node<'a>> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::SubroutineCallExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    if !cursor.goto_next_sibling() {
        cursor.goto_parent();
        return None;
    }

    let target = cursor.node();
    debug_assert_eq!(
        target.kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );
    Some(target)
}

fn extract_script_call(text: &str, cursor: &mut TreeCursor) -> Option<CallExpression> {
    let call = cursor.node();

    debug_assert_eq!(
        call.kind_id(),
        NodeKind::CommandExpression.into_id(&call.language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
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

fn extract_goto_target(text: &str, cursor: &mut TreeCursor) -> Option<Range<usize>> {
    let goto = cursor.node();

    debug_assert_eq!(
        goto.kind_id(),
        NodeKind::CommandExpression.into_id(&goto.language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    let command = text[cursor.node().byte_range()].split(".").last()?;
    if !(command.eq_ignore_ascii_case(KEYWORD_GOTO) && cursor.goto_next_sibling()) {
        cursor.goto_parent();
        return None;
    }

    let mut target = cursor.node().byte_range();
    while cursor.goto_next_sibling() {
        target.end = cursor.node().end_byte();
    }
    cursor.goto_parent();

    Some(target)
}

fn find_docstring(cursor: &mut TreeCursor) -> Option<Range<usize>> {
    let target = cursor.node();

    if !(cursor.goto_parent() && cursor.goto_first_child()) {
        unreachable!("Target node must have a parent.");
    }

    let id_comment = NodeKind::Comment.into_id(&target.language());
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

fn resides_in_subroutine(subroutines: &Vec<Subroutine>, offset: usize) -> Option<&Subroutine> {
    subroutines.iter().find(|s| s.definition.contains(&offset))
}

fn goto_subroutine(tree: &Tree, offset: usize) -> TreeCursor<'_> {
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

#[expect(unused)]
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
        NodeKind::Identifier.into_id(&cursor.node().language())
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

#[expect(unused)]
pub fn skip_comments(cursor: &mut TreeCursor) {
    let node = cursor.node();
    let lang = node.language();

    let id = NodeKind::Comment.into_id(&lang);

    while cursor.node().kind_id() == id {
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}
