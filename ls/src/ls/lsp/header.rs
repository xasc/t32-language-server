// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    fmt,
    num::{NonZero, NonZeroUsize},
};

use crate::{
    ls::lsp::{Token, TokenType},
    protocol::{ErrorCodes, ResponseError},
};

struct ScanState<'a> {
    next: usize,
    source: &'a [u8],
}

#[derive(Debug, PartialEq)]
enum HeaderValue {
    ContentType,
    ContentLength(NonZeroUsize),
}

#[derive(Debug, PartialEq)]
pub struct ScanError(pub usize);

const CONTENT_LENGTH_NAME: &str = "Content-Length";
const CONTENT_TYPE_NAME: &str = "Content-Type";
const CONTENT_TYPE_VALUE: &str = "application/vscode-jsonrpc; charset=utf-8";

impl fmt::Display for HeaderValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HeaderValue::ContentType => write!(f, "{}", CONTENT_TYPE_NAME),
            HeaderValue::ContentLength(_) => write!(f, "{}", CONTENT_LENGTH_NAME),
        }
    }
}

/// Scanning tolerates superfluous whitespaces between header name, colon and
/// header field.
pub fn scan(buf: &[u8], one_token: bool) -> Result<Vec<Token>, ScanError> {
    // TODO: Avoid frequent memory allocations for tokens in function.
    let mut tokens = Vec::<Token>::new();

    let mut state = ScanState {
        next: 0,
        source: buf,
    };

    while state.next < state.source.len() {
        match scan_token(&mut state) {
            Ok(Some(token)) => tokens.push(token),
            Ok(None) => continue,
            Err(err) => return Err(err),
        }

        if one_token && tokens.len() >= 1 {
            break;
        }

        if header_section_ends(&tokens) {
            let len = tokens.len();
            tokens[len - 1].kind = TokenType::EndOfHeaders;
            break;
        }
    }
    Ok(tokens)
}

pub fn assemble_sequence(prior: &mut Vec<Token>, post: &mut Vec<Token>) {
    debug_assert!(prior.len() > 0 && post.len() > 0);

    let last = prior.len() - 1;
    if prior[last].fusible
        && post[0].start == 0
        && prior[last].kind == post[0].kind
        && (post[0].kind == TokenType::HeaderFieldName
            || post[0].kind == TokenType::HeaderFieldValue)
    {
        prior[last].lexeme.push_str(&post[0].lexeme);

        post.remove(0);
    }
    prior.append(post);
}

/// Grammar for the header section:
///
///   headers        →  header_fields eoh
///   header_fields  →  content_length content_type? | content_type content_length
///
///   content_length →  "Content-Length" number "\r\n"
///   content_type   →  "Content-Type" "application/vscode-jsonrpc; charset=utf-8" "\r\n"
///
pub fn validate_headers(tokens: &mut Vec<Token>) -> Result<NonZeroUsize, ResponseError> {
    let mut content_len: NonZeroUsize = NonZero::new(usize::MAX).unwrap();
    match header_field(tokens) {
        Ok(HeaderValue::ContentLength(len)) => content_len = len,
        Err(err) => {
            try_recover(tokens, 1);
            return Err(err);
        }
        _ => (),
    }

    let mut offset = 3;
    if content_len >= NonZero::new(usize::MAX).unwrap() {
        content_len = match header_field(&tokens[3..]) {
            Ok(HeaderValue::ContentLength(len)) => len,
            Ok(HeaderValue::ContentType) => {
                try_recover(tokens, 4);
                return Err(error_wrong_header_type(
                    HeaderValue::ContentType,
                    HeaderValue::ContentLength(content_len),
                ));
            }
            Err(err) => {
                try_recover(tokens, 4);
                return Err(err);
            }
        };
        offset += 3
    } else if let Ok(HeaderValue::ContentType) = header_field(&tokens[3..]) {
        offset += 3;
    }

    if tokens.len() <= offset {
        try_recover(tokens, offset + 1);
        Err(error_missing_header_token(TokenType::EndOfHeaders))
    } else if !end_of_headers(&tokens[offset]) {
        try_recover(tokens, offset + 1);
        Err(error_wrong_header_type_token(
            tokens[offset].kind,
            TokenType::EndOfHeaders,
        ))
    } else {
        Ok(content_len)
    }
}

