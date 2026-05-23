// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Semantic Token to Tree-sitter Capture Mapping
//! ====================================================
//!
//! We are using these correspondence tables for mapping semantic token types to
//! Tree-sitter grammar captures:
//!
//!   | Token Type  | Capture              | Nodes                                              | Comment                                   |
//!   | ----------- | -------------------- | -------------------------------------------------- | ----------------------------------------- |
//!   | comment     | @comment             |                                                    |                                           |
//!   | enum        | @constant.builtin    | child of (format_expression)                       |                                           |
//!   | function    | @function            |                                                    |                                           |
//!   | function    | @function.builtin    | (identifier)                                       | PRACTICE functions                        |
//!   | function    | @function.call       | child of (subroutine_call_expression)              | Subroutine calls                          |
//!   | keyword     | @keyword             |                                                    |                                           |
//!   | keyword     | @conditional.ternary |                                                    |                                           |
//!   | keyword     | @conditional         | child of (if_block), (elif_block), or (else_block) | `IF` and `ELSE` commands                  |
//!   | keyword     | @keyword.return      | child of (command_expression)                      | `RETURN`, `END`, and `ENDDO` commands     |
//!   | keyword     | @keyword.function    | (identifier)                                       | `SUBROUTINE` command                      |
//!   | keyword     | @keyword.operator    |                                                    |                                           |
//!   | keyword     | @repeat              | child of (while_block) or (repeat_block)           | `WHILE` and `RePeaT` commands             |
//!   | label       | @label               |                                                    |                                           |
//!   | macro       | @variable.builtin    |                                                    | Always contains an "operator" capture.    |
//!   | modifier    | @constant.builtin    | child of (option_expression)                       |                                           |
//!   | number      | @number              |                                                    |                                           |
//!   | parameter   | @variable.parameter  | (macro)                                            | Always contains an "operator" capture.    |
//!   | storage     | @keyword             | child of (macro_definition)                        | `GLOBAL`, `LOCAL`, and `PRIVATE` commands |
//!   | string      | @string              |                                                    |                                           |
//!   | string      | @string.special      | (path)                                             |                                           |
//!   | type        | @type                | (hll_type_identifier), (hll_type_descriptor)       |                                           |
//!   | variable    | @variable            |                                                    |                                           |
//!   | variable    | @constant            |                                                    |                                           |
//!   | operator    | @operator            |                                                    |                                           |
//!
//!   (macro_definition)
//!
//! Tokens can only have a single type.
//!
//!   | Token Modifier | Capture           | Nodes                                               | Comment                                      |
//!   | -------------- | ----------------- | --------------------------------------------------- | -------------------------------------------- |
//!   | abstract       | @string.special   |                                                     |                                              |
//!   | definition     | @keyword          | child of (macro_definition)                         | Definition with "GLOBAL", "LOCAL", "PRIVATE" |
//!   | defaultLibrary | @constant.builtin | child of (format_expression) or (option_expression) |                                              |
//!   | defaultLibrary | @function         |                                                     | PRACTICE functions                           |
//!   | modification   | @keyword          | child of (macro_definition)                         | `GLOBAL`, `LOCAL`, and `PRIVATE` commands    |
//!   | static         | @conditional      | child of (if_block), (elif_block), or (else_block)  | `IF` and `ELSE` commands                     |
//!   | static         | @keyword.return   |                                                     | `RETURN`, `END`, and `ENDDO` commands        |
//!   | static         | @repeat           | child of (while_block) or (repeat_block)            | `WHILE` and `RePeaT` commands                |
//!
//! Semantic tokens selectors are created from token types and modifiers.
//! Selectors map to TextMate scopes according to this table:
//!
//!   | Token Selectors         | TextMate scope                           | Comment                                                                    |
//!   | ----------------------- | ---------------------------------------- | -------------------------------------------------------------------------- |
//!   | comment                 | comment.practice                         |                                                                            |
//!   | enum.defaultLibrary     | constant.language.format.practice        | Command format parameters                                                  |
//!   | function                | entity.name.function.practice            | Function calls would normally map to `entity.name.function.call.practice`. |
//!   | function.defaultLibrary | support.function.trace32.practice        |                                                                            |
//!   | keyword                 | keyword.other.practice                   |                                                                            |
//!   | keyword.modification    | storage.modifier.macro.practice          | `GLOBAL`, `LOCAL`, and `PRIVATE` commands                                  |
//!   | keyword.static          | keyword.control.practice                 | `IF`, `ELSE`, `WHILE`, `RePeaT`, `RETURN`, `END`, and `ENDDO` commands     |
//!   | macro                   | variable.other.macro.practice            |                                                                            |
//!   | macro.definition        | variable.other.macro.definition.practice |                                                                            |
//!   | modifier.defaultLibrary | constant.language.option.practice        | Command option parameters                                                  |
//!   | number                  | constant.numeric.practice                |                                                                            |
//!   | operator                | keyword.operator.practice                |                                                                            |
//!   | parameter               | variable.parameter.practice              |                                                                            |
//!   | string                  | string.quoted.double.practice            |                                                                            |
//!   | string.abstract         | string.other.path.practice               |                                                                            |
//!
//! A language node may match multiple query captures with equal validity. In
//! such cases we need to use the pattern index for prioritization. The pattern
//! with the higher index should be chosen. The pattern index corresponds to
//! the file position of a pattern. Patterns that appear later in the file have
//! a higher index.
//!
//!
//! [Note] Semantic Token Format
//!
//! Even though the `TokenFormat` is extensible, the specification states that
//! the "only format that is currently specified is relative". So, there is no
//! real choice: There is only a single format. Absolute positions are not
//! supported.
//!
//!
//! [Note] Order Overlapping Semantic Tokens
//!
//! Tree-sitter sorts tokens having the same start position by their width.
//! Tokens with longer width and larger end position come before tokens with
//! shorter width. Parent tokens before nested ones.
//!
//! Nested semantic tokens do not extend beyond the limits of their parent.
//!

