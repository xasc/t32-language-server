// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2
//

use crate::{
    ReturnCode,
    config::{SemanticTokenEncoding, SemanticTokenSupport},
    ls::{
        TextDocs,
        doc::{TextDoc, TextDocData},
        lsp::Message,
        request::Notification,
        response::{NullResponse, Response},
        tasks::{Task, Tasks, trace_doc_unknown, try_schedule},
    },
    protocol::{
        LogTraceParams, NumberOrString, Position, Range, SemanticTokens, SemanticTokensLegend,
        SemanticTokensParams, SemanticTokensRangeParams, TraceValue,
    },
    t32::{SemanticToken, do_syntax_highlighting, do_syntax_highlighting_in_range},
};

#[derive(Debug)]
struct SemanticTokenIntegerEncoded {
    line: u32,
    character: u32,
    len: u32,
    r#type: u32,
    r#modifier: u32,
}

#[derive(Debug)]
struct SemanticTokenAbsoluteEncoding(SemanticTokenIntegerEncoded);

#[derive(Debug)]
struct SemanticTokenRelativeEncoding(SemanticTokenIntegerEncoded);

pub fn process_semantic_tokens_full_req(
    id: NumberOrString,
    params: SemanticTokensParams,
    trace_level: TraceValue,
    capabilities: SemanticTokenSupport,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_semantic_tok_full_req(id.clone())));
    }

    let (doc, tree) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, _)) => (doc, tree),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut ts.runner,
        Task::SemanticTokensFull(
            id,
            capabilities.legend,
            capabilities.encoding,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            capture_semantic_tokens_full_doc,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

pub fn process_semantic_tokens_range_req(
    id: NumberOrString,
    params: SemanticTokensRangeParams,
    trace_level: TraceValue,
    capabilities: SemanticTokenSupport,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_semantic_tok_range_req(id.clone())));
    }

    let (doc, tree) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, _)) => (doc, tree),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut ts.runner,
        Task::SemanticTokensRange(
            id,
            capabilities.legend,
            capabilities.encoding,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            params.range,
            capture_semantic_tokens_doc_range,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

fn capture_semantic_tokens_full_doc(
    legend: SemanticTokensLegend,
    encoding: SemanticTokenEncoding,
    textdoc: TextDocData,
) -> SemanticTokens {
    let mut tokens = do_syntax_highlighting(legend, &textdoc.doc, &textdoc.tree);
    if !tokens.is_empty() {
        if !encoding.multiline_tokens {
            tokens = split_multiline_semantic_tokens(&textdoc.doc, tokens);
        }
        if !encoding.overlapping_tokens {
            tokens = flatten_semantic_tokens(tokens);
        }
    }
    let tokens = encode_semantic_tokens_absolute_format(&textdoc.doc, &tokens);
    let tokens = encode_semantic_tokens_relative_format(&tokens);
    let tokens = encode_semantic_tokens_integer_array(&tokens);

    SemanticTokens {
        result_id: None,
        data: tokens,
    }
}

fn capture_semantic_tokens_doc_range(
    legend: SemanticTokensLegend,
    encoding: SemanticTokenEncoding,
    textdoc: TextDocData,
    range: Range,
) -> SemanticTokens {
    let mut tokens = do_syntax_highlighting_in_range(legend, &textdoc.doc, &textdoc.tree, &range);
    if !tokens.is_empty() {
        if !encoding.multiline_tokens {
            tokens = split_multiline_semantic_tokens(&textdoc.doc, tokens);
        }
        if !encoding.overlapping_tokens {
            tokens = flatten_semantic_tokens(tokens);
        }
    }
    let tokens = encode_semantic_tokens_absolute_format(&textdoc.doc, &tokens);
    let tokens = encode_semantic_tokens_relative_format(&tokens);
    let tokens = encode_semantic_tokens_integer_array(&tokens);

    SemanticTokens {
        result_id: None,
        data: tokens,
    }
}

fn encode_semantic_tokens_absolute_format(
    doc: &TextDoc,
    tokens: &[SemanticToken],
) -> Vec<SemanticTokenAbsoluteEncoding> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut encoded: Vec<SemanticTokenAbsoluteEncoding> = Vec::with_capacity(tokens.len() + 1);

    // Prepend token list with a virtual token starting at the begin of the
    // file. This simplifies the conversion to relative positions, because all
    // operations become uniform.
    encoded.push(SemanticTokenAbsoluteEncoding(SemanticTokenIntegerEncoded {
        line: 0,
        character: 0,
        len: 0,
        r#type: 0,
        modifier: 0,
    }));

    for token in tokens {
        let span = &token.span;

        // The end of a token is one character after its last character.
        let distance = doc.calculate_distance(&span.start, &span.end);

        let len = if distance > 0 { distance - 1 } else { 0 };

        encoded.push(SemanticTokenAbsoluteEncoding(SemanticTokenIntegerEncoded {
            line: span.start.line,
            character: span.start.character,
            len,
            r#type: token.r#type,
            modifier: token.modifier,
        }));
    }
    encoded
}

