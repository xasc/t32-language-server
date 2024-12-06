// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::BufRead;
use std::{fmt, str};

use serde_json::Value;

use crate::protocol::{ErrorCodes, ResponseError};

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

pub fn parse(buf: &mut impl BufRead) -> Option<ResponseError> {
    let mut line = 1;

    let len = match parse_header(buf, &mut line) {
        Ok(len) => len,
        Err(err) => return Some(err),
    };

    let mut content = vec![0u8; len as usize];
    if let Err(err) = buf.read_exact(&mut content) {
        return Some(ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: err.to_string(),
            data: None,
        });
    };

    let repr = match str::from_utf8(&content) {
        Ok(str) => str,
        Err(err) => {
            return Some(ResponseError {
                code: ErrorCodes::ParseError as i64,
                message: err.to_string(),
                data: None,
            })
        }
    };

    let json: Value = match serde_json::from_str(repr) {
        Ok(v) => v,
        Err(err) => {
            return Some(ResponseError {
                code: ErrorCodes::ParseError as i64,
                message: err.to_string(),
                data: None,
            })
        }
    };

    None
}

fn parse_header(buf: &mut impl BufRead, line: &mut u32) -> Result<u32, ResponseError> {
    let tokens = header::scan(buf, line)?;
    if tokens.len() < 5 {
        min_header(&tokens)
    } else {
        full_header(&tokens)
    }
}

fn min_header(tokens: &[Token]) -> Result<u32, ResponseError> {
    if tokens.len() != 4
        || !(tokens[0].kind == TokenType::HeaderFieldName
            && tokens[1].kind == TokenType::HeaderFieldValue
            && tokens[2].kind == TokenType::HeaderFieldTerm
            && tokens[3].kind == TokenType::HeaderFieldTerm)
    {
        return Err(ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from("Header format is invalid."),
            data: None,
        });
    }

    let value = parse_header_field(&tokens[0].lexeme, &tokens[1].lexeme)?;
    if let HeaderValue::ContentType(_) = value {
        return Err(ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from("Header \"Content-Type\" is not allowed here."),
            data: None,
        });
    }

    if let HeaderValue::ContentLength(len) = value {
        return Ok(len);
    }
    unreachable!()
}

fn full_header(tokens: &[Token]) -> Result<u32, ResponseError> {
    if tokens.len() != 7
        || !(tokens[0].kind == TokenType::HeaderFieldName
            && tokens[1].kind == TokenType::HeaderFieldValue
            && tokens[2].kind == TokenType::HeaderFieldTerm
            && tokens[3].kind == TokenType::HeaderFieldName
            && tokens[4].kind == TokenType::HeaderFieldValue
            && tokens[5].kind == TokenType::HeaderFieldTerm
            && tokens[6].kind == TokenType::HeaderFieldTerm)
    {
        return Err(ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from("Header format is invalid."),
            data: None,
        });
    }

    let mut headers = Vec::<HeaderValue>::new();

    let value = parse_header_field(&tokens[0].lexeme, &tokens[1].lexeme)?;
    headers.push(value);

    let value = parse_header_field(&tokens[3].lexeme, &tokens[4].lexeme)?;
    headers.push(value);

    match &headers[0] {
        HeaderValue::ContentType(r#type) => {
            if let HeaderValue::ContentType(_) = headers[1] {
                return Err(ResponseError {
                    code: ErrorCodes::ParseError as i64,
                    message: String::from("Header \"Content-Type\" is not allowed here."),
                    data: None,
                });
            }
            if r#type != "application/vscode-jsonrpc; charset=utf-8" {
                return Err(ResponseError {
                    code: ErrorCodes::ParseError as i64,
                    message: String::from("Header \"Content-Type\" has invalid value."),
                    data: None,
                });
            }
            if let HeaderValue::ContentLength(len) = headers[1] {
                return Ok(len);
            }
            unreachable!();
        }
        HeaderValue::ContentLength(len) => {
            if let HeaderValue::ContentLength(_) = headers[1] {
                return Err(ResponseError {
                    code: ErrorCodes::ParseError as i64,
                    message: String::from("Header \"Content-Length\" is not allowed here."),
                    data: None,
                });
            }
            if let HeaderValue::ContentType(r#type) = &headers[1] {
                if r#type != "application/vscode-jsonrpc; charset=utf-8" {
                    return Err(ResponseError {
                        code: ErrorCodes::ParseError as i64,
                        message: String::from("Header \"Content-Type\" has invalid value."),
                        data: None,
                    });
                }
            }
            return Ok(*len);
        }
    }
}

fn parse_header_field(name: &str, value: &str) -> Result<HeaderValue, ResponseError> {
    match name {
        "Content-Type" => Ok(HeaderValue::ContentType(value.parse::<String>().unwrap())),
        "Content-Length" => {
            let val = value.parse::<u32>();
            if let Err(_) = val {
                return Err(ResponseError {
                    code: ErrorCodes::ParseError as i64,
                    message: String::from("Invalid header format."),
                    data: None,
                });
            }
            Ok(HeaderValue::ContentLength(val.unwrap()))
        }
        _ => Err(ResponseError {
            code: ErrorCodes::ParseError as i64,
            message: String::from("Invalid header detected."),
            data: None,
        }),
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