pub fn make_header(content_len: NonZeroUsize) -> String {
    format!(
        "{}: {}\r\n{}: {}\r\n\r\n",
        CONTENT_TYPE_NAME, CONTENT_TYPE_VALUE, CONTENT_LENGTH_NAME, content_len
    )
}

fn scan_token(state: &mut ScanState) -> Result<Option<Token>, ScanError> {
    let next = advance(state).expect("Function should only be called if there is more data.");
    let token = match next {
        ':' => {
            if r#match(state, ' ') {
                advance(state).expect("Cannot fail due to prior check.");

                let ch = match peek(state) {
                    Some(ch) => ch,
                    None => return Ok(None),
                };

                // Values of header fields must not start with a whitespace
                if is_ascii_printable(ch) {
                    value(state)
                } else {
                    return Err(ScanError(state.next));
                }
            } else {
                return Err(ScanError(state.next));
            }
        }
        '\r' => {
            if r#match(state, '\n') {
                let tok = term(state);
                advance(state);
                tok
            } else {
                return Ok(None);
            }
        }
        '\n' => {
            return Err(ScanError(state.next - 1));
        }
        ' ' | '\t' => return Ok(None),
        ch => {
            if is_name(ch) {
                name(state)
            } else {
                return Err(ScanError(state.next - 1));
            }
        }
    };

    Ok(Some(token))
}

fn r#match(state: &mut ScanState, expected: char) -> bool {
    if state.next >= state.source.len() {
        return false;
    }
    let ch = state.source[state.next] as char;
    ch == expected
}

fn advance(state: &mut ScanState) -> Option<char> {
    if state.next >= state.source.len() {
        None
    } else {
        let char = state.source[state.next] as char;
        state.next += 1;

        Some(char)
    }
}

fn peek(state: &mut ScanState) -> Option<char> {
    if state.next >= state.source.len() {
        None
    } else {
        Some(state.source[state.next] as char)
    }
}

fn is_at_end(state: &ScanState) -> bool {
    state.next >= state.source.len()
}

/// Header field names are case-insensitive. ASCII visual characters are permitted
/// with the exception of double quotes and "(),/:;<=>?@[\]{}".
fn name(state: &mut ScanState) -> Token {
    debug_assert!(state.next > 0);
    let start = state.next - 1;
    while let Some(ch) = peek(state) {
        if !is_name(ch) {
            break;
        }
        state.next += 1;
    }
    let end = state.next;

    Token {
        kind: TokenType::HeaderFieldName,
        lexeme: String::from_utf8_lossy(&state.source[start..end]).to_string(),
        fusible: is_at_end(state),
        start,
        end,
    }
}

/// Header field values support ASCII visual characters separated by spaces and
/// tabs.
fn value(state: &mut ScanState) -> Token {
    debug_assert!(state.next > 0);
    let start = state.next;

    while let Some(ch) = peek(state) {
        if !(is_ascii_printable(ch) || is_space(ch)) {
            break;
        }
        state.next += 1;
    }
    let till_end = is_at_end(state);

    let end = if till_end { state.next - 1 } else { state.next };

    Token {
        kind: TokenType::HeaderFieldValue,
        lexeme: String::from_utf8_lossy(&state.source[start..end]).to_string(),
        fusible: is_at_end(state),
        start,
        end,
    }
}

fn term(state: &mut ScanState) -> Token {
    let start = state.next - 1;
    let end = state.next;

    Token {
        kind: TokenType::HeaderFieldTerm,
        lexeme: String::from_utf8_lossy(&state.source[start..=end]).to_string(),
        fusible: false,
        start,
        end,
    }
}

