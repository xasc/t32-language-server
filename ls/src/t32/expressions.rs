// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ops::Range;

use tree_sitter::{Range as TRange, Tree, TreeCursor};

use crate::t32::{
    NodeKind,
    ast::{get_block_opener_ids, get_subroutine_ids, node_into_id, start_on_adjacent_lines},
};

#[derive(Clone, Debug)]
pub struct MacroDefinition {
    #[allow(dead_code)]
    pub scope: MacroScope,

    pub definition: Range<usize>,
    pub r#macro: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Subroutine {
    pub name: Range<usize>,
    pub definition: Range<usize>,
    pub docstring: Option<Range<usize>>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct MacroDefinitions {
    pub locals: Option<Vec<Range<usize>>>,
    pub globals: Option<Vec<MacroDefinition>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MacroScope {
    Global,
    Local,
    Private,
}

impl MacroDefinitions {
    pub fn build(locals: Vec<Range<usize>>, globals: Vec<MacroDefinition>) -> Self {
        let locals: Option<Vec<Range<usize>>> = match locals {
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

    let definition = find_macro_def_in_main_body(text, &node.byte_range(), name, tree);
    definition
}

pub fn find_all_global_macro_definitions(text: &str, tree: &Tree) -> MacroDefinitions {
    let mut cursor = tree.walk();

    let lang = tree.language();
    let block_openers = get_block_opener_ids(&lang);

    let id_macro_def = node_into_id(&tree.language(), NodeKind::MacroDefinition);

    let mut locals: Vec<Range<usize>> = Vec::new();
    let mut globals: Vec<MacroDefinition> = Vec::new();

    if !cursor.goto_first_child() {
        return MacroDefinitions::build(locals, globals);
    }

    'outer: loop {
        let node = cursor.node();
        let id = node.kind_id();

        if id == id_macro_def {
            if let Some(scope) = find_macro_scope(text, &mut cursor) {
                if scope == MacroScope::Local {
                    extract_local_macro_defs(text, &mut cursor, &mut locals);
                } else if scope == MacroScope::Global {
                    let num = globals.len();
                    extract_global_macro_defs(text, &mut cursor, &mut globals);
                    if globals.len() != num {
                        let docstring = find_docstring(tree, &mut cursor);
                        if docstring.is_some() {
                            for def in globals[num..].iter_mut() {
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
                let docstring = find_docstring(tree, &mut cursor);
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

pub fn find_macro_def_in_main_body(
    text: &str,
    origin: &Range<usize>,
    name: &str,
    tree: &Tree,
) -> Option<MacroDefinition> {
    let id_macro_def = node_into_id(&tree.language(), NodeKind::MacroDefinition);

    let mut definition: Option<MacroDefinition> = None;

    let mut cursor = tree.walk();
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

fn extract_local_macro_defs(text: &str, def: &mut TreeCursor, locals: &mut Vec<Range<usize>>) {
    debug_assert_eq!(
        def.node().kind_id(),
        node_into_id(&def.node().language(), NodeKind::MacroDefinition)
    );

    if !def.goto_first_child() {
        return;
    }

    debug_assert_eq!(
        def.node().kind_id(),
        node_into_id(&def.node().language(), NodeKind::Identifier)
    );
    debug_assert_eq!(
        MacroScope::from(&text[def.node().byte_range()]),
        MacroScope::Local
    );

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
        locals.push(range);
    }
    def.goto_parent();
}

fn extract_global_macro_defs(
    text: &str,
    cursor: &mut TreeCursor,
    globals: &mut Vec<MacroDefinition>,
) {
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
    debug_assert_eq!(
        MacroScope::from(&text[cursor.node().byte_range()]),
        MacroScope::Global
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
        globals.push(MacroDefinition {
            scope: MacroScope::Global,
            definition: def.byte_range(),
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

fn find_macro_scope(text: &str, def: &mut TreeCursor) -> Option<MacroScope> {
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
    let scope = Some(MacroScope::from(&text[def.node().byte_range()]));

    def.goto_parent();
    scope
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
