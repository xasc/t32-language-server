// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Semantic Token to Tree-sitter Capture Mapping
//! ==========================================================
//!
//! We are using these correspondence tables for mapping semantic token types to
//! grammar captures:
//!
//!   | Token Type  | Capture             | Nodes                                         | Comment                                 |
//!   | ----------- | ------------------- | --------------------------------------------- | --------------------------------------- |
//!   | operator    | operator            |                                               |                                         |
//!   | keyword     | keyword             |                                               |                                         |
//!   | keyword     | keyword.operator    |                                               |                                         |
//!   | keyword     | conditional.ternary |                                               |                                         |
//!   | keyword     | conditional         |                                               |                                         |
//!   | keyword     | repeat              |                                               |                                         |
//!   | keyword     | keyword.return      |                                               |                                         |
//!   | keyword     | keyword.function    |                                               |                                         |
//!   | modifier    | constant.builtin    |                                               |                                         |
//!   | string      | string              |                                               |                                         |
//!   | string      | string.special      |                                               |                                         |
//!   | number      | number              |                                               |                                         |
//!   | type        | type                | (hll_type_identifier), (hll_type_descriptor)  |                                         |
//!   | variable    | variable            |                                               |                                         |
//!   | variable    | constant            |                                               |                                         |
//!   | macro       | variable.builtin    |                                               | Always contains an "operator" capture.  |
//!   | function    | function            |                                               |                                         |
//!   | function    | function.call       |                                               |                                         |
//!   | parameter   | variable.parameter  |                                               |                                         |
//!   | label       | label               |                                               |                                         |
//!   | comment     | comment             |                                               |                                         |
//!
//!
//! Tokens can only have a single type.
//!
//!   | Token Modifier | Capture  | Nodes                        | Comment                       |
//!   | -------------- | -------- | ---------------------------- | ----------------------------- |
//!   | definition     | keyword  | child of (macro_definition)  | "GLOBAL", "LOCAL", "PRIVATE"  |
//!   | definition     | function |                              |                               |
//!
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
    protocol::{Range as LRange, SemanticTokenModifiers, SemanticTokenTypes, SemanticTokensLegend},
    t32::{NodeKind, parse},
    utils::BRange,
};