mod captures;

use std::sync::LazyLock;

use tree_sitter::{LanguageRef, Query, QueryCaptures, QueryCursor, StreamingIterator, Tree};
use tree_sitter_t32::HIGHLIGHTS_QUERY;

use crate::{
    ls::TextDoc,
    protocol::{
        FoldingRange, FoldingRangeKind, Range as LRange, SemanticTokenModifiers,
        SemanticTokenTypes, SemanticTokensLegend,
    },
    t32::{NodeKind, parse_full},
    utils::BRange,
};

use captures::{
    CAPTURE_BLOCK, CAPTURE_COMMENT, CAPTURE_CONDITIONAL, CAPTURE_CONDITIONAL_TERNARY,
    CAPTURE_CONSTANT, CAPTURE_CONSTANT_BUILTIN, CAPTURE_FUNCTION, CAPTURE_FUNCTION_BUILTIN,
    CAPTURE_FUNCTION_CALL, CAPTURE_KEYWORD, CAPTURE_KEYWORD_FUNCTION, CAPTURE_KEYWORD_OPERATOR,
    CAPTURE_KEYWORD_RETURN, CAPTURE_LABEL, CAPTURE_NUMBER, CAPTURE_OPERATOR, CAPTURE_REPEAT,
    CAPTURE_STRING, CAPTURE_STRING_SPECIAL, CAPTURE_TYPE, CAPTURE_VARIABLE,
    CAPTURE_VARIABLE_BUILTIN, CAPTURE_VARIABLE_PARAMETER,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticToken {
    pub span: LRange,
    pub r#type: u32,
    /// Already in bitwise encoding
    pub modifier: u32,
}

#[derive(Debug)]
struct SemanticTokenQueryCaptures {
    types: SemanticTokenQueryCaptureMap,
    modifiers: SemanticTokenQueryCaptureMap,
}

struct SemanticTokenQueryCaptureMapIterator<'a> {
    captures: &'a SemanticTokenQueryCaptureMap,
    idx: usize,
}

#[derive(Debug)]
struct SemanticTokenQueryCaptureMap(pub Vec<usize>, pub Vec<usize>, pub Vec<u32>);

static QUERY_HIGHLIGHTS_CACHED: LazyLock<Query> = LazyLock::new(|| {
    let tree = parse_full(&[]);
    Query::new(&tree.language(), HIGHLIGHTS_QUERY).expect("Highlights query must be valid")
});

static QUERY_FOLDS_CACHED: LazyLock<Query> = LazyLock::new(|| {
    let tree = parse_full(&[]);
    Query::new(
        &tree.language(),
        r#"(block) @block

(comment) @comment

[
    (assignment_expression)
    (command_expression)
    (macro_definition)
    (macro)
    (if_block)
    (parameter_declaration)
    (recursive_macro_expansion)
    (repeat_block)
    (subroutine_block)
    (subroutine_call_expression)
    (while_block)
    (labeled_expression)
] @other"#,
    )
    .expect("Highlights query must be valid")
});

impl SemanticTokenQueryCaptures {
    pub fn new(legend: &SemanticTokensLegend, query: &Query) -> Self {
        Self {
            types: Self::map_token_types_to_captures(&legend.token_types, query),
            modifiers: Self::map_token_modifiers_to_captures(&legend.token_modifiers, &query),
        }
    }

