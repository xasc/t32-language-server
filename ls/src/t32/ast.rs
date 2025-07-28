// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use tree_sitter::{Language, Node, Range};

#[derive(Debug, Clone, Copy)]
pub enum NodeKind {
    ArgumentList,
    Block,
    CommandExpression,
    Comment,
    Identifier,
    IfBlock,
    LabeledExpression, // TODO: Differentiate plain labels from subroutines.
    Macro,
    MacroDefinition,
    Path,
    ParameterDeclaration,
    RepeatBlock,
    Script,
    String,
    SubroutineBlock,
    SubroutineCallExpression,
    Unknown,
    WhileBlock,
}

pub const GOTO_REF_SOURCES: [NodeKind; 3] = [
    NodeKind::CommandExpression,
    NodeKind::Macro,
    NodeKind::SubroutineCallExpression,
];

const BLOCK_OPENERS: [NodeKind; 6] = [
    NodeKind::Block,
    NodeKind::IfBlock,
    NodeKind::WhileBlock,
    NodeKind::SubroutineBlock,
    NodeKind::LabeledExpression,
    NodeKind::RepeatBlock,
];

pub const KEYWORD_SUBROUTINE_ENTRY: &'static str = "ENTRY";
pub const KEYWORD_SUBROUTINE_PARAMETERS: &'static str = "PARAMETERS";
pub const KEYWORDS_SCRIPT_CALL: [&'static str; 2] = ["DO", "RUN"];
pub const KEYWORDS_SCRIPT_END: [&'static str; 2] = ["END", "ENDDO"];

pub const NODE_ARGUMENT_LIST: &'static str = "argument_list";
pub const NODE_BLOCK: &'static str = "block";
pub const NODE_COMMENT: &'static str = "comment";
pub const NODE_COMMAND_EXPRESSION: &'static str = "command_expression";
pub const NODE_IDENTIFIER: &'static str = "identifier";
pub const NODE_IF_BLOCK: &'static str = "if_block";
pub const NODE_MACRO: &'static str = "macro";
pub const NODE_MACRO_DEFINITION: &'static str = "macro_definition";
pub const NODE_LABELED_EXPRESSION: &'static str = "labeled_expression";
pub const NODE_PARAMETER_DECLARATION: &'static str = "parameter_declaration";
pub const NODE_PATH: &'static str = "path";
pub const NODE_REPEAT_BLOCK: &'static str = "repeat_block";
pub const NODE_SCRIPT: &'static str = "script";
pub const NODE_STRING: &'static str = "string";
pub const NODE_SUBROUTINE_BLOCK: &'static str = "subroutine_block";
pub const NODE_SUBROUTINE_CALL_EXPRESSION: &'static str = "subroutine_call_expression";
pub const NODE_WHILE_BLOCK: &'static str = "while_block";

const SUBROUTINES: [NodeKind; 2] = [NodeKind::LabeledExpression, NodeKind::SubroutineBlock];

impl NodeKind {
    pub fn into_id(self, lang: &Language) -> u16 {
        node_into_id(lang, self)
    }
}

pub fn node_into_id(lang: &Language, node: NodeKind) -> u16 {
    lang.id_for_node_kind(
        match node {
            NodeKind::ArgumentList => NODE_ARGUMENT_LIST,
            NodeKind::Block => NODE_BLOCK,
            NodeKind::CommandExpression => NODE_COMMAND_EXPRESSION,
            NodeKind::Comment => NODE_COMMENT,
            NodeKind::Identifier => NODE_IDENTIFIER,
            NodeKind::IfBlock => NODE_IF_BLOCK,
            NodeKind::LabeledExpression => NODE_LABELED_EXPRESSION,
            NodeKind::Macro => NODE_MACRO,
            NodeKind::MacroDefinition => NODE_MACRO_DEFINITION,
            NodeKind::Path => NODE_PATH,
            NodeKind::ParameterDeclaration => NODE_PARAMETER_DECLARATION,
            NodeKind::RepeatBlock => NODE_REPEAT_BLOCK,
            NodeKind::Script => NODE_SCRIPT,
            NodeKind::String => NODE_STRING,
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
            NODE_ARGUMENT_LIST => NodeKind::ArgumentList,
            NODE_BLOCK => NodeKind::Block,
            NODE_COMMAND_EXPRESSION => NodeKind::CommandExpression,
            NODE_COMMENT => NodeKind::Comment,
            NODE_IDENTIFIER => NodeKind::Identifier,
            NODE_IF_BLOCK => NodeKind::IfBlock,
            NODE_LABELED_EXPRESSION => NodeKind::LabeledExpression,
            NODE_MACRO => NodeKind::Macro,
            NODE_MACRO_DEFINITION => NodeKind::MacroDefinition,
            NODE_PATH => NodeKind::Path,
            NODE_PARAMETER_DECLARATION => NodeKind::ParameterDeclaration,
            NODE_REPEAT_BLOCK => NodeKind::RepeatBlock,
            NODE_SCRIPT => NodeKind::Script,
            NODE_STRING => NodeKind::String,
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
        NODE_ARGUMENT_LIST => NodeKind::ArgumentList,
        NODE_BLOCK => NodeKind::Block,
        NODE_COMMAND_EXPRESSION => NodeKind::CommandExpression,
        NODE_COMMENT => NodeKind::Comment,
        NODE_IDENTIFIER => NodeKind::Identifier,
        NODE_IF_BLOCK => NodeKind::IfBlock,
        NODE_MACRO => NodeKind::Macro,
        NODE_MACRO_DEFINITION => NodeKind::MacroDefinition,
        NODE_PATH => NodeKind::Path,
        NODE_REPEAT_BLOCK => NodeKind::RepeatBlock,
        NODE_SCRIPT => NodeKind::Script,
        NODE_STRING => NodeKind::String,
        NODE_SUBROUTINE_BLOCK => NodeKind::SubroutineBlock,
        NODE_SUBROUTINE_CALL_EXPRESSION => NodeKind::SubroutineCallExpression,
        NODE_WHILE_BLOCK => NodeKind::WhileBlock,
        _ => NodeKind::Unknown,
    }
}

pub fn get_block_opener_ids(lang: &Language) -> [u16; 6] {
    let mut ids = [0u16; 6];
    for (ii, &node) in BLOCK_OPENERS.iter().enumerate() {
        ids[ii] = node_into_id(&lang, node);
    }
    ids
}

pub fn get_subroutine_ids(lang: &Language) -> [u16; 2] {
    let mut ids = [0u16; 2];
    for (ii, &node) in SUBROUTINES.iter().enumerate() {
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

pub fn get_string_body<'a>(node: &Node, text: &'a str) -> &'a str {
    let range = node.byte_range();

    &text[(range.start + 1)..(range.end - 1)]
}
