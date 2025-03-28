// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use tree_sitter::{Language, Range};

#[derive(Debug, Clone, Copy)]
pub enum NodeKind {
    Unknown,
    Block,
    CommandExpression,
    Comment,
    Identifier,
    IfBlock,
    LabeledExpression,
    Macro,
    MacroDefinition,
    RepeatBlock,
    Script,
    SubroutineBlock,
    SubroutineCallExpression,
    WhileBlock,
}

pub const GOTO_REF_SOURCES: [NodeKind; 3] = [
    NodeKind::CommandExpression,
    NodeKind::Macro,
    NodeKind::SubroutineCallExpression,
];

const SCOPE_OPENERS: [NodeKind; 6] = [
    NodeKind::Block,
    NodeKind::IfBlock,
    NodeKind::WhileBlock,
    NodeKind::SubroutineBlock,
    NodeKind::LabeledExpression,
    NodeKind::RepeatBlock,
];

pub const NODE_BLOCK: &'static str = "block";
pub const NODE_COMMENT: &'static str = "comment";
pub const NODE_COMMAND_EXPRESSION: &'static str = "command_expression";
pub const NODE_IDENTIFIER: &'static str = "identifier";
pub const NODE_IF_BLOCK: &'static str = "if_block";
pub const NODE_MACRO: &'static str = "macro";
pub const NODE_MACRO_DEFINITION: &'static str = "macro_definition";
pub const NODE_LABELED_EXPRESSION: &'static str = "labeled_expression";
pub const NODE_REPEAT_BLOCK: &'static str = "repeat_block";
pub const NODE_SCRIPT: &'static str = "script";
pub const NODE_SUBROUTINE_BLOCK: &'static str = "subroutine_block";
pub const NODE_SUBROUTINE_CALL_EXPRESSION: &'static str = "subroutine_call_expression";
pub const NODE_WHILE_BLOCK: &'static str = "while_block";

pub fn node_into_id(lang: &Language, node: NodeKind) -> u16 {
    lang.id_for_node_kind(
        match node {
            NodeKind::Block => NODE_BLOCK,
            NodeKind::Identifier => NODE_IDENTIFIER,
            NodeKind::IfBlock => NODE_IF_BLOCK,
            NodeKind::LabeledExpression => NODE_LABELED_EXPRESSION,
            NodeKind::Macro => NODE_MACRO,
            NodeKind::MacroDefinition => NODE_MACRO_DEFINITION,
            NodeKind::CommandExpression => NODE_COMMAND_EXPRESSION,
            NodeKind::Comment => NODE_COMMENT,
            NodeKind::RepeatBlock => NODE_REPEAT_BLOCK,
            NodeKind::Script => NODE_SCRIPT,
            NodeKind::SubroutineBlock => NODE_SUBROUTINE_BLOCK,
            NodeKind::SubroutineCallExpression => NODE_SUBROUTINE_CALL_EXPRESSION,
            NodeKind::WhileBlock => NODE_WHILE_BLOCK,
            NodeKind::Unknown => unimplemented!("Node has no ID."),
        },
        true,
    )
}

pub fn id_into_node(lang: &Language, id: u16) -> NodeKind {
    match lang.node_kind_for_id(id) {
        Some(name) => match name {
            NODE_BLOCK => NodeKind::Block,
            NODE_COMMAND_EXPRESSION => NodeKind::CommandExpression,
            NODE_COMMENT => NodeKind::Comment,
            NODE_IDENTIFIER => NodeKind::Identifier,
            NODE_IF_BLOCK => NodeKind::IfBlock,
            NODE_LABELED_EXPRESSION => NodeKind::LabeledExpression,
            NODE_MACRO => NodeKind::Macro,
            NODE_MACRO_DEFINITION => NodeKind::MacroDefinition,
            NODE_REPEAT_BLOCK => NodeKind::RepeatBlock,
            NODE_SCRIPT => NodeKind::Script,
            NODE_SUBROUTINE_BLOCK => NodeKind::SubroutineBlock,
            NODE_SUBROUTINE_CALL_EXPRESSION => NodeKind::SubroutineCallExpression,
            NODE_WHILE_BLOCK => NodeKind::WhileBlock,
            _ => NodeKind::Unknown,
        },
        None => NodeKind::Unknown,
    }
}

#[allow(dead_code)]
pub fn name_into_node(name: &str) -> NodeKind {
    match name {
        NODE_BLOCK => NodeKind::Block,
        NODE_COMMAND_EXPRESSION => NodeKind::CommandExpression,
        NODE_COMMENT => NodeKind::Comment,
        NODE_IF_BLOCK => NodeKind::IfBlock,
        NODE_MACRO => NodeKind::Macro,
        NODE_MACRO_DEFINITION => NodeKind::MacroDefinition,
        NODE_REPEAT_BLOCK => NodeKind::RepeatBlock,
        NODE_SCRIPT => NodeKind::Script,
        NODE_SUBROUTINE_BLOCK => NodeKind::SubroutineBlock,
        NODE_SUBROUTINE_CALL_EXPRESSION => NodeKind::SubroutineCallExpression,
        NODE_WHILE_BLOCK => NodeKind::WhileBlock,
        _ => NodeKind::Unknown,
    }
}

pub fn get_scope_opener_ids(lang: &Language) -> [u16; 6] {
    let mut ids = [0u16; 6];
    for (ii, &node) in SCOPE_OPENERS.iter().enumerate() {
        ids[ii] = node_into_id(&lang, node);
    }
    ids
}

pub fn start_on_adjacent_lines(a: &Range, b: &Range) -> bool {
    if a.start_point.row < b.start_point.row {
        a.end_point.row == b.start_point.row
    } else {
        b.end_point.row == a.start_point.row
    }
}
