// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Subroutines Definitions from `(labeled_expression)`
//! ==========================================================
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
//! [Note] Parameter Passing
//! ========================
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

use std::{cmp::Ordering, ops::Range};

use tree_sitter::{Node, Range as TRange, Tree, TreeCursor};

use crate::{
    protocol::Uri,
    t32::{
        FileIndex, NodeKind,
        ast::{
            KEYWORD_DO, KEYWORD_GOTO, KEYWORD_RUN, KEYWORD_SUBROUTINE_RETURN, KEYWORDS_SCRIPT_CALL,
            KEYWORDS_SCRIPT_END, get_block_opener_ids, get_string_body, get_subroutine_ids,
            start_on_adjacent_lines,
        },
        macros::extract_params,
        path::locate_script,
    },
    utils::BRange,
};

pub const RECURSION_BREAKER_SUBROUTINE_SCAN: usize = 50;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParameterDeclarationKind {
    Parameters,
    Entry,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SubscriptCallKind {
    Do,
    Run,
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
pub struct CallExpressions {
    pub subroutines: Vec<CallExpression>,
    pub scripts: Option<SubscriptCalls>,
}

#[derive(Clone, Debug)]
pub struct CallLocations {
    pub subroutines: Vec<CallExpression>,
    pub scripts: Vec<(CallExpression, SubscriptCallKind)>,
}

#[derive(Clone, Debug)]
pub struct Command {
    pub command: BRange,
    pub identifier: BRange,
    pub docstring: Option<BRange>,
}

#[derive(Clone, Debug)]
pub struct ParameterDeclaration {
    pub cmd: Range<usize>,
    pub r#macro: Range<usize>,
    pub kind: ParameterDeclarationKind,
    pub docstring: Option<Range<usize>>, // TODO: Detect inline docstring after expression.
}

#[derive(Clone, Debug)]
pub struct SubscriptCalls {
    pub locations: Vec<CallExpression>,
    pub targets: Vec<Option<Uri>>,
    pub kinds: Vec<SubscriptCallKind>,
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

impl Eq for MacroDefinition {}

impl Ord for MacroDefinition {
    fn cmp(&self, other: &Self) -> Ordering {
        let Self { r#macro: this, .. } = self;
        let Self { r#macro: other, .. } = other;

        if this.start > other.start || this.end > other.end {
            Ordering::Greater
        } else if this.start < other.start || this.end < other.end {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    }
}

impl PartialOrd for MacroDefinition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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
    let mut scripts: Vec<(CallExpression, SubscriptCallKind)> = Vec::new();

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
                    call.0.docstring = docstring;
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

pub fn find_all_commands_and_parameter_declarations(
    text: &str,
    tree: &Tree,
) -> (Vec<Command>, Vec<ParameterDeclaration>) {
    let mut cursor = tree.walk();

    let mut parameters: Vec<ParameterDeclaration> = Vec::new();
    let mut commands: Vec<Command> = Vec::new();

    if !cursor.goto_first_child() {
        return (commands, parameters);
    }

    let lang = tree.language();

    let declaration = NodeKind::ParameterDeclaration.into_id(&lang);
    let command = NodeKind::CommandExpression.into_id(&lang);
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
        } else if id == command {
            let num = commands.len();
            extract_command(&mut cursor, &mut commands);
            if commands.len() != num {
                let docstring = find_docstring(&mut cursor);
                if let Some(docstr) = docstring {
                    let len = commands.len();
                    commands[len - 1].docstring = Some(BRange::from(docstr));
                }
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
    (commands, parameters)
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

    if cursor.goto_first_child_for_byte(target).is_none() {
        return None;
    }
    let path = extract_script_call_command_arguments(text, &mut cursor)?;
    locate_script(path, &files)
}

pub fn locate_subscript_call_target<'a>(text: &'a str, cursor: &mut TreeCursor) -> Option<&'a str> {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::CommandExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    let node = cursor.node();
    let command = text[node.byte_range()].split(".").last()?;

    if !(KEYWORDS_SCRIPT_CALL
        .iter()
        .any(|k| k.eq_ignore_ascii_case(command))
        && cursor.goto_next_sibling())
    {
        return None;
    }
    extract_script_call_command_arguments(text, cursor)
}

fn extract_command(cursor: &mut TreeCursor, commands: &mut Vec<Command>) {
    let command = cursor.node();
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::CommandExpression.into_id(&cursor.node().language())
    );

    if !cursor.goto_first_child() {
        return;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );

    commands.push(Command {
        command: BRange::from(command.byte_range()),
        identifier: BRange::from(cursor.node().byte_range()),
        docstring: None,
    });

    cursor.goto_parent();

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::CommandExpression.into_id(&cursor.node().language())
    );
}

pub fn resides_in_subroutine(subroutines: &Vec<Subroutine>, offset: usize) -> Option<&Subroutine> {
    subroutines.iter().find(|s| s.definition.contains(&offset))
}

pub fn assign_lhs_matches_macro(text: &str, name: &str, cursor: &mut TreeCursor) -> Option<BRange> {
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

pub fn goto_subroutine(tree: &Tree, offset: usize) -> TreeCursor<'_> {
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

pub fn find_docstring(cursor: &mut TreeCursor) -> Option<Range<usize>> {
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

pub fn extract_assign_lhs_macro(cursor: &mut TreeCursor) -> Option<BRange> {
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

    if lhs.kind_id() == id {
        Some(BRange::from(lhs.byte_range()))
    } else {
        None
    }
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

fn extract_script_call_command_arguments<'a>(
    text: &'a str,
    cursor: &mut TreeCursor,
) -> Option<&'a str> {
    let node = cursor.node();
    let lang = node.language();

    let args = NodeKind::ArgumentList.into_id(&lang);
    if cursor.node().kind_id() != args {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::ArgumentList.into_id(&cursor.node().language())
    );

    let entry = cursor.clone();

    let (path, string): (u16, u16) = {
        let id_path = NodeKind::Path.into_id(&lang);
        let id_string = NodeKind::String.into_id(&lang);

        (id_path, id_string)
    };

    if !cursor.goto_first_child() {
        return None;
    }

    let path: &str = loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == path {
            break &text[node.byte_range()];
        } else if id == string {
            break &get_string_body(&node, &text);
        }

        if !cursor.goto_next_sibling() {
            return None;
        }
    };
    cursor.reset_to(&entry);

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::ArgumentList.into_id(&cursor.node().language())
    );
    Some(path)
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

pub fn extract_script_call(
    text: &str,
    cursor: &mut TreeCursor,
) -> Option<(CallExpression, SubscriptCallKind)> {
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
    let kind = extract_subscript_call_kind(command);

    if !(kind.is_some() && cursor.goto_next_sibling()) {
        cursor.goto_parent();
        return None;
    }

    let mut target = cursor.node().byte_range();
    while cursor.goto_next_sibling() {
        target.end = cursor.node().end_byte();
    }
    cursor.goto_parent();

    Some((
        CallExpression {
            target,
            call: call.byte_range(),
            docstring: None,
        },
        kind.expect("Type must be retrieved."),
    ))
}

pub fn find_command_identifier(text: &str, command: TreeCursor) -> Option<String> {
    debug_assert_eq!(
        command.node().kind_id(),
        NodeKind::CommandExpression.into_id(&command.node().language())
    );

    let mut cursor = command;

    if !cursor.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::Identifier.into_id(&cursor.node().language())
    );
    Some(text[cursor.node().byte_range()].to_string())
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

fn extract_subscript_call_kind(command: &str) -> Option<SubscriptCallKind> {
    if command.eq_ignore_ascii_case(KEYWORD_DO) {
        Some(SubscriptCallKind::Do)
    } else if command.eq_ignore_ascii_case(KEYWORD_RUN) {
        Some(SubscriptCallKind::Run)
    } else {
        None
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