use captures::{
    CAPTURE_COMMENT, CAPTURE_CONDITIONAL, CAPTURE_CONDITIONAL_TERNARY, CAPTURE_CONSTANT,
    CAPTURE_CONSTANT_MODIFIER, CAPTURE_FUNCTION, CAPTURE_FUNCTION_CALL, CAPTURE_KEYWORD,
    CAPTURE_KEYWORD_FUNCTION, CAPTURE_KEYWORD_OPERATOR, CAPTURE_KEYWORD_RETURN, CAPTURE_LABEL,
    CAPTURE_NUMBER, CAPTURE_OPERATOR, CAPTURE_REPEAT, CAPTURE_STRING, CAPTURE_STRING_SPECIAL,
    CAPTURE_TYPE, CAPTURE_VARIABLE, CAPTURE_VARIABLE_BUILTIN, CAPTURE_VARIABLE_PARAMETER,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticToken {
    pub span: LRange,
    pub r#type: u32,
    /// Already in bitwise encoding
    pub modifier: u32,
}

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

pub static QUERY_HIGHLIGHTS_CACHED: LazyLock<Query> = LazyLock::new(|| {
    let tree = parse(&[], None);
    Query::new(&tree.language(), HIGHLIGHTS_QUERY).expect("Highlights query must be valid")
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
                            .capture_index_for_name(CAPTURE_CONSTANT_MODIFIER)
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
                    let ts = [CAPTURE_FUNCTION, CAPTURE_FUNCTION_CALL];

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
                SemanticTokenTypes::Class
                | SemanticTokenTypes::Decorator
                | SemanticTokenTypes::Enum
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
                    let ts = [CAPTURE_KEYWORD, CAPTURE_FUNCTION];

                    captures.1.push(ts.len());
                    for capture in ts {
                        captures.2.push(
                            query
                                .capture_index_for_name(capture)
                                .expect("Capture name must exist."),
                        );
                    }
                }
                SemanticTokenModifiers::Abstract
                | SemanticTokenModifiers::Async
                | SemanticTokenModifiers::Declaration
                | SemanticTokenModifiers::DefaultLibrary
                | SemanticTokenModifiers::Deprecated
                | SemanticTokenModifiers::Documentation
                | SemanticTokenModifiers::Modification
                | SemanticTokenModifiers::Readonly
                | SemanticTokenModifiers::Static => {
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

    capture_semantic_tokens(&doc, &tree.language(), &query, &captures, num, r#matches)
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
    let captures = SemanticTokenQueryCaptures::new(&legend, &query);

    capture_semantic_tokens(&doc, &tree.language(), &query, &captures, num, r#matches)
}

fn capture_semantic_tokens<'a, 'b>(
    doc: &'b TextDoc,
    lang: &LanguageRef,
    query: &Query,
    selection: &SemanticTokenQueryCaptures,
    num_matches: usize,
    matches: QueryCaptures<'a, 'a, &'b [u8], &'b [u8]>,
) -> Vec<SemanticToken> {
    let (operator, id_type, keyword, var_builtin) = {
        let id_operator = query
            .capture_index_for_name(CAPTURE_OPERATOR)
            .expect("Capture name must exist.");
        let id_type = query
            .capture_index_for_name(CAPTURE_TYPE)
            .expect("Capture name must exist.");
        let id_keyword = query
            .capture_index_for_name(CAPTURE_KEYWORD)
            .expect("Capture name must exist.");
        let id_var_builtin = query
            .capture_index_for_name(CAPTURE_VARIABLE_BUILTIN)
            .expect("Capture name must exist.");

        (id_operator, id_type, id_keyword, id_var_builtin)
    };

    let (macro_definition, hll_type_identifier, hll_type_descriptor) = {
        let id_macro_definition = NodeKind::MacroDefinition.into_id(&lang);
        let id_hll_type_descriptor = NodeKind::HllTypeDescriptor.into_id(&lang);
        let id_hll_type_identifier = NodeKind::HllTypeIdentifier.into_id(&lang);
        (
            id_macro_definition,
            id_hll_type_descriptor,
            id_hll_type_identifier,
        )
    };

    let num_patterns = query.pattern_count() as u32;

    // No query pattern captures the root node
    let mut prior_id = num_patterns;
    let mut prior_span: BRange = BRange::from(0..0);

    let mut tokens: Vec<SemanticToken> = Vec::with_capacity(num_matches);
    matches.for_each(|(m, idx)| {
        let capture = m.captures[*idx];

        let node = &capture.node;
        let span = BRange::from(node.byte_range());

        // Macros will always capture the `&` as separate operator.
        if capture.index == operator && prior_id == var_builtin && span.contained(&prior_span) {
            prior_id = capture.index;
            prior_span = span;
            return;
        }

        let mut modifier: u32 = 0;
        for (ii, r#mods) in selection.modifiers.iter().enumerate() {
            if mods.iter().any(|m| *m == capture.index) {
                if capture.index == keyword {
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
                }

                let range = capture.node.byte_range();
                tokens.push(SemanticToken {
                    span: doc.to_range(range.start, range.end),
                    r#type: ii as u32,
                    modifier,
                });
            }
        }
        prior_id = capture.index;
        prior_span = span;
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

    use crate::{ls::read_doc, protocol::Position, utils::create_file_idx};

    #[test]
    fn can_determine_synax_highlights() {
        let files = create_file_idx();
        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = read_doc(uri_a, files).expect("Must not fail.");

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
        let files = create_file_idx();
        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = read_doc(uri_a, files).expect("Must not fail.");

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
    fn does_not_capture_other_keywords_as_modifiers() {
        let files = create_file_idx();
        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = read_doc(uri_a, files).expect("Must not fail.");

        let legend = SemanticTokensLegend {
            token_types: vec![SemanticTokenTypes::Function],
            token_modifiers: vec![SemanticTokenModifiers::Definition],
        };

        let tokens = do_syntax_highlighting(legend.clone(), &doc, &tree);
        debug_assert!(!tokens.is_empty());
        debug_assert!(!tokens.iter().any(|t| *t
            == SemanticToken {
                span: LRange {
                    start: Position {
                        line: 12,
                        character: 0,
                    },
                    end: Position {
                        line: 12,
                        character: 5,
                    },
                },
                r#type: 0,
                modifier: 1,
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
                modifier: 1,
            }));
    }

    #[test]
    fn can_restrict_captures_to_range() {
        let files = create_file_idx();
        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/semantic.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = read_doc(uri_a, files).expect("Must not fail.");

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
}
