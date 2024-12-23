// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::{BufRead, Write};

use crate::{
    config::{ChannelKind, Config},
    parser::{self, ParseState, Token},
    protocol::{ErrorCodes, RequestMessage, ResponseError},
};

pub enum Channel<'a, R: BufRead, W: Write> {
    StdioChannel {
        cin: R,
        cout: &'a mut W,
        decoder: Decoder,
    },
}

struct Decoder {
    state: ParseState,
    rest: Vec<u8>,
    tokens: Vec<Token>,
}

impl<'a, R: BufRead, W: Write> Channel<'a, R, W> {
    pub fn read_msg(&mut self) -> Result<Option<RequestMessage>, ResponseError> {
        let len = self.read_all()?;
        if len <= 0 {
            return Ok(None);
        }
        self.decode_stream()
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

    fn decode_stream(&mut self) -> Result<Option<RequestMessage>, ResponseError> {
        let decoder: &mut Decoder = match self {
            Channel::StdioChannel { decoder, .. } => decoder,
        };
        parser::parse(&mut decoder.state, &mut decoder.rest, &mut decoder.tokens)
    }
}

pub fn build_channel<R: BufRead, W: Write>(cfg: Config<R>, outs: &mut W) -> Channel<R, W> {
    if cfg.channel == ChannelKind::Stdio {
        Channel::StdioChannel {
            cin: cfg.stdin,
            cout: outs,
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
