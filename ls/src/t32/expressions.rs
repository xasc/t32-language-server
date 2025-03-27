// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ops::Range;

use tree_sitter::{Tree, TreeCursor};

use crate::t32::{
    NodeKind,
    ast::{get_scope_opener_ids, node_into_id},
};

#[derive(Debug)]
pub struct MacroDefinition {
    #[allow(dead_code)]
    pub scope: MacroScope,

    pub definition: Range<usize>,
    pub r#macro: Range<usize>,
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

pub fn find_macro_definition<'a>(
    text: &str,
    tree: &'a Tree,
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
            if let Some(def) = defines_macro(&text, &mut cursor, name) {
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
            });
        }
    }
    def.goto_parent();
    None
}
