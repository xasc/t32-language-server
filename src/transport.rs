// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    cmp::min,
    io::{BufRead, Write},
};

use crate::{
    config::{ChannelKind, Config},
    lsp::{self, ParseState, Token},
    protocol::{ErrorCodes, NumberOrString, ResponseError},
    request::Request,
    response::ResponseResult,
    Stdio,
};

pub enum Channel<'a, R: BufRead, W: Write, E: Write> {
    StdioChannel {
        cin: R,
        cout: &'a mut W,
        cerr: &'a mut E,
        decoder: Decoder,
    },
}

pub struct Decoder {
    state: ParseState,
    rest: Vec<u8>,
    tokens: Vec<Token>,
}

impl Decoder {
    const CAPACITY: usize = 4096;
}

impl<'a, R: BufRead, W: Write, E: Write> Channel<'a, R, W, E> {
    pub fn read_msg(&mut self) -> Result<Option<Request>, ResponseError> {
        let len = self.read_stream()?;
        if len <= 0 {
            return Ok(None);
        }
        Ok(self.decode_stream()?)
    }

    pub fn write_response(&mut self, id: NumberOrString, result: ResponseResult) {
        let msg = lsp::make_response(id, result);
        self.write_all(&msg);
    }

    pub fn write_response_error(&mut self, id: Option<NumberOrString>, error: ResponseError) {
        let msg = lsp::make_error_response(id, error);
        self.write_all(&msg);
    }

    fn read_stream(&mut self) -> Result<usize, ResponseError> {
        match self {
            Channel::StdioChannel { cin, decoder, .. } => match cin.fill_buf() {
                Ok(buf) => {
                    if buf.len() <= 0 {
                        return Ok(0);
                    }
                    let len = min(Decoder::CAPACITY - decoder.rest.len(), buf.len());
                    decoder.rest.extend(&buf[..len]);
                    cin.consume(len);

                    debug_assert!(decoder.rest.len() <= Decoder::CAPACITY);
                    Ok(len)
                }
                Err(err) => Err(ResponseError {
                    code: ErrorCodes::ParseError as i64,
                    message: err.to_string(),
                    data: None,
                }),
            },
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
                    Ok(_) => {
                        if let Err(err) = cout.flush() {
                            // Stop trying if error reporting to fallback channel fails.
                            let _ = cerr.write_all(err.to_string().as_bytes());
                            let _ = cerr.flush();
                        }
                    }
                    Err(err) => {
                        // Stop trying if error reporting to fallback channel fails.
                        let _ = cerr.write_all(err.to_string().as_bytes());
                        let _ = cerr.flush();
                    }
                }
            }
        };
    }
}

pub fn build_channel<'a, R, W, E>(cfg: Config, stdio: Stdio<'a, R, W, E>) -> Channel<'a, R, W, E>
where
    R: BufRead,
    W: Write,
    E: Write,
{
    if cfg.channel == ChannelKind::Stdio {
        Channel::StdioChannel {
            cin: stdio.reader,
            cout: stdio.writer,
            cerr: stdio.error,
            decoder: Decoder {
                state: ParseState::Syncing,
                rest: Vec::with_capacity(Decoder::CAPACITY),
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
