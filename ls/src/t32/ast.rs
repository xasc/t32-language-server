// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use tree_sitter::{Language, Node, Range};

#[derive(Debug, Clone, Copy)]
pub enum NodeKind {
    AccessClass,
    ArgumentList,
    AssignmentExpression,
    BinaryExpression,
    Block,
    CallExpression,
    CommandExpression,
    Comment,
    ElifBlock,
    ElseBlock,
    FormatExpression,
    HllTypeDescriptor,
    HllTypeIdentifier,
    Identifier,
    IfBlock,
    LabeledExpression, // TODO: Differentiate plain labels from subroutines.
    Macro,
    MacroDefinition,
    OptionExpression,
    Path,
    ParameterDeclaration,
    RecursiveMacroExpansion,
    RepeatBlock,
    Script,
    String,
    SubroutineBlock,
    SubroutineCallExpression,
    UnaryExpression,
    Unknown,
    WhileBlock,
}

const SUBROUTINES: [NodeKind; 2] = [NodeKind::LabeledExpression, NodeKind::SubroutineBlock];

const BLOCK_OPENERS: [NodeKind; 8] = [
    NodeKind::Block,
    NodeKind::ElseBlock,
    NodeKind::ElifBlock,
    NodeKind::IfBlock,
    NodeKind::LabeledExpression,
    NodeKind::RepeatBlock,
    NodeKind::SubroutineBlock,
    NodeKind::WhileBlock,
];

const CONTROL_FLOW_BLOCKS: [NodeKind; 5] = [
    NodeKind::ElifBlock,
    NodeKind::ElseBlock,
    NodeKind::IfBlock,
    NodeKind::WhileBlock,
    NodeKind::RepeatBlock,
];

const MACRO_CONTAINER_EXPRESSIONS: [NodeKind; 10] = [
    NodeKind::CommandExpression,
    NodeKind::AssignmentExpression,
    NodeKind::BinaryExpression,
    NodeKind::ArgumentList,
    NodeKind::String,
    NodeKind::CallExpression,
    NodeKind::OptionExpression,
    NodeKind::UnaryExpression,
    NodeKind::FormatExpression,
    NodeKind::RecursiveMacroExpansion,
];

pub const FIND_REF_SOURCES: [NodeKind; 5] = [
    NodeKind::CommandExpression,
    NodeKind::LabeledExpression,
    NodeKind::Macro,
    NodeKind::SubroutineBlock,
    NodeKind::SubroutineCallExpression,
];

pub const GOTO_DEF_SOURCES: [NodeKind; 3] = [
    NodeKind::CommandExpression,
    NodeKind::Macro,
    NodeKind::SubroutineCallExpression,
];

