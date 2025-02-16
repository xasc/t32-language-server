// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # LSP Protocol Notes
//!
//! - HTTP header fields are normally preceded by a CRLF sequence. However, the LSP
//! base protocol does not specify how the first header field of a message shall
//! be delimited from the previous one. Hence, we need to handle sequences like
//! "`abcContent-Length: 100`" where one message passes directly into the first
//! header field of the next one.
//! - The whitespace after the colon delimter between HTTP header field name and
//! value is normally optional. The LSP protocol makes it mandatory.
//! - The string representation is UTF-8.
//! - Text ranges (line and character offset) exclude the end position. To select the
//! last character in a line, the first character of the next line should be used as
//! the end position. If the character offset is longer than the line length it should
//! be reverted back to the line length. How do we then select the last character in a text?

mod header;
mod jsonrpc;

use std::{fmt, num::NonZeroUsize};

use crate::{
    ls::request::{Notification, Request},
    ls::response::{ErrorResponse, Response},
};

use header::ScanError;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TokenType {
    HeaderFieldTerm,
    HeaderFieldName,
    HeaderFieldValue,
    EndOfHeaders,
}

#[derive(Debug, PartialEq)]
pub enum ParseState {
    Syncing,
    InHeader,
    InContent(NonZeroUsize),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenType,
    pub lexeme: String,
    pub start: usize,
    pub end: usize,
    pub fusible: bool,
}

#[derive(Debug)]
pub enum Message {
    Request(Request),
    Notification(Notification),
    Response(Response),
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:?}[{}]",
            self.kind,
            if self.lexeme.is_empty() {
                "empty"
            } else {
                &self.lexeme
            }
        )
    }
}

impl Message {
    pub fn is_request(&self) -> bool {
        match self {
            Message::Request(_) => true,
            _ => false,
        }
    }

    pub fn is_notification(&self) -> bool {
        match self {
            Message::Notification(_) => true,
            _ => false,
        }
    }

    pub fn get_request(&self) -> &Request {
        assert!(self.is_request());
        if let Message::Request(req) = self {
            req
        } else {
            panic!("Must only be called for Request.")
        }
    }