fn encode_semantic_tokens_relative_format(
    tokens: &[SemanticTokenAbsoluteEncoding],
) -> Vec<SemanticTokenRelativeEncoding> {
    if tokens.is_empty() {
        return Vec::new();
    }
    // The token list starts with a virtual token with length zero at position 0.
    debug_assert!(tokens.len() > 1);

    let mut encoded: Vec<SemanticTokenRelativeEncoding> = Vec::with_capacity(tokens.len());

    for (ii, SemanticTokenAbsoluteEncoding(tok)) in tokens[1..].iter().enumerate() {
        let prior = &tokens[ii].0;

        let (delta_line, delta_char): (u32, u32) = if prior.line == tok.line {
            debug_assert!(prior.character <= tok.character);
            (0, tok.character - prior.character)
        } else {
            debug_assert!(prior.line <= tok.line);
            (tok.line - prior.line, tok.character)
        };

        encoded.push(SemanticTokenRelativeEncoding(SemanticTokenIntegerEncoded {
            line: delta_line,
            character: delta_char,
            ..*tok
        }));
    }
    encoded
}

fn encode_semantic_tokens_integer_array(tokens: &[SemanticTokenRelativeEncoding]) -> Vec<u32> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut encoded: Vec<u32> = Vec::with_capacity(tokens.len() * 5);

    for SemanticTokenRelativeEncoding(tok) in tokens {
        encoded.push(tok.line);
        encoded.push(tok.character);
        encoded.push(tok.len);
        encoded.push(tok.r#type);
        encoded.push(tok.r#modifier);
    }
    encoded
}

fn split_multiline_semantic_tokens(
    doc: &TextDoc,
    tokens: Vec<SemanticToken>,
) -> Vec<SemanticToken> {
    if !contains_multiline_tokens(&tokens) {
        return tokens;
    }

    let mut extra_tokens: Vec<SemanticToken> = Vec::with_capacity(tokens.len() + 1);
    let mut segments: Vec<SemanticToken> = Vec::with_capacity(2);

    for token in tokens {
        let (start, end) = (&token.span.start, &token.span.end);

        // Tokens ending at the end of the line have set their end to character
        // 0 of the next line. These tokens do not need to be split, because
        // their length does not take them "past the end of the line" (see
        // LSP specification).
        debug_assert!(end.line >= start.line);
        if end.line <= start.line || (end.character == 0 && end.line == start.line + 1) {
            extra_tokens.push(token);
        } else {
            distribute_multiline_token(doc, &token, &mut segments);
            extra_tokens.append(&mut segments);

            segments.clear();
        }
    }
    extra_tokens
}

fn contains_multiline_tokens(tokens: &[SemanticToken]) -> bool {
    for token in tokens {
        let (start, end) = (&token.span.start, &token.span.end);

        debug_assert!(end.line >= start.line);
        if start.line < end.line {
            return true;
        }
    }
    false
}

fn distribute_multiline_token(
    doc: &TextDoc,
    multiline_token: &SemanticToken,
    segments: &mut Vec<SemanticToken>,
) {
    debug_assert!(segments.is_empty());
    debug_assert!(multiline_token.span.start.line < multiline_token.span.end.line);

    let (start, end) = (&multiline_token.span.start, &multiline_token.span.end);

    let end_pos: Position = if let Some(eol) = doc.get_eol_character_offset(start.line as usize) {
        Position {
            line: start.line,
            character: eol,
        }
    } else {
        // We cannot determine the line length of the token start
        // position. The token cannot start at the end of the line,
        // so we recover with the minimal token length of 1.
        Position {
            line: start.line,
            character: start.character + 1,
        }
    };

    segments.push(SemanticToken {
        span: Range {
            start: multiline_token.span.start,
            end: end_pos,
        },
        ..multiline_token.clone()
    });

    let mut offset: u32 = 1;

    while start.line + offset < end.line {
        let start_pos = Position {
            line: start.line + offset,
            character: 0,
        };

        let end_pos: Position =
            if let Some(eol) = doc.get_eol_character_offset((start.line + offset) as usize) {
                Position {
                    line: start_pos.line,
                    character: eol,
                }
            } else {
                Position {
                    line: start_pos.line,
                    character: start.character + 1,
                }
            };

        segments.push(SemanticToken {
            span: Range {
                start: start_pos,
                end: end_pos,
            },
            ..multiline_token.clone()
        });

        offset += 1;
    }

    if end.character > 0 {
        let start_pos = Position {
            line: end.line,
            character: 0,
        };

        segments.push(SemanticToken {
            span: Range {
                start: start_pos,
                end: multiline_token.span.end,
            },
            ..multiline_token.clone()
        });
    }
    debug_assert!(segments.len() > 1);
}

/// See [Note: Order Overlapping Semantic Tokens] for structure of overlapping
/// tokens.
fn flatten_semantic_tokens(tokens: Vec<SemanticToken>) -> Vec<SemanticToken> {
    debug_assert!(!tokens.is_empty());

    let mut flattened: Vec<SemanticToken> = Vec::with_capacity(tokens.len());

    let mut stacked_tokens: Vec<SemanticToken> = Vec::with_capacity(1);
    let mut cutoff = tokens[0].span.start;

    // The next token can only be processed if all its nested children have
    // been found. We add an extra token at the end, so that we can put the
    // complete logic inside the loop. The extra token is then discarded.
    for token in tokens.into_iter().chain([SemanticToken {
        span: Range {
            start: Position {
                line: u32::MAX,
                character: u32::MAX,
            },
            end: Position {
                line: u32::MAX,
                character: u32::MAX,
            },
        },
        r#type: 0,
        modifier: 0,
    }]) {
        let len = stacked_tokens.len();
        if len <= 0 {
            cutoff = token.span.start;
            stacked_tokens.push(token);
            continue;
        }

        let top = &stacked_tokens[len - 1];

        if top.span.contains(&token.span) {
            let top = &top.span;
            let child = &token.span;

            // Fill gap until the nested token starts by adding filler
            // segments. Only the direct parent is checked to determine the
            // filler width.
            if top.start < child.start && cutoff < child.start {
                let filler = SemanticToken {
                    span: Range {
                        start: top.start,
                        end: child.start,
                    },
                    ..stacked_tokens[len - 1]
                };
                flattened.push(filler);
            }
            cutoff = child.start;
        } else {
            // Deal with the prior tokens. Tokens need to be inserted until we
            // find a parent of the current token. Once we have found we have
            // to check wheter we need a filler.
            let mut ii: usize = len;
            while ii > 0 {
                let pos = ii - 1;

                if stacked_tokens[pos].span.contains(&token.span) {
                    let parent = &stacked_tokens[pos].span;
                    let child = &token.span;

                    if parent.start < child.start && cutoff < child.start {
                        let filler = SemanticToken {
                            span: Range {
                                start: if cutoff < parent.start {
                                    parent.start
                                } else {
                                    cutoff
                                },
                                end: child.start,
                            },
                            ..stacked_tokens[pos]
                        };
                        flattened.push(filler);
                    }
                    cutoff = child.start;
                    break;
                }
                let token = stacked_tokens.pop().unwrap();

                let span = &token.span;
                if cutoff < span.end {
                    flattened.push(SemanticToken {
                        span: Range {
                            start: if cutoff < span.start {
                                span.start
                            } else {
                                cutoff
                            },
                            end: span.end,
                        },
                        ..token
                    });
                    cutoff = span.end;
                }
                ii -= 1;
            }
        }
        stacked_tokens.push(token);
    }
    debug_assert_eq!(stacked_tokens.len(), 1);

    flattened
}

fn log_semantic_tok_full_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received semantic tokens for full file request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}