    fn map_token_types_to_captures(
        types: &[SemanticTokenTypes],
        query: &Query,
    ) -> SemanticTokenQueryCaptureMap {
        let mut captures = SemanticTokenQueryCaptureMap(
            Vec::with_capacity(types.len()),
            Vec::with_capacity(types.len()),
            Vec::with_capacity(types.len()),
        );
        for token in types {
            captures.0.push(captures.2.len());
            match token {
                SemanticTokenTypes::Operator => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_OPERATOR)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Keyword => {
                    let ts = [
                        CAPTURE_CONDITIONAL,
                        CAPTURE_KEYWORD_FUNCTION,
                        CAPTURE_KEYWORD,
                        CAPTURE_KEYWORD_OPERATOR,
                        CAPTURE_CONDITIONAL_TERNARY,
                        CAPTURE_REPEAT,
                        CAPTURE_KEYWORD_RETURN,
                    ];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenTypes::Modifier => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_CONSTANT_BUILTIN)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::String => {
                    let ts = [CAPTURE_STRING, CAPTURE_STRING_SPECIAL];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenTypes::Number => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_NUMBER)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Type => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_TYPE)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Variable => {
                    let ts = [CAPTURE_VARIABLE, CAPTURE_CONSTANT];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenTypes::Macro => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_VARIABLE_BUILTIN)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Function => {
                    let ts = [
                        CAPTURE_FUNCTION,
                        CAPTURE_FUNCTION_BUILTIN,
                        CAPTURE_FUNCTION_CALL,
                    ];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenTypes::Parameter => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_VARIABLE_PARAMETER)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Label => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_LABEL)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Comment => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_COMMENT)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Enum => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_CONSTANT_BUILTIN)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenTypes::Class
                | SemanticTokenTypes::Decorator
                | SemanticTokenTypes::EnumMember
                | SemanticTokenTypes::Event
                | SemanticTokenTypes::Interface
                | SemanticTokenTypes::Method
                | SemanticTokenTypes::Namespace
                | SemanticTokenTypes::Property
                | SemanticTokenTypes::Regexp
                | SemanticTokenTypes::Struct
                | SemanticTokenTypes::TypeParameter => {
                    unreachable!("Semantic token type not available for language.")
                }
            };
        }
        debug_assert_eq!(types.len(), captures.0.len());
        debug_assert_eq!(captures.0.len(), captures.1.len());
        debug_assert!(captures.0.len() <= captures.2.len());

        captures
    }

    fn map_token_modifiers_to_captures(
        modifiers: &[SemanticTokenModifiers],
        query: &Query,
    ) -> SemanticTokenQueryCaptureMap {
        let mut captures = SemanticTokenQueryCaptureMap(
            Vec::with_capacity(modifiers.len()),
            Vec::with_capacity(modifiers.len()),
            Vec::with_capacity(modifiers.len()),
        );
        for modifier in modifiers {
            captures.0.push(captures.2.len());
            match modifier {
                SemanticTokenModifiers::Definition => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_VARIABLE_BUILTIN)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenModifiers::Static => {
                    let ts = [CAPTURE_CONDITIONAL, CAPTURE_REPEAT, CAPTURE_KEYWORD_RETURN];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenModifiers::Abstract => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_STRING_SPECIAL)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenModifiers::Modification => {
                    captures.1.push(1usize);
                    captures.2.push(
                        query
                            .capture_index_for_name(CAPTURE_KEYWORD)
                            .expect("Capture name must exist."),
                    );
                }
                SemanticTokenModifiers::DefaultLibrary => {
                    let ts = [CAPTURE_FUNCTION_BUILTIN, CAPTURE_CONSTANT_BUILTIN];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenModifiers::Async
                | SemanticTokenModifiers::Declaration
                | SemanticTokenModifiers::Deprecated
                | SemanticTokenModifiers::Documentation
                | SemanticTokenModifiers::Readonly => {
                    unreachable!("Semantic token modifier not available for language.")
                }
            };
        }
        debug_assert_eq!(modifiers.len(), captures.0.len());
        debug_assert_eq!(captures.0.len(), captures.1.len());
        debug_assert!(captures.0.len() <= captures.2.len());

        captures
    }
}

impl SemanticTokenQueryCaptureMap {
    pub fn iter<'a>(&'a self) -> SemanticTokenQueryCaptureMapIterator<'a> {
        SemanticTokenQueryCaptureMapIterator {
            captures: self,
            idx: 0,
        }
    }
}