    pub fn get_notification(&self) -> &Notification {
        assert!(self.is_notification());
        if let Message::Notification(notif) = self {
            notif
        } else {
            panic!("Must only be called for Request.")
        }
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn parse(
    state: &mut ParseState,
    buf: &mut Vec<u8>,
    tokens: &mut Vec<Token>,
) -> Result<Option<Message>, ErrorResponse> {
    if *state == ParseState::Syncing {
        assert_eq!(tokens.len(), 0);

        if let Some(offset) = guess_msg_start(buf) {
            buf.drain(0..offset);
            *state = ParseState::InHeader;
        } else {
            buf.clear();
            return Ok(None);
        }
    }

    if *state == ParseState::InHeader {
        let content_len = parse_header(buf, tokens)?;
        if let Some(len) = content_len {
            *state = ParseState::InContent(len);
            tokens.clear();
        } else {
            debug_assert!(buf.len() <= 0);
            return Ok(None);
        }
    }

    if let ParseState::InContent(len) = *state {
        if buf.len() < len.into() {
            Ok(None)
        } else {
            let msg = parse_content(&buf[..len.into()]);

            let end: usize = len.into();
            buf.drain(..end);
            *state = ParseState::Syncing;

            match msg {
                Ok(msg) => Ok(Some(msg)),
                Err(err) => Err(err),
            }
        }
    } else {
        Ok(None)
    }
}

pub fn make_response(msg: Message) -> Vec<u8> {
    let content = jsonrpc::serialize_msg(msg);
    let header = header::make_header(
        NonZeroUsize::new(content.len()).expect("Response messages must have content section."),
    );

    format!("{}{}", header, content).as_bytes().to_vec()
}

fn parse_header(
    buf: &mut Vec<u8>,
    hist: &mut Vec<Token>,
) -> Result<Option<NonZeroUsize>, ErrorResponse> {
    // Next token might end the header section
    if hist.len() > 0 && hist[hist.len() - 1].kind == TokenType::HeaderFieldTerm {
        match header::scan(buf, true) {
            Ok(mut token) => {
                buf.drain(..=token[0].end);
                if token[0].kind == TokenType::HeaderFieldTerm {
                    token[0].kind = TokenType::EndOfHeaders;
                }
                hist.append(&mut token)
            }
            Err(ScanError(offset)) => {
                buf.drain(..=offset);
                hist.clear();
            }
        }
    }

    while buf.len() > 0 {
        let mut scanned = match header::scan(buf, false) {
            Ok(tokens) => {
                if tokens.len() > 0 {
                    buf.drain(..=tokens[tokens.len() - 1].end);
                } else {
                    buf.clear();
                    continue;
                }
                tokens
            }
            Err(ScanError(offset)) => {
                buf.drain(..=offset);
                hist.clear();
                continue;
            }
        };

        // We tolerate data sequences that are either so short or so repetitive that
        // the scanner cannot detect any token. The token history remains valid.
        if scanned.len() > 0 {
            if hist.len() > 0 {
                header::assemble_sequence(hist, &mut scanned);
            } else {
                hist.append(&mut scanned);
            }
            debug_assert_eq!(scanned.len(), 0);
        } else if hist.len() <= 0 {
            continue;
        }

        // Scanner stops as soon as the end of the header section is detected. No
        // further characters should be scanned.
        if header_section_ends(hist) {
            break;
        }
    }

    if hist.len() <= 0 || !header_section_ends(hist) {
        Ok(None)
    } else {
        // Try to recover from errors by parsing the remaining
        // tokens until they all are used up. Should we detect
        // all valid header structure we will accept it.
        match header::validate_headers(hist) {
            Ok(len) => Ok(Some(len)),
            Err(err) => {
                while hist.len() > 0 {
                    if let Ok(len) = header::validate_headers(hist) {
                        return Ok(Some(len));
                    }
                }
                Err(ErrorResponse {
                    id: None,
                    error: err,
                })
            }
        }
    }
}

fn parse_content(buf: &[u8]) -> Result<Message, ErrorResponse> {
    let msg = jsonrpc::parse_message(buf).map_err(|error| ErrorResponse { id: None, error })?;
    Ok(jsonrpc::deserialize_msg(msg)?)
}

/// Guess the start of the next message by looking for the start of the next
/// header. Valid header names start with the letter "C". Header field names
/// are case insensitive.
///
fn guess_msg_start(buf: &mut [u8]) -> Option<usize> {
    buf.iter()
        .position(|&c| (c as char).to_ascii_lowercase() == 'c')
}

fn header_section_ends(tokens: &[Token]) -> bool {
    tokens[tokens.len() - 1].kind == TokenType::EndOfHeaders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_header() {
        let header = "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 100\r\n\r\n";

        let mut state = ParseState::Syncing;
        let mut tokens = Vec::<Token>::new();
        let rc = parse(&mut state, &mut header.as_bytes().to_vec(), &mut tokens)
            .expect("Should not fail.");

        assert!(rc.is_none());
        assert_eq!(
            state,
            ParseState::InContent(NonZeroUsize::new(100).unwrap())
        );
    }

    #[test]
    fn parses_all_lowercase_header() {
        let header =
            "content-type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-length: 99\r\n\r\n";

        let mut state = ParseState::Syncing;
        let mut tokens = Vec::<Token>::new();
        let rc = parse(&mut state, &mut header.as_bytes().to_vec(), &mut tokens)
            .expect("Should not fail.");

        assert!(rc.is_none());
        assert_eq!(state, ParseState::InContent(NonZeroUsize::new(99).unwrap()));
    }

    #[test]
    fn ignores_garbage_before_header() {
        let header = "abcContent-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 1\r\n\r\n";

        let mut state = ParseState::Syncing;
        let mut tokens = Vec::<Token>::new();
        let rc = parse(&mut state, &mut header.as_bytes().to_vec(), &mut tokens)
            .expect("Should not fail.");

        assert!(rc.is_none());
        assert_eq!(state, ParseState::InContent(NonZeroUsize::new(1).unwrap()));
    }

    #[test]
    fn parses_msg_content() {
        let content = r#"{ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }"#;

        let mut state = ParseState::InContent(NonZeroUsize::new(content.len()).unwrap());
        let mut tokens = Vec::<Token>::new();

        let rc = parse(&mut state, &mut content.as_bytes().to_vec(), &mut tokens)
            .expect("Should not fail.");

        assert!(rc.is_some());
        assert_eq!(state, ParseState::Syncing);
    }

    #[test]
    fn waits_for_content_len() {
        let content = r#"{ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }"#;

        let mut state = ParseState::InContent(NonZeroUsize::new(content.len()).unwrap());
        let mut tokens = Vec::<Token>::new();

        let rc = parse(
            &mut state,
            &mut content.as_bytes()[..(content.len() - 1)].to_vec(),
            &mut tokens,
        )
        .expect("Should not fail.");

        assert!(rc.is_none());
        assert_eq!(
            state,
            ParseState::InContent(NonZeroUsize::new(content.len()).unwrap())
        );
    }

    #[test]
    fn parses_msg() {
        let content = r#"{ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }"#;
        let header = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
            content.len()
        );

        let msg = format!("{}{}", header, content);

        let mut state = ParseState::Syncing;
        let mut tokens = Vec::<Token>::new();

        let rc =
            parse(&mut state, &mut msg.as_bytes().to_vec(), &mut tokens).expect("Should not fail.");

        assert!(rc.is_some());
        assert_eq!(state, ParseState::Syncing);

        let content = r#"{ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }"#;
        let header = format!("abcdefContent-Length: {}\r\n\r\n", content.len());

        let msg = format!("{}{}", header, content);

        let mut state = ParseState::Syncing;
        let mut tokens = Vec::<Token>::new();

        let rc =
            parse(&mut state, &mut msg.as_bytes().to_vec(), &mut tokens).expect("Should not fail.");

        assert!(rc.is_some());
        assert_eq!(state, ParseState::Syncing);
    }
}
