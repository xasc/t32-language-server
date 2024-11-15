// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;
use std::io::BufRead;

use crate::ErrorCode;

mod header;
mod scanner;

#[derive(Debug, PartialEq)]
pub enum TokenType {
    HeaderFieldTerm,
    HeaderFieldName,
    HeaderFieldValue,
}

#[derive(Debug)]
pub struct Token {
    pub kind: TokenType,
    pub lexeme: String,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} {}", self.kind, self.lexeme)
    }
}

#[derive(Debug)]
enum HeaderValue {
    ContentType(String),
    ContentLength(u32),
}

pub fn parse(buf: &mut impl BufRead) -> Option<ErrorCode> {
    let mut line = 1;

    let _ = parse_header(buf, &mut line);

    None
}

fn parse_header(buf: &mut impl BufRead, line: &mut u32) -> Result<u32, ErrorCode> {
    let tokens = header::scan(buf, line)?;

    if tokens.len() < 5 {
        min_header(&tokens)
    } else {
        full_header(&tokens)
    }
}

fn min_header(tokens: &[Token]) -> Result<u32, ErrorCode> {
    if tokens.len() != 4
        || !(tokens[0].kind == TokenType::HeaderFieldName
            && tokens[1].kind == TokenType::HeaderFieldValue
            && tokens[2].kind == TokenType::HeaderFieldTerm
            && tokens[3].kind == TokenType::HeaderFieldTerm)
    {
        return Err(ErrorCode::ProtocolErr);
    }

    let value = parse_header_field(&tokens[0].lexeme, &tokens[1].lexeme)?;
    if let HeaderValue::ContentType(_) = value {
        return Err(ErrorCode::ProtocolErr);
    }

    if let HeaderValue::ContentLength(len) = value {
        return Ok(len);
    }
    unreachable!()
}

fn full_header(tokens: &[Token]) -> Result<u32, ErrorCode> {
    if tokens.len() != 7
        || !(tokens[0].kind == TokenType::HeaderFieldName
            && tokens[1].kind == TokenType::HeaderFieldValue
            && tokens[2].kind == TokenType::HeaderFieldTerm
            && tokens[3].kind == TokenType::HeaderFieldName
            && tokens[4].kind == TokenType::HeaderFieldValue
            && tokens[5].kind == TokenType::HeaderFieldTerm
            && tokens[6].kind == TokenType::HeaderFieldTerm)
    {
        return Err(ErrorCode::ProtocolErr);
    }

    let mut headers = Vec::<HeaderValue>::new();

    let value = parse_header_field(&tokens[0].lexeme, &tokens[1].lexeme)?;
    headers.push(value);

    let value = parse_header_field(&tokens[3].lexeme, &tokens[4].lexeme)?;
    headers.push(value);

    match &headers[0] {
        HeaderValue::ContentType(r#type) => {
            if let HeaderValue::ContentType(_) = headers[1] {
                return Err(ErrorCode::ProtocolErr);
            }
            if r#type != "application/vscode-jsonrpc; charset=utf-8" {
                return Err(ErrorCode::ProtocolErr);
            }
            if let HeaderValue::ContentLength(len) = headers[1] {
                return Ok(len);
            }
            unreachable!();
        }
        HeaderValue::ContentLength(len) => {
            if let HeaderValue::ContentLength(_) = headers[1] {
                return Err(ErrorCode::ProtocolErr);
            }
            if let HeaderValue::ContentType(r#type) = &headers[1] {
                if r#type != "application/vscode-jsonrpc; charset=utf-8" {
                    return Err(ErrorCode::ProtocolErr);
                }
            }
            return Ok(*len);
        }
    }
}

fn parse_header_field(name: &str, value: &str) -> Result<HeaderValue, ErrorCode> {
    match name {
        "Content-Type" => Ok(HeaderValue::ContentType(value.parse::<String>().unwrap())),
        "Content-Length" => {
            let val = value.parse::<u32>();
            if let Err(_) = val {
                return Err(ErrorCode::ProtocolErr);
            }
            Ok(HeaderValue::ContentLength(val.unwrap()))
        }
        _ => Err(ErrorCode::ProtocolErr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_full_header() {
        let tokens = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Type".to_string(),
                line: 1,
                column: 13,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "application/vscode-jsonrpc; charset=utf-8".to_string(),
                line: 1,
                column: 55,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 2,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                line: 2,
                column: 15,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "724".to_string(),
                line: 2,
                column: 19,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 3,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 4,
                column: 1,
            },
        ];

        let len = full_header(&tokens);
        assert_eq!(len.unwrap(), 724);
    }

    #[test]
    fn valid_min_header() {
        let tokens = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                line: 1,
                column: 15,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "5340".to_string(),
                line: 1,
                column: 19,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 2,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 3,
                column: 1,
            },
        ];

        let len = min_header(&tokens);
        assert_eq!(len.unwrap(), 5340);
    }

    #[test]
    fn header_missing_content_length() {
        let tokens = [
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Type".to_string(),
                line: 1,
                column: 13,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "application/vscode-jsonrpc; charset=utf-8".to_string(),
                line: 1,
                column: 55,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 2,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Leng".to_string(),
                line: 2,
                column: 15,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "724".to_string(),
                line: 2,
                column: 19,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 3,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 4,
                column: 1,
            },
        ];

        let len = full_header(&tokens);
        assert!(len.is_err());

        let tokens = vec![
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 2,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 3,
                column: 1,
            },
        ];

        let len = min_header(&tokens);
        assert!(len.is_err());
    }

    #[test]
    fn header_missing_termination() {
        let tokens = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Type".to_string(),
                line: 1,
                column: 13,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "application/vscode-jsonrpc; charset=utf-8".to_string(),
                line: 1,
                column: 55,
            },
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                line: 2,
                column: 15,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "724".to_string(),
                line: 2,
                column: 19,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 3,
                column: 1,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 4,
                column: 1,
            },
        ];
        let len = full_header(&tokens);
        assert!(len.is_err());

        let tokens = vec![
            Token {
                kind: TokenType::HeaderFieldName,
                lexeme: "Content-Length".to_string(),
                line: 1,
                column: 15,
            },
            Token {
                kind: TokenType::HeaderFieldValue,
                lexeme: "5340".to_string(),
                line: 1,
                column: 19,
            },
            Token {
                kind: TokenType::HeaderFieldTerm,
                lexeme: "".to_string(),
                line: 2,
                column: 1,
            },
        ];

        let len = min_header(&tokens);
        assert!(len.is_err());
    }
}