impl<'a> Iterator for SemanticTokenQueryCaptureMapIterator<'a> {
    type Item = &'a [u32];

    fn next(&mut self) -> Option<Self::Item> {
        debug_assert_eq!(self.captures.0.len(), self.captures.1.len());
        debug_assert!(self.captures.0.len() <= self.captures.2.len());

        if self.idx < self.captures.0.len() {
            let offset = self.idx;
            self.idx += 1;

            let start = self.captures.0[offset];
            let end = start + self.captures.1[offset];

            Some(&self.captures.2[start..end])
        } else {
            None
        }
    }
}

pub fn do_syntax_highlighting(
    legend: SemanticTokensLegend,
    doc: &TextDoc,
    tree: &Tree,
) -> Vec<SemanticToken> {
    debug_assert!(!(legend.token_types.is_empty() && legend.token_modifiers.is_empty()));

    let query = &*QUERY_HIGHLIGHTS_CACHED;

    let mut cursor = QueryCursor::new();

    let Some((num, matches)) = run_query(&query, &tree, &doc, &mut cursor) else {
        return Vec::new();
    };
    let captures = SemanticTokenQueryCaptures::new(&legend, &query);

    capture_semantic_tokens(
        &doc,
        &tree.language(),
        &legend,
        &query,
        &captures,
        num,
        r#matches,
    )
}

pub fn do_syntax_highlighting_in_range(
    legend: SemanticTokensLegend,
    doc: &TextDoc,
    tree: &Tree,
    range: &LRange,
) -> Vec<SemanticToken> {
    debug_assert!(!(legend.token_types.is_empty() && legend.token_modifiers.is_empty()));

    let query = &*QUERY_HIGHLIGHTS_CACHED;

    let span = doc.to_byte_range(&range.start, &range.end);

    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(span.to_inner());

    let Some((num, matches)) = run_query(&query, &tree, &doc, &mut cursor) else {
        return Vec::new();
    };

    if num <= 0 {
        return Vec::new();
    }
    let captures = SemanticTokenQueryCaptures::new(&legend, &query);

    capture_semantic_tokens(
        &doc,
        &tree.language(),
        &legend,
        &query,
        &captures,
        num,
        r#matches,
    )
}

