// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use core::{char, str};
use std::{collections, io::BufRead};

use crate::{
    parser::{Token, TokenType},
    ErrorCode,
};

struct ScanState<'a> {
    source: str::Chars<'a>,
    lookahead: collections::VecDeque<char>,
    lexeme: &'a mut String,
    line: u32,
    column: u32,
}

pub fn scan(buf: &mut impl BufRead, line: &mut u32) -> Result<Vec<Token>, ErrorCode> {
    let mut tokens = Vec::<Token>::new();

    let mut lexeme = String::new();
    let mut msg = String::new();

    loop {
        msg.clear();
        match read_line(buf, &mut msg) {
            Some(err) => return Err(err),
            None => {
                if msg.len() <= 0 {
                    break;
                }
            }
        }

        println!("{}", msg);

        let mut state = ScanState {
            source: msg.chars(),
            lookahead: collections::VecDeque::new(),
            lexeme: &mut lexeme,
            line: *line,
            column: 1,
        };

        match consume_line(&mut state, &mut tokens) {
            Some(err) => return Err(err),
            None => {
                if tokens.len() > 1
                    && tokens[tokens.len() - 1].kind == TokenType::HeaderFieldTerm
                    && tokens[tokens.len() - 2].kind == TokenType::HeaderFieldTerm
                {
                    *line = state.line;
                    return Ok(tokens);
                }
            }
        }
        *line = state.line;
    }
    Ok(tokens)
}

fn read_line(buf: &mut impl BufRead, msg: &mut String) -> Option<ErrorCode> {
    if let Err(err) = buf.read_line(msg) {
        crate::error(&err.to_string());
        Some(ErrorCode::IoErr)
    } else {
        None
    }
}

fn consume_line(state: &mut ScanState, tokens: &mut Vec<Token>) -> Option<ErrorCode> {
    while let Some(ch) = advance(state) {
        match scan_token(state, ch) {
            Ok(Some(token)) => {
                tokens.push(token);

                let last = &tokens[tokens.len() - 1];
                if last.kind == TokenType::HeaderFieldTerm {
                    if tokens.len() > 1
                        && tokens[tokens.len() - 2].kind == TokenType::HeaderFieldTerm
                    {
                        return None;
                    }
                }
            }
            Ok(None) => continue,
            Err(err) => return Some(err),
        }
        state.lexeme.clear();
    }
    None
}

fn scan_token(state: &mut ScanState, next: char) -> Result<Option<Token>, ErrorCode> {
    let kind = match next {
        ':' => {
            if r#match(state, ' ') {
                value(state);
                TokenType::HeaderFieldValue
            } else {
                crate::error(&format!(
                    "Unexpected character in line {} column {}.",
                    state.line, state.column
                ));
                return Err(ErrorCode::ProtocolErr);
            }
        }
        '\r' => {
            if r#match(state, '\n') {
                line_break(&mut state.line, &mut state.column);
                TokenType::HeaderFieldTerm
            } else {
                return Ok(None);
            }
        }
        '\n' => {
            line_break(&mut state.line, &mut state.column);
            return Ok(None);
        }
        ' ' | '\t' => return Ok(None),
        ch => {
            if is_name(ch) {
                state.lexeme.push(ch);
                name(state);
                TokenType::HeaderFieldName
            } else {
                crate::error(&format!(
                    "Unexpected character in line {} column {}.",
                    state.line, state.column
                ));
                return Err(ErrorCode::ProtocolErr);
            }
        }
    };

    Ok(Some(Token {
        kind,
        lexeme: state.lexeme.clone(),
        line: state.line,
        column: state.column,
    }))
}

fn name(state: &mut ScanState) {
    while let Some(ch) = peek(state) {
        if !is_name(ch) {
            break;
        }
        let next = advance(state).unwrap();
        state.lexeme.push(next);
    }
}

fn value(state: &mut ScanState) {
    while let Some(ch) = peek(state) {
        if !is_ascii_printable(ch) {
            break;
        }
        let next = advance(state).unwrap();
        state.lexeme.push(next);
    }
}

fn is_alpha(ch: char) -> bool {
    (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || ch == '_'
}

fn is_ascii_printable(ch: char) -> bool {
    ch >= ' ' && ch <= '~'
}

fn is_token_special_char(ch: char) -> bool {
    let special = [
        '!', '#', '$', '%', '&', '\'', '*', '+', '-', '.', '^', '_', '`', '|', '~',
    ];
    special.contains(&ch)
}

fn is_name(ch: char) -> bool {
    is_alpha(ch) || ch.is_digit(10) || is_token_special_char(ch)
}

fn advance(state: &mut ScanState) -> Option<char> {
    state.column += 1;
    if !state.lookahead.is_empty() {
        let front = state.lookahead.pop_front();
        front
    } else {
        state.source.next()
    }
}

fn peek(state: &mut ScanState) -> Option<char> {
    if state.lookahead.is_empty() {
        let next = state.source.next();
        match next {
            Some(ch) => state.lookahead.push_back(ch),
            None => return None,
        }
        next
    } else {
        let next = *state.lookahead.front().unwrap();
        Some(next)
    }
}

fn r#match(state: &mut ScanState, expected: char) -> bool {
    let next = match state.source.next() {
        Some(ch) => ch,
        None => return false,
    };

    let equal = next == expected;
    if !equal {
        state.lookahead.push_back(next)
    }
    equal
}

fn line_break(line: &mut u32, column: &mut u32) {
    *line += 1;
    *column = 1;
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn scans_full_header() {
        let header =
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 100\r\n\r\n";

        let mut line = 1;
        let mut input = io::BufReader::new(header.as_bytes());

        let tokens = scan(&mut input, &mut line).expect("Scanning should be sucessful");

        assert_eq!(line, 4);
        assert_eq!(tokens.len(), 7);

        assert_eq!(tokens[0].kind, TokenType::HeaderFieldName);
        assert_eq!(tokens[0].lexeme, "Content-Type");

        assert_eq!(tokens[1].kind, TokenType::HeaderFieldValue);
        assert_eq!(
            tokens[1].lexeme,
            "application/vscode-jsonrpc; charset=utf-8"
        );

        assert_eq!(tokens[2].kind, TokenType::HeaderFieldTerm);

        assert_eq!(tokens[3].kind, TokenType::HeaderFieldName);
        assert_eq!(tokens[3].lexeme, "Content-Length");

        assert_eq!(tokens[4].kind, TokenType::HeaderFieldValue);
        assert_eq!(tokens[4].lexeme, "100");

        assert_eq!(tokens[5].kind, TokenType::HeaderFieldTerm);

        assert_eq!(tokens[6].kind, TokenType::HeaderFieldTerm);
    }

    #[test]
    fn scans_min_header() {
        let header = "Content-Length: 5438\r\n\r\n";

        let mut line = 1;
        let mut input = io::BufReader::new(header.as_bytes());

        let tokens = scan(&mut input, &mut line).expect("Scanning should be sucessful");
        assert_eq!(line, 3);
        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].kind, TokenType::HeaderFieldName);
        assert_eq!(tokens[0].lexeme, "Content-Length");

        assert_eq!(tokens[1].kind, TokenType::HeaderFieldValue);
        assert_eq!(tokens[1].lexeme, "5438");

        assert_eq!(tokens[2].kind, TokenType::HeaderFieldTerm);

        assert_eq!(tokens[3].kind, TokenType::HeaderFieldTerm);
    }
}