fn log_semantic_tok_range_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received semantic tokens for file range request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{protocol::TextDocumentItem, t32};

    #[test]
    fn can_flatten_overlapping_tokens() {
        let tokens = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 11,
                    },
                    end: Position {
                        line: 0,
                        character: 12,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
        ];

        let flattened = tokens.clone();

        let tokens = flatten_semantic_tokens(tokens);
        debug_assert_eq!(tokens, flattened);

        let tokens = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
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
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 8,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
        ];

        let flattened = vec![
            SemanticToken {
                span: Range {
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
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 8,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
        ];

        let tokens = flatten_semantic_tokens(tokens);
        debug_assert_eq!(tokens, flattened);

        let tokens = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 6,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
        ];

        let flattened = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 2,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 4,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 6,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 8,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
        ];

        let tokens = flatten_semantic_tokens(tokens);
        debug_assert_eq!(tokens, flattened);

        let tokens = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 4,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                r#type: 2,
                modifier: 2,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 10,
                    },
                    end: Position {
                        line: 0,
                        character: 12,
                    },
                },
                r#type: 3,
                modifier: 3,
            },
        ];

        let flattened = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 2,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 2,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 4,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                r#type: 2,
                modifier: 2,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 6,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 8,
                    },
                    end: Position {
                        line: 0,
                        character: 10,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 10,
                    },
                    end: Position {
                        line: 0,
                        character: 12,
                    },
                },
                r#type: 3,
                modifier: 3,
            },
        ];

        let tokens = flatten_semantic_tokens(tokens);
        debug_assert_eq!(tokens, flattened);
    }

    #[test]
    fn can_split_multiline_tokens() {
        let textdoc = TextDoc::from(TextDocumentItem {
            uri: "file:///test.cmm".to_string(),
            language_id: t32::LANGUAGE_ID.to_string(),
            version: 0,
            text: "Line one\nLine two\nLine three\nLine four\n".to_string(),
        });

        let tokens = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 2,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
        ];

        let expected = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 2,
                        character: 0,
                    },
                    end: Position {
                        line: 2,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
        ];

        let split_tokens = split_multiline_semantic_tokens(&textdoc, tokens);
        debug_assert_eq!(split_tokens, expected);

        let tokens = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 1,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 1,
                        character: 5,
                    },
                    end: Position {
                        line: 1,
                        character: 8,
                    },
                },
                r#type: 2,
                modifier: 2,
            },
        ];

        let expected = vec![
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 4,
                    },
                },
                r#type: 0,
                modifier: 0,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 0,
                        character: 5,
                    },
                    end: Position {
                        line: 0,
                        character: 8,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 4,
                    },
                },
                r#type: 1,
                modifier: 1,
            },
            SemanticToken {
                span: Range {
                    start: Position {
                        line: 1,
                        character: 5,
                    },
                    end: Position {
                        line: 1,
                        character: 8,
                    },
                },
                r#type: 2,
                modifier: 2,
            },
        ];

        let split_tokens = split_multiline_semantic_tokens(&textdoc, tokens);
        debug_assert_eq!(split_tokens, expected);
    }
}