pub fn list_code_folds(doc: &TextDoc, tree: &Tree) -> Vec<FoldingRange> {
    let query = &*QUERY_FOLDS_CACHED;

    let mut cursor = QueryCursor::new();

    let Some((num, matches)) = run_query(&query, &tree, &doc, &mut cursor) else {
        return Vec::new();
    };

    if num <= 0 {
        return Vec::new();
    }
    capture_code_folds(&doc, &query, num, r#matches)
}

fn capture_semantic_tokens<'a, 'b>(
    doc: &'b TextDoc,
    lang: &LanguageRef,
    legend: &SemanticTokensLegend,
    query: &Query,
    selection: &SemanticTokenQueryCaptures,
    num_matches: usize,
    matches: QueryCaptures<'a, 'a, &'b [u8], &'b [u8]>,
) -> Vec<SemanticToken> {
    debug_assert_ne!(num_matches, 0);

    let (const_builtin, operator, id_type, keyword, var, var_builtin) = {
        let id_constant_builtin = query
            .capture_index_for_name(CAPTURE_CONSTANT_BUILTIN)
            .expect("Capture name must exist.");
        let id_operator = query
            .capture_index_for_name(CAPTURE_OPERATOR)
            .expect("Capture name must exist.");
        let id_keyword = query
            .capture_index_for_name(CAPTURE_KEYWORD)
            .expect("Capture name must exist.");
        let id_type = query
            .capture_index_for_name(CAPTURE_TYPE)
            .expect("Capture name must exist.");
        let id_var = query
            .capture_index_for_name(CAPTURE_VARIABLE)
            .expect("Capture name must exist.");
        let id_var_builtin = query
            .capture_index_for_name(CAPTURE_VARIABLE_BUILTIN)
            .expect("Capture name must exist.");

        (
            id_constant_builtin,
            id_operator,
            id_type,
            id_keyword,
            id_var,
            id_var_builtin,
        )
    };

    let (
        command_expr,
        format_expr,
        hll_type_identifier,
        hll_type_descriptor,
        r#macro,
        macro_definition,
        option_expr,
        param_decl,
        subroutine_call_expr,
    ) = {
        let id_command_expr = NodeKind::CommandExpression.into_id(&lang);
        let id_format_expr = NodeKind::FormatExpression.into_id(&lang);
        let id_hll_type_descriptor = NodeKind::HllTypeDescriptor.into_id(&lang);
        let id_hll_type_identifier = NodeKind::HllTypeIdentifier.into_id(&lang);
        let id_macro = NodeKind::Macro.into_id(&lang);
        let id_macro_definition = NodeKind::MacroDefinition.into_id(&lang);
        let id_option_expr = NodeKind::OptionExpression.into_id(&lang);
        let id_param_decl = NodeKind::ParameterDeclaration.into_id(&lang);
        let id_subroutine_call_expr = NodeKind::SubroutineCallExpression.into_id(&lang);
        (
            id_command_expr,
            id_format_expr,
            id_hll_type_descriptor,
            id_hll_type_identifier,
            id_macro,
            id_macro_definition,
            id_option_expr,
            id_param_decl,
            id_subroutine_call_expr,
        )
    };

    let num_patterns = query.pattern_count() as u32;

    // No query pattern captures the root node
    let mut prior_id_captured = num_patterns;

    let legend_modifier = legend
        .token_types
        .iter()
        .position(|t| *t == SemanticTokenTypes::Modifier)
        .unwrap_or(usize::MAX);

    let mut tokens: Vec<SemanticToken> = Vec::with_capacity(num_matches);
    matches.for_each(|(m, idx)| {
        let capture = m.captures[*idx];

        let node = &capture.node;
        let span = BRange::from(node.byte_range());
        if span.inner().start == span.inner().end {
            return;
        }

        // Macros will always capture the `&` as separate operator.
        if capture.index == operator
            && let Some(parent) = node.parent()
        {
            if parent.kind_id() == r#macro {
                return;
            }
        }

        // Prevent capture of command expressions as variables. The variable
        // capture has a higher priority.
        if capture.index == var
            && let Some(parent) = node.parent()
        {
            if [
                command_expr,
                macro_definition,
                subroutine_call_expr,
                param_decl,
                option_expr,
                format_expr,
            ]
            .contains(&parent.kind_id())
            {
                return;
            }
        }

        let mut modifier: u32 = 0;
        for (ii, r#mods) in selection.modifiers.iter().enumerate() {
            if mods.iter().any(|m| *m == capture.index) {
                if [var_builtin, keyword].contains(&capture.index) {
                    let Some(parent) = node.parent() else {
                        continue;
                    };

                    if parent.kind_id() != macro_definition {
                        continue;
                    }
                }
                modifier |= 1u32 << ii;
            }
        }

        // TODO: Differentiate subroutines from labels.
        for (ii, types) in selection.types.iter().enumerate() {
            if types.iter().any(|t| *t == capture.index) {
                if capture.index == id_type {
                    let kind = capture.node.kind_id();
                    if !(kind == hll_type_descriptor || kind == hll_type_identifier) {
                        continue;
                    }
                } else if capture.index == const_builtin
                    && let Some(parent) = node.parent()
                {
                    if parent.kind_id() == format_expr && ii == legend_modifier {
                        continue;
                    }
                }

                let bytes = capture.node.byte_range();
                let span = doc.to_range(bytes.start, bytes.end);

                let token = SemanticToken {
                    span,
                    r#type: ii as u32,
                    modifier,
                };

                if let Some(prior) = tokens.last_mut()
                    && prior.span == token.span
                {
                    // If multiple captures select the same range, then we only
                    // store the one with the highest capture index.
                    if capture.index > prior_id_captured {
                        *prior = token;
                    } else {
                        continue;
                    }
                } else {
                    tokens.push(token);
                }
                prior_id_captured = capture.index;
            }
        }
    });
    debug_assert!(
        tokens
            .iter()
            .all(|t| t.r#type < selection.types.0.len() as u32)
    );
    debug_assert!(
        tokens
            .iter()
            .all(|t| t.modifier < (1u32 << selection.modifiers.0.len()))
    );

    tokens
}

fn capture_code_folds<'a, 'b>(
    doc: &'b TextDoc,
    query: &Query,
    num_matches: usize,
    matches: QueryCaptures<'a, 'a, &'b [u8], &'b [u8]>,
) -> Vec<FoldingRange> {
    debug_assert_ne!(num_matches, 0);

    let (block, comment) = {
        let id_block = query
            .capture_index_for_name(CAPTURE_BLOCK)
            .expect("Capture name must exist.");
        let id_comment = query
            .capture_index_for_name(CAPTURE_COMMENT)
            .expect("Capture name must exist.");
        (id_block, id_comment)
    };

    let num_patterns = query.pattern_count() as u32;

    const MIN_LEN_FOLD_COMMENT: u32 = 3;

    let mut prior_id_captured = num_patterns;

    let mut folds: Vec<FoldingRange> = Vec::with_capacity(num_matches / 2);
    matches.for_each(|(m, idx)| {
        let capture = m.captures[*idx];

        let node = &capture.node;
        let span = BRange::from(node.byte_range());
        if span.inner().start == span.inner().end {
            return;
        }

        let bytes = capture.node.byte_range();
        let span = doc.to_range(bytes.start, bytes.end);

        // Set a fold for a comment block only if it is at least 3 lines long.
        if prior_id_captured == comment {
            let prior = folds.last_mut().expect("Must point to (comment) node.");
            debug_assert!(prior.end_line >= prior.end_line);

            if capture.index == comment {
                if prior.end_line == span.start.line {
                    prior.end_line = span.end.line;
                    prior.end_character = Some(span.end.character);
                } else {
                    let fold = FoldingRange {
                        start_line: span.start.line,
                        start_character: Some(span.start.character),
                        end_line: span.end.line,
                        end_character: Some(span.end.character),
                        kind: Some(FoldingRangeKind::Comment),
                        collapsed_text: None,
                    };

                    let len = prior.end_line - prior.start_line;
                    if len >= MIN_LEN_FOLD_COMMENT {
                        folds.push(fold);
                    } else {
                        *prior = fold;
                    }
                }
            } else if capture.index == block {
                let fold = FoldingRange {
                    start_line: span.start.line,
                    start_character: Some(span.start.character),
                    end_line: span.end.line,
                    end_character: Some(span.end.character),
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                };

                let len = prior.end_line - prior.start_line;
                if len >= MIN_LEN_FOLD_COMMENT {
                    folds.push(fold);
                } else {
                    *prior = fold;
                }
            }
        } else if capture.index == comment || capture.index == block {
            let kind = if capture.index == block {
                Some(FoldingRangeKind::Region)
            } else if capture.index == comment {
                Some(FoldingRangeKind::Comment)
            } else {
                unreachable!("No other capture type possible.");
            };

            let fold = FoldingRange {
                start_line: span.start.line,
                start_character: Some(span.start.character),
                end_line: span.end.line,
                end_character: Some(span.end.character),
                kind,
                collapsed_text: None,
            };
            folds.push(fold);
        }
        prior_id_captured = capture.index;
    });

    // Remove the last comment block if it is too short.
    if prior_id_captured == comment {
        let prior = folds.last().expect("Must point to (comment) node.");
        debug_assert!(prior.end_line >= prior.end_line);

        let len = prior.end_line - prior.start_line;
        if len < 3 {
            drop(folds.pop());
        }
    }
    folds
}

fn run_query<'a, 'b>(
    query: &'a Query,
    tree: &'a Tree,
    doc: &'b TextDoc,
    cursor: &'a mut QueryCursor,
) -> Option<(usize, QueryCaptures<'a, 'a, &'b [u8], &'b [u8]>)> {
    let matches = cursor.captures(query, tree.root_node(), doc.text.as_bytes());
    let count = matches.count();
    if count <= 0 {
        return None;
    }

    let matches = cursor.captures(query, tree.root_node(), doc.text.as_bytes());
    Some((count, matches))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path;

    use url::Url;

    use crate::{config, ls, protocol::Position, utils};

    fn create_full_legend() -> SemanticTokensLegend {
        let types = vec![
            SemanticTokenTypes::Operator,
            SemanticTokenTypes::Keyword,
            SemanticTokenTypes::Modifier,
            SemanticTokenTypes::String,
            SemanticTokenTypes::Number,
            SemanticTokenTypes::Type,
            SemanticTokenTypes::Variable,
            SemanticTokenTypes::Macro,
            SemanticTokenTypes::Function,
            SemanticTokenTypes::Parameter,
            SemanticTokenTypes::Label,
            SemanticTokenTypes::Comment,
            SemanticTokenTypes::Enum,
        ];

        let modifiers = vec![
            SemanticTokenModifiers::Definition,
            SemanticTokenModifiers::DefaultLibrary,
            SemanticTokenModifiers::Abstract,
            SemanticTokenModifiers::Modification,
            SemanticTokenModifiers::Static,
        ];

        SemanticTokensLegend {
            token_types: types,
            token_modifiers: modifiers,
        }
    }

    #[test]
    fn can_determine_syntax_highlights() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let types = vec![
            SemanticTokenTypes::Operator,
            SemanticTokenTypes::Keyword,
            SemanticTokenTypes::Modifier,
            SemanticTokenTypes::String,
            SemanticTokenTypes::Number,
            SemanticTokenTypes::Type,
            SemanticTokenTypes::Macro,
            SemanticTokenTypes::Function,
            SemanticTokenTypes::Parameter,
            SemanticTokenTypes::Label,
            SemanticTokenTypes::Comment,
        ];

        let modifiers = vec![SemanticTokenModifiers::Definition];

        let legend = SemanticTokensLegend {
            token_types: types.clone(),
            token_modifiers: modifiers.clone(),
        };

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);
        debug_assert!(!tokens.is_empty());

        for r#type in types {
            let pos = legend
                .token_types
                .iter()
                .position(|t| *t == r#type)
                .expect("Must be in legend.") as u32;
            assert!(tokens.iter().any(|t| t.r#type == pos));
        }
        assert!(
            tokens
                .iter()
                .all(|t| (t.r#type as usize) < legend.token_types.len())
        );

        for modifier in modifiers {
            let pos = legend
                .token_modifiers
                .iter()
                .position(|t| *t == modifier)
                .expect("Must be in legend.") as u32;
            assert!(tokens.iter().any(|t| (t.modifier & (1u32 << pos)) > 0));
        }
        assert!(
            tokens
                .iter()
                .all(|t| (t.modifier as usize) < (1usize << legend.token_modifiers.len()))
        );
    }

    #[test]
    fn does_not_capture_operators_in_macros() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let legend = SemanticTokensLegend {
            token_types: vec![SemanticTokenTypes::Operator],
            token_modifiers: Vec::new(),
        };

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);
        debug_assert!(!tokens.is_empty());
        debug_assert!(!tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 4,
                        character: 0,
                    },
                    end: Position {
                        line: 4,
                        character: 1,
                    },
                },
                r#type: 0,
                modifier: 0,
            }));

        let legend = SemanticTokensLegend {
            token_types: vec![SemanticTokenTypes::Macro, SemanticTokenTypes::Operator],
            token_modifiers: Vec::new(),
        };

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);
        debug_assert!(!tokens.is_empty());
        debug_assert!(!tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 4,
                        character: 0,
                    },
                    end: Position {
                        line: 4,
                        character: 1,
                    },
                },
                r#type: 0,
                modifier: 0,
            }));
    }

    #[test]
    fn does_not_capture_other_keywords_with_function_definition() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let legend = SemanticTokensLegend {
            token_types: vec![SemanticTokenTypes::Function],
            token_modifiers: vec![SemanticTokenModifiers::Definition],
        };

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(!tokens.is_empty());
        debug_assert!(!tokens.iter().any(|t| t.span
            == LRange {
                start: Position {
                    line: 12,
                    character: 0,
                },
                end: Position {
                    line: 12,
                    character: 5,
                },
            }));
        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 16,
                        character: 11,
                    },
                    end: Position {
                        line: 16,
                        character: 15,
                    },
                },
                r#type: 0,
                modifier: 0,
            }));
    }

    #[test]
    fn can_restrict_captures_to_range() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let legend = SemanticTokensLegend {
            token_types: vec![SemanticTokenTypes::Macro],
            token_modifiers: vec![SemanticTokenModifiers::Definition],
        };

        let range = LRange {
            start: Position {
                line: 5,
                character: 2,
            },
            end: Position {
                line: 10,
                character: 0,
            },
        };
        let tokens = do_syntax_highlighting_in_range(legend.clone(), &doc, &tree, &range);

        debug_assert_eq!(tokens.iter().count(), 1);
    }

    #[test]
    fn selects_capture_with_higher_priority() {
        let text = "SUBROUTINE abc\n(\n    RETURN \"&a\"\n)";
        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 1,
                modifier: 0,
            }));
        debug_assert!(!tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 6,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 2,
                        character: 4,
                    },
                    end: Position {
                        line: 2,
                        character: 10,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));
        debug_assert!(!tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 2,
                        character: 4,
                    },
                    end: Position {
                        line: 2,
                        character: 10,
                    },
                },
                r#type: 6,
                modifier: 0,
            }));
    }

    #[test]
    fn ignores_tokens_with_zero_length() {
        let text = "abc:\n(\n    RETURN \"&a\"\n)";
        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(!tokens.iter().any(|t| t.span.start == t.span.end));
    }

    #[test]
    fn sets_token_modifier_for_builtin_functions() {
        let text = "PRINT STATE.RUNNING()\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 6,
                    },
                    end: Position {
                        line: 0,
                        character: 19,
                    },
                },
                r#type: 8,
                modifier: 2,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_parameter_declarations() {
        let text = "PARAMETERS &a &b\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 1,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 11,
                    },
                    end: Position {
                        line: 0,
                        character: 13,
                    },
                },
                r#type: 9,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 14,
                    },
                    end: Position {
                        line: 0,
                        character: 16,
                    },
                },
                r#type: 9,
                modifier: 0,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_control_flow_and_commands_keywords() {
        let text = "IF &a\nPRINT \"hello\"\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 2,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 5,
                    },
                },
                r#type: 1,
                modifier: 0,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_operator_keywords() {
        let text = "&a=1+1";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 3,
                    },
                },
                r#type: 0,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 4,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                r#type: 0,
                modifier: 0,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_strings() {
        let text = "PRINT \"Hello\"+\", World\"\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 6,
                    },
                    end: Position {
                        line: 0,
                        character: 13,
                    },
                },
                r#type: 3,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 14,
                    },
                    end: Position {
                        line: 0,
                        character: 23,
                    },
                },
                r#type: 3,
                modifier: 0,
            }));
    }

    #[test]
    fn captures_paths_as_special_string_semantic_tokens() {
        let text = "DO C:\\run.cmm\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 3,
                    },
                    end: Position {
                        line: 0,
                        character: 13,
                    },
                },
                r#type: 3,
                modifier: 4,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_comments() {
        let text = "&a // Comment\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 1,
                        character: 0,
                    },
                },
                r#type: 11,
                modifier: 0,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_numbers() {
        let text = "&a=1+2\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 3,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 4,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                r#type: 4,
                modifier: 0,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_subroutine_calls() {
        let text = "GOSUB terminate\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                r#type: 1,
                modifier: 0,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 6,
                    },
                    end: Position {
                        line: 0,
                        character: 15,
                    },
                },
                r#type: 8,
                modifier: 0,
            }));
    }

    #[test]
    fn sets_token_modifier_for_macro_definitions() {
        let text = "PRIVATE &a\nLOCAL &b\nGLOBAL &c\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 7,
                    },
                },
                r#type: 1,
                modifier: 1 << 3,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 5,
                    },
                },
                r#type: 1,
                modifier: 1 << 3,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 6,
                    },
                },
                r#type: 1,
                modifier: 1 << 3,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_format_parameters() {
        let text = "Data.Set %Byte\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 10,
                    },
                    end: Position {
                        line: 0,
                        character: 14,
                    },
                },
                r#type: 12,
                modifier: 2,
            }));
    }

    #[test]
    fn captures_semantic_tokens_for_option_parameters() {
        let text = "Data.View /Track\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 11,
                    },
                    end: Position {
                        line: 0,
                        character: 16,
                    },
                },
                r#type: 2,
                modifier: 2,
            }));
    }

    #[test]
    fn sets_token_modifier_for_if_then_keywords() {
        let text = "IF &a\n(\n  PRINT \"hello\"\n)\nELSE\n  PRINT \", world!\"\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 2,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 4,
                        character: 0,
                    },
                    end: Position {
                        line: 4,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));
    }

    #[test]
    fn sets_token_modifier_for_loop_keywords() {
        let text = "WHILE &a\n(\n  PRINT \"hello\"\n)\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));

        let text = "RePeaT &a\n(\n  PRINT \", world!\"\n)\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));
    }

    #[test]
    fn sets_token_modifier_for_return_keywords() {
        let text = "RETURN\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));

        let text = "END TRUE()\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 3,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));

        let text = "ENDDO \"&a\"\n";

        let tree = parse_full(&text.as_bytes());
        let doc = utils::create_doc("file://test.cmm".to_string(), 0, text.to_string());
        let legend = create_full_legend();

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);

        debug_assert!(tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                r#type: 1,
                modifier: 1 << 4,
            }));
    }

    #[test]
    fn can_mark_code_folds() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/folds.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let folds = list_code_folds(&doc, &tree);

        assert!(folds.iter().any(|f| *f
            == FoldingRange {
                start_line: 0,
                start_character: Some(0),
                end_line: 3,
                end_character: Some(0),
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            }));

        assert!(folds.iter().any(|f| *f
            == FoldingRange {
                start_line: 6,
                start_character: Some(0),
                end_line: 11,
                end_character: Some(0),
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            }));

        assert!(
            folds
                .iter()
                .any(|f| !(f.start_line == 14 || f.start_line == 15 || f.start_line == 17))
        );

        assert!(folds.iter().any(|f| *f
            == FoldingRange {
                start_line: 20,
                start_character: Some(0),
                end_line: 24,
                end_character: Some(0),
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            }));

        assert!(folds.iter().any(|f| *f
            == FoldingRange {
                start_line: 26,
                start_character: Some(0),
                end_line: 29,
                end_character: Some(0),
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            }));
    }
}