pub const KEYWORD_DO: &'static str = "DO";
pub const KEYWORD_GOTO: &'static str = "GOTO";
pub const KEYWORD_RUN: &'static str = "RUN";
pub const KEYWORD_SUBROUTINE_ENTRY: &'static str = "ENTRY";
pub const KEYWORD_SUBROUTINE_PARAMETERS: &'static str = "PARAMETERS";
pub const KEYWORD_SUBROUTINE_RETURN: &'static str = "RETURN";
pub const KEYWORDS_SCRIPT_CALL: [&'static str; 2] = ["DO", "RUN"];
pub const KEYWORDS_SCRIPT_END: [&'static str; 2] = ["END", "ENDDO"];

pub const NODE_ACCESS_CLASS: &'static str = "access_class";
pub const NODE_ARGUMENT_LIST: &'static str = "argument_list";
pub const NODE_ASSIGNMENT_EXPRESSION: &'static str = "assignment_expression";
pub const NODE_BINARY_EXPRESSION: &'static str = "binary_expression";
pub const NODE_BLOCK: &'static str = "block";
pub const NODE_CALL_EXPRESSION: &'static str = "call_expression";
pub const NODE_COMMENT: &'static str = "comment";
pub const NODE_COMMAND_EXPRESSION: &'static str = "command_expression";
pub const NODE_ELIF_BLOCK: &'static str = "elif_block";
pub const NODE_ELSE_BLOCK: &'static str = "else_block";
pub const NODE_FORMAT_EXPRESSION: &'static str = "format_expression";
pub const NODE_HLL_TYPE_IDENTIFIER: &'static str = "hll_type_identifier";
pub const NODE_HLL_TYPE_DESCRIPTOR: &'static str = "hll_type_descriptor";
pub const NODE_IDENTIFIER: &'static str = "identifier";
pub const NODE_IF_BLOCK: &'static str = "if_block";
pub const NODE_MACRO: &'static str = "macro";
pub const NODE_MACRO_DEFINITION: &'static str = "macro_definition";
pub const NODE_LABELED_EXPRESSION: &'static str = "labeled_expression";
pub const NODE_OPTION_EXPRESSION: &'static str = "option_expression";
pub const NODE_PARAMETER_DECLARATION: &'static str = "parameter_declaration";
pub const NODE_PATH: &'static str = "path";
pub const NODE_RECURSIVE_MACRO_EXPANSION: &'static str = "recursive_macro_expansion";
pub const NODE_REPEAT_BLOCK: &'static str = "repeat_block";
pub const NODE_SCRIPT: &'static str = "script";
pub const NODE_STRING: &'static str = "string";
pub const NODE_SUBROUTINE_BLOCK: &'static str = "subroutine_block";
pub const NODE_SUBROUTINE_CALL_EXPRESSION: &'static str = "subroutine_call_expression";
pub const NODE_UNARY_EXPRESSION: &'static str = "unary_expression";
pub const NODE_WHILE_BLOCK: &'static str = "while_block";

impl NodeKind {
    pub fn into_id(self, lang: &Language) -> u16 {
        node_into_id(lang, self)
    }
}

fn node_into_id(lang: &Language, node: NodeKind) -> u16 {
    lang.id_for_node_kind(
        match node {
            NodeKind::AccessClass => NODE_ACCESS_CLASS,
            NodeKind::ArgumentList => NODE_ARGUMENT_LIST,
            NodeKind::AssignmentExpression => NODE_ASSIGNMENT_EXPRESSION,
            NodeKind::BinaryExpression => NODE_BINARY_EXPRESSION,
            NodeKind::Block => NODE_BLOCK,
            NodeKind::CallExpression => NODE_CALL_EXPRESSION,
            NodeKind::CommandExpression => NODE_COMMAND_EXPRESSION,
            NodeKind::Comment => NODE_COMMENT,
            NodeKind::ElifBlock => NODE_ELIF_BLOCK,
            NodeKind::ElseBlock => NODE_ELSE_BLOCK,
            NodeKind::FormatExpression => NODE_FORMAT_EXPRESSION,
            NodeKind::HllTypeDescriptor => NODE_HLL_TYPE_DESCRIPTOR,
            NodeKind::HllTypeIdentifier => NODE_HLL_TYPE_IDENTIFIER,
            NodeKind::Identifier => NODE_IDENTIFIER,
            NodeKind::IfBlock => NODE_IF_BLOCK,
            NodeKind::LabeledExpression => NODE_LABELED_EXPRESSION,
            NodeKind::Macro => NODE_MACRO,
            NodeKind::MacroDefinition => NODE_MACRO_DEFINITION,
            NodeKind::OptionExpression => NODE_OPTION_EXPRESSION,
            NodeKind::Path => NODE_PATH,
            NodeKind::ParameterDeclaration => NODE_PARAMETER_DECLARATION,
            NodeKind::RecursiveMacroExpansion => NODE_RECURSIVE_MACRO_EXPANSION,
            NodeKind::RepeatBlock => NODE_REPEAT_BLOCK,
            NodeKind::Script => NODE_SCRIPT,
            NodeKind::String => NODE_STRING,
            NodeKind::SubroutineBlock => NODE_SUBROUTINE_BLOCK,
            NodeKind::SubroutineCallExpression => NODE_SUBROUTINE_CALL_EXPRESSION,
            NodeKind::UnaryExpression => NODE_UNARY_EXPRESSION,
            NodeKind::WhileBlock => NODE_WHILE_BLOCK,
            NodeKind::Unknown => unimplemented!("Node has no ID."),
        },
        true,
    )
}

pub fn id_into_node(lang: &Language, id: u16) -> NodeKind {
    match lang.node_kind_for_id(id) {
        Some(name) => match name {
            NODE_ACCESS_CLASS => NodeKind::AccessClass,
            NODE_ARGUMENT_LIST => NodeKind::ArgumentList,
            NODE_ASSIGNMENT_EXPRESSION => NodeKind::AssignmentExpression,
            NODE_BINARY_EXPRESSION => NodeKind::BinaryExpression,
            NODE_BLOCK => NodeKind::Block,
            NODE_CALL_EXPRESSION => NodeKind::CallExpression,
            NODE_COMMAND_EXPRESSION => NodeKind::CommandExpression,
            NODE_COMMENT => NodeKind::Comment,
            NODE_ELIF_BLOCK => NodeKind::ElifBlock,
            NODE_ELSE_BLOCK => NodeKind::ElseBlock,
            NODE_FORMAT_EXPRESSION => NodeKind::FormatExpression,
            NODE_HLL_TYPE_DESCRIPTOR => NodeKind::HllTypeDescriptor,
            NODE_HLL_TYPE_IDENTIFIER => NodeKind::HllTypeIdentifier,
            NODE_IDENTIFIER => NodeKind::Identifier,
            NODE_IF_BLOCK => NodeKind::IfBlock,
            NODE_LABELED_EXPRESSION => NodeKind::LabeledExpression,
            NODE_MACRO => NodeKind::Macro,
            NODE_MACRO_DEFINITION => NodeKind::MacroDefinition,
            NODE_OPTION_EXPRESSION => NodeKind::OptionExpression,
            NODE_PATH => NodeKind::Path,
            NODE_PARAMETER_DECLARATION => NodeKind::ParameterDeclaration,
            NODE_RECURSIVE_MACRO_EXPANSION => NodeKind::RecursiveMacroExpansion,
            NODE_REPEAT_BLOCK => NodeKind::RepeatBlock,
            NODE_SCRIPT => NodeKind::Script,
            NODE_STRING => NodeKind::String,
            NODE_SUBROUTINE_BLOCK => NodeKind::SubroutineBlock,
            NODE_SUBROUTINE_CALL_EXPRESSION => NodeKind::SubroutineCallExpression,
            NODE_UNARY_EXPRESSION => NodeKind::UnaryExpression,
            NODE_WHILE_BLOCK => NodeKind::WhileBlock,
            _ => NodeKind::Unknown,
        },
        None => NodeKind::Unknown,
    }
}

#[expect(unused)]
pub fn name_into_node(name: &str) -> NodeKind {
    match name {
        NODE_ACCESS_CLASS => NodeKind::AccessClass,
        NODE_ARGUMENT_LIST => NodeKind::ArgumentList,
        NODE_ASSIGNMENT_EXPRESSION => NodeKind::AssignmentExpression,
        NODE_BINARY_EXPRESSION => NodeKind::BinaryExpression,
        NODE_BLOCK => NodeKind::Block,
        NODE_CALL_EXPRESSION => NodeKind::CallExpression,
        NODE_COMMAND_EXPRESSION => NodeKind::CommandExpression,
        NODE_COMMENT => NodeKind::Comment,
        NODE_ELIF_BLOCK => NodeKind::ElifBlock,
        NODE_ELSE_BLOCK => NodeKind::ElseBlock,
        NODE_FORMAT_EXPRESSION => NodeKind::FormatExpression,
        NODE_HLL_TYPE_DESCRIPTOR => NodeKind::HllTypeDescriptor,
        NODE_HLL_TYPE_IDENTIFIER => NodeKind::HllTypeIdentifier,
        NODE_IDENTIFIER => NodeKind::Identifier,
        NODE_IF_BLOCK => NodeKind::IfBlock,
        NODE_MACRO => NodeKind::Macro,
        NODE_MACRO_DEFINITION => NodeKind::MacroDefinition,
        NODE_OPTION_EXPRESSION => NodeKind::OptionExpression,
        NODE_PATH => NodeKind::Path,
        NODE_REPEAT_BLOCK => NodeKind::RepeatBlock,
        NODE_SCRIPT => NodeKind::Script,
        NODE_STRING => NodeKind::String,
        NODE_SUBROUTINE_BLOCK => NodeKind::SubroutineBlock,
        NODE_SUBROUTINE_CALL_EXPRESSION => NodeKind::SubroutineCallExpression,
        NODE_UNARY_EXPRESSION => NodeKind::UnaryExpression,
        NODE_WHILE_BLOCK => NodeKind::WhileBlock,
        _ => NodeKind::Unknown,
    }
}

pub fn get_block_opener_ids(lang: &Language) -> [u16; 8] {
    let mut ids = [0u16; 8];
    for (ii, &node) in BLOCK_OPENERS.iter().enumerate() {
        ids[ii] = node_into_id(&lang, node);
    }
    ids
}

pub fn get_control_flow_block_ids(lang: &Language) -> [u16; 5] {
    let mut ids = [0u16; 5];
    for (ii, &node) in CONTROL_FLOW_BLOCKS.iter().enumerate() {
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

pub fn get_macro_container_expr_ids(lang: &Language) -> [u16; 10] {
    let mut ids = [0u16; 10];
    for (ii, &node) in MACRO_CONTAINER_EXPRESSIONS.iter().enumerate() {
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
