// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::{BufRead, Write};

use crate::{
    config::{ChannelKind, Config},
    lsp::{self, ParseState, Token},
    protocol::{ErrorCodes, NumberOrString, ResponseError, ResponseMessage},
    request::Request,
};

pub enum Channel<'a, R: BufRead, W: Write, E: Write> {
    StdioChannel {
        cin: R,
        cout: &'a mut W,
        cerr: &'a mut E,
        decoder: Decoder,
    },
}

struct Decoder {
    state: ParseState,
    rest: Vec<u8>,
    tokens: Vec<Token>,
}

impl<'a, R: BufRead, W: Write, E: Write> Channel<'a, R, W, E> {
    pub fn read_msg(&mut self) -> Result<Option<Request>, ResponseError> {
        let len = self.read_all()?;
        if len <= 0 {
            return Ok(None);
        }
        Ok(self.decode_stream()?)
    }

    pub fn write_response_msg(&mut self) {}

    pub fn write_response_error(&mut self, id: Option<NumberOrString>, error: ResponseError) {
        let msg = ResponseMessage {
            jsonrpc: "2.0".to_string(),
            id,
            error: Some(error),
            result: None,
        };

        self.write_all(
            serde_json::ser::to_string(&msg)
                .expect("Error serialization must not fail.")
                .as_bytes(),
        );
    }

    fn read_all(&mut self) -> Result<usize, ResponseError> {
        match self {
            Channel::StdioChannel { cin, decoder, .. } => {
                match cin.read_to_end(&mut decoder.rest) {
                    Ok(len) => Ok(len),
                    Err(err) => Err(ResponseError {
                        code: ErrorCodes::ParseError as i64,
                        message: err.to_string(),
                        data: None,
                    }),
                }
            }
        }
    }

    fn decode_stream(&mut self) -> Result<Option<Request>, ResponseError> {
        let decoder: &mut Decoder = match self {
            Channel::StdioChannel { decoder, .. } => decoder,
        };
        lsp::parse(&mut decoder.state, &mut decoder.rest, &mut decoder.tokens)
    }

    fn write_all(&mut self, buf: &[u8]) {
        match self {
            Channel::StdioChannel { cout, cerr, .. } => {
                match cout.write_all(buf) {
                    Ok(_) => (),
                    Err(err) => {
                        // Stop trying if error reporting to fallback channel fails.
                        let _ = cerr.write_all(err.to_string().as_bytes());
                    }
                }
            }
        };
    }
}

pub fn build_channel<'a, R, W, E>(cfg: Config<'a, R, W, E>) -> Channel<'a, R, W, E>
where
    R: BufRead,
    W: Write,
    E: Write,
{
    if cfg.channel == ChannelKind::Stdio {
        Channel::StdioChannel {
            cin: cfg.stdin,
            cout: cfg.stdout,
            cerr: cfg.stderr,
            decoder: Decoder {
                state: ParseState::Syncing,
                rest: Vec::new(),
                tokens: Vec::new(),
            },
        }
    } else if cfg.channel == ChannelKind::Pipe {
        unreachable!()
    } else {
        assert_eq!(cfg.channel, ChannelKind::Socket);
        unreachable!()
    }
}
