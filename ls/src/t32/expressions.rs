// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ops::Range;

use tree_sitter::{Range as TRange, Tree, TreeCursor};

use crate::t32::{
    NodeKind,
    ast::{get_scope_opener_ids, node_into_id, start_on_adjacent_lines},
};

#[derive(Debug)]
pub struct MacroDefinition {
    #[allow(dead_code)]
    pub scope: MacroScope,

    pub definition: Range<usize>,
    pub r#macro: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[derive(Debug)]
pub enum MacroScope {
    Global,
    Local,
    Private,
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
    r#macro: TreeCursor,
) -> Option<MacroDefinition> {
    let node = r#macro.node();
    debug_assert!(node.end_byte() < text.len());

    let name = &text[node.start_byte()..node.end_byte()];

    let mut cursor = tree.walk();

    if !cursor.goto_first_child() {
        return None;
    }
    let lang = tree.language();
    let _scope_openers = get_scope_opener_ids(&lang);

    let definition = find_macro_def_in_main_body(text, &node.byte_range(), name, tree);
    definition
}

pub fn find_macro_def_in_main_body(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<MacroDefinition> {
    let mut cursor = tree.walk();

    let id_macro_def = node_into_id(&tree.language(), NodeKind::MacroDefinition);

    let mut definition: Option<MacroDefinition> = None;

    loop {
        let node = cursor.node();
        if node.start_byte() > origin.end {
            break;
        }
        let id = node.kind_id();

        if id == id_macro_def {
            if let Some(mut def) = defines_macro(&text, &mut cursor, name) {
                let docstring = find_docstring(tree, &mut cursor);
                if docstring.is_some() {
                    def.docstring = docstring;
                }

                debug_assert_eq!(
                    cursor.node().kind_id(),
                    node_into_id(&cursor.node().language(), NodeKind::MacroDefinition)
                );
                definition = Some(def);
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

fn defines_macro(text: &str, def: &mut TreeCursor, name: &str) -> Option<MacroDefinition> {
    debug_assert_eq!(
        def.node().kind_id(),
        node_into_id(&def.node().language(), NodeKind::MacroDefinition)
    );

    if !def.goto_first_child() {
        return None;
    }

    debug_assert_eq!(
        def.node().kind_id(),
        node_into_id(&def.node().language(), NodeKind::Identifier)
    );
    let scope = MacroScope::from(&text[def.node().byte_range()]);

    while def.goto_next_sibling() {
        let r#macro = def.node();
        debug_assert_eq!(
            r#macro.kind_id(),
            node_into_id(&r#macro.language(), NodeKind::Macro)
        );

        let range = r#macro.byte_range();
        if range.end >= text.len() {
            break;
        }

        if &text[range] == name {
            def.goto_parent();
            return Some(MacroDefinition {
                scope,
                definition: def.node().byte_range(),
                r#macro: r#macro.byte_range(),
                docstring: None,
            });
        }
    }
    def.goto_parent();
    None
}

fn find_docstring(tree: &Tree, cursor: &mut TreeCursor) -> Option<Range<usize>> {
    let target = cursor.node();

    if !(cursor.goto_parent() && cursor.goto_first_child()) {
        unreachable!("Target node must have a parent.");
    }

    let id_comment = node_into_id(&tree.language(), NodeKind::Comment);
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