fn is_alpha(ch: char) -> bool {
    (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || ch == '_'
}

fn is_ascii_printable(ch: char) -> bool {
    ch >= '!' && ch <= '~'
}

fn is_space(ch: char) -> bool {
    ch == ' ' || ch == '\t'
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

fn header_field(tokens: &[Token]) -> Result<HeaderValue, ResponseError> {
    const CONTENT_LENGTH: &str = "content-length";
    const CONTENT_TYPE: &str = "content-type";

    if tokens.len() <= 0 {
        return Err(error_missing_header_token(TokenType::HeaderFieldName));
    }
    if tokens[0].kind != TokenType::HeaderFieldName {
        return Err(error_wrong_header_type_token(
            tokens[0].kind,
            TokenType::HeaderFieldName,
        ));
    }

    // Field names may start with characters that are not part of the header
    // field. These characters are remains of the previous message.
    let mut name = tokens[0].lexeme.to_ascii_lowercase();
    if name.ends_with(CONTENT_LENGTH) {
        name = CONTENT_LENGTH.to_string();
    } else if name.ends_with(CONTENT_TYPE) {
        name = CONTENT_TYPE.to_string();
    } else {
        return Err(ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from(format!("Unexpected name \"{}\" for header field.", name)),
            data: None,
        });
    }

    if tokens.len() <= 1 {
        return Err(error_missing_header_token(TokenType::HeaderFieldValue));
    }
    if tokens[1].kind != TokenType::HeaderFieldValue {
        return Err(error_wrong_header_type_token(
            tokens[1].kind,
            TokenType::HeaderFieldValue,
        ));
    }

    let value = tokens[1].lexeme.to_ascii_lowercase();
    let header = match name.as_str() {
        CONTENT_TYPE => {
            let r#type = value.parse::<String>().unwrap();
            if r#type == CONTENT_TYPE_VALUE {
                HeaderValue::ContentType
            } else {
                return Err(error_wrong_header_type_value(&value, "Content-Type"));
            }
        }
        CONTENT_LENGTH => {
            let number = value.parse::<NonZeroUsize>();
            if let Err(_) = number {
                return Err(error_wrong_header_type_value(&value, "Content-Length"));
            }
            HeaderValue::ContentLength(number.unwrap())
        }
        _ => {
            return Err(error_wrong_header_type_name(&value, &name));
        }
    };

    if tokens.len() <= 2 {
        return Err(error_missing_header_token(TokenType::HeaderFieldTerm));
    }
    if tokens[2].kind != TokenType::HeaderFieldTerm {
        return Err(error_wrong_header_type_token(
            tokens[2].kind,
            TokenType::HeaderFieldTerm,
        ));
    }

    Ok(header)
}

fn end_of_headers(token: &Token) -> bool {
    token.kind == TokenType::EndOfHeaders
}

fn header_section_ends(tokens: &[Token]) -> bool {
    let len = tokens.len();
    if len < 2 {
        return false;
    }
    tokens[(len - 2)..len]
        .iter()
        .all(|t| t.kind == TokenType::HeaderFieldTerm)
}
// If we get lost during scanning the best pattern for resynchronization is the
// start of a header field name.
//
fn try_recover(tokens: &mut Vec<Token>, offset: usize) {
    match tokens[offset..]
        .iter()
        .position(|t| t.kind == TokenType::HeaderFieldName)
    {
        Some(idx) => {
            tokens.drain(..=idx);
        }
        None => tokens.clear(),
    }
}

fn error_missing_header_token(token: TokenType) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Missing token in message header. Expected token \"{}\".",
            token
        )),
        data: None,
    }
}

fn error_wrong_header_type_token(token: TokenType, expected: TokenType) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Unexpected token in message header. Expected token \"{}\", but found \"{}\".",
            expected, token
        )),
        data: None,
    }
}

fn error_wrong_header_type_name(name: &str, header: &str) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Unexpected name \"{}\" for message header field \"{}\".",
            name, header
        )),
        data: None,
    }
}

fn error_wrong_header_type_value(value: &str, header: &str) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Unexpected value \"{}\" for message header field \"{}\".",
            value, header
        )),
        data: None,
    }
}

fn error_wrong_header_type(header: HeaderValue, expected: HeaderValue) -> ResponseError {
    ResponseError {
        code: ErrorCodes::ParseError as i64,
        message: String::from(format!(
            "Syntax error: Unexpected message header field. Expected \"{}\", but found \"{}\".",
            expected, header
        )),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_full_header() {
        let header = "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 100\r\n\r\n";

        let tokens = scan(header.as_bytes(), false).expect("Scanning should be sucessful");

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

        assert_eq!(tokens[6].kind, TokenType::EndOfHeaders);
    }

    #[test]
    fn scans_min_header() {
        let header = "Content-Length: 5438\r\n\r\n";

        let tokens = scan(header.as_bytes(), false).expect("Scanning should be sucessful");
        assert_eq!(tokens.len(), 4);

        assert_eq!(tokens[0].kind, TokenType::HeaderFieldName);
        assert_eq!(tokens[0].lexeme, "Content-Length");

        assert_eq!(tokens[1].kind, TokenType::HeaderFieldValue);
        assert_eq!(tokens[1].lexeme, "5438");

        assert_eq!(tokens[2].kind, TokenType::HeaderFieldTerm);

        assert_eq!(tokens[3].kind, TokenType::EndOfHeaders);
    }

    #[test]
    fn scans_single_token() {
        let header = "\r\nContent-Length";

        let tokens = scan(header.as_bytes(), true).expect("Scanning should be sucessful");
        assert_eq!(tokens.len(), 1);

        assert_eq!(tokens[0].kind, TokenType::HeaderFieldTerm);
        assert_eq!(tokens[0].lexeme, "\r\n");
    }

    #[test]
    fn detects_invalid_headers() {
        let header = "abcCon{tent-Length: 100\r\n\r\n";

        let tokens = scan(header.as_bytes(), false);
        assert_eq!(tokens.err(), Some(ScanError("abcCon{".len() - 1)));

        let header = "Content-Length:100\r\n\r\n";

        let tokens = scan(header.as_bytes(), false);
        assert_eq!(tokens.err(), Some(ScanError("Content-Length:1".len() - 1)));

        let header = "Content-Length:  100\r\n\r\n";

        let tokens = scan(header.as_bytes(), false);
        assert_eq!(tokens.err(), Some(ScanError("Content-Length:  ".len() - 1)));

        let header = "Content-\nLength:  100\r\n\r\n";

        let tokens = scan(header.as_bytes(), false);
        assert_eq!(tokens.err(), Some(ScanError("Content-\n".len() - 1)));
    }

    #[test]
    fn validates_full_header() {
        let mut header = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Type".to_string(),
                start: 0,
                end: 12,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "application/vscode-jsonrpc; charset=utf-8".to_string(),
                start: 24,
                end: 64,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "\r\n".to_string(),
                start: 65,
                end: 66,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                start: 67,
                end: 80,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "100".to_string(),
                start: 83,
                end: 85,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "\r\n".to_string(),
                start: 86,
                end: 87,
                fusible: false,
            },
            Token {
                kind: TokenType::EndOfHeaders,
                lexeme: "\r\n".to_string(),
                start: 88,
                end: 89,
                fusible: false,
            },
        ];
        let len = validate_headers(&mut header).expect("Should not fail.");

        assert_eq!(len, NonZeroUsize::new(100).unwrap());
    }

    #[test]
    fn validates_min_header() {
        let mut header = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                start: 0,
                end: 12,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "10".to_string(),
                start: 15,
                end: 16,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "\r\n".to_string(),
                start: 17,
                end: 18,
                fusible: false,
            },
            Token {
                kind: TokenType::EndOfHeaders,
                lexeme: "\r\n".to_string(),
                start: 17,
                end: 18,
                fusible: false,
            },
        ];
        let len = validate_headers(&mut header).expect("Should not fail.");

        assert_eq!(len, NonZeroUsize::new(10).unwrap());
    }

    #[test]
    fn detects_invalid_header() {
        let header = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                start: 0,
                end: 12,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "10".to_string(),
                start: 15,
                end: 16,
                fusible: false,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "\r\n".to_string(),
                start: 17,
                end: 18,
                fusible: false,
            },
            Token {
                kind: TokenType::EndOfHeaders,
                lexeme: "\r\n".to_string(),
                start: 17,
                end: 18,
                fusible: false,
            },
        ];

        let mut invalid = header.clone();
        invalid[0].lexeme = "ContentLength".to_string();

        let len = validate_headers(&mut invalid);
        assert!(len.is_err());

        let err = len.as_ref().err().unwrap();
        assert_eq!(err.code, ErrorCodes::ParseError as i64);

        let mut invalid = header.clone();
        invalid[1].lexeme = "application/vscode-jsonrpc; charset=utf-8".to_string();

        let len = validate_headers(&mut invalid);
        assert!(len.is_err());

        let err = len.as_ref().err().unwrap();
        assert_eq!(err.code, ErrorCodes::ParseError as i64);
    }
}
