// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    cmp::min,
    io::{self, BufRead, Write},
    os::unix::io::AsRawFd,
    sync::mpsc,
    thread,
    time,
};

use crate::{
    config::{ChannelKind, Config},
    lsp::{self, ParseState, Token},
    protocol::{ErrorCodes, NumberOrString, ResponseError},
    request::Request,
    response::ResponseResult,
};

enum RecvMessage {
    Req(Request),
    Err(ResponseError),
}

pub struct StdioChannel {
    worker: Option<thread::JoinHandle<()>>,
    rx: Option<mpsc::Receiver<RecvMessage>>,
}

pub struct Decoder {
    state: ParseState,
    rest: Vec<u8>,
    tokens: Vec<Token>,
}

impl Decoder {
    const CAPACITY: usize = 4096;

    pub fn build() -> Self {
        Decoder {
            state: ParseState::Syncing,
            rest: Vec::with_capacity(Decoder::CAPACITY),
            tokens: Vec::new(),
        }
    }
}

impl StdioChannel {
    pub fn build() -> Self {
        let (tx, rx) = mpsc::channel::<RecvMessage>();

        let worker = thread::spawn(move || {
                Self::make_stdin_nonblock();

                let mut cin = io::stdin().lock();
                let mut decoder = Decoder::build();

                let idle = time::Duration::from_millis(10);

                loop {
                    let num = match Self::read_stdin(&mut cin, &mut decoder) {
                        Ok(num) => num,
                        Err(err) => {
                            if let Err(_) = tx.send(RecvMessage::Err(err)) {
                                return;
                            }
                            continue;
                        }
                    };

                    match lsp::parse(&mut decoder.state, &mut decoder.rest, &mut decoder.tokens) {
                        Ok(None) => (),
                        Ok(Some(req)) => {
                            if let Err(_) = tx.send(RecvMessage::Req(req)) {
                                return;
                            }
                        }
                        Err(err) => {
                            if let Err(_) = tx.send(RecvMessage::Err(err)) {
                                return;
                            }
                        }
                    }

                    // Place check after send operations to check whether the receiver is
                    // still alive.
                    if num <=0 {
                        thread::sleep(idle);
                    }
                }
        });

        StdioChannel {
            worker: Some(worker),
            rx: Some(rx),
        }
    }

    pub fn recv_msg(&self) -> Result<Option<Request>, ResponseError> {
        match self.rx.as_ref().expect("Must have been populated.").try_recv() {
            Ok(msg) => {
                match msg {
                    RecvMessage::Req(req) => Ok(Some(req)),
                    RecvMessage::Err(err) => Err(err),
                }
            }
            Err(err) => {
                match err {
                    mpsc::TryRecvError::Empty => Ok(None),
                    // The channel's send half in the worker must not disconnect first.
                    mpsc::TryRecvError::Disconnected => unreachable!(),
                }
            }
        }
    }

    pub fn send_response(&mut self, id: NumberOrString, result: ResponseResult) {
        let msg = lsp::make_response(id, result);
        self.write_stdout(&msg);
    }

    pub fn send_response_error(&mut self, id: Option<NumberOrString>, error: ResponseError) {
        let msg = lsp::make_error_response(id, error);
        self.write_stdout(&msg);
    }

    fn read_stdin(cin: &mut impl BufRead, decoder: &mut Decoder) -> Result<usize, ResponseError> {
        match cin.fill_buf() {
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
        }
    }

    // Switch Unix file descriptor of stdin to non-blocking mode.
    // (see https://stackoverflow.com/questions/68173341/how-to-clear-or-remove-iostdin-buffer-in-rust/68174244#68174244)
    fn make_stdin_nonblock() {
        let fd = io::stdin().as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(
                fd,
                libc::F_SETFL,
                flags | libc::O_NONBLOCK
            );
        }
    }

    fn write_stdout(&mut self, buf: &[u8]) {
        match io::stdout().write_all(buf) {
            Ok(_) => {
                if let Err(err) = io::stdout().flush() {
                    // Stop trying if error reporting to fallback channel fails.
                    let _ = io::stderr().write_all(err.to_string().as_bytes());
                    let _ = io::stderr().flush();
                }
            }
            Err(err) => {
                // Stop trying if error reporting to fallback channel fails.
                let _ = io::stderr().write_all(err.to_string().as_bytes());
                let _ = io::stderr().flush();
            }
        }
    }
}

impl Drop for StdioChannel {
    fn drop(&mut self) {
        // Close receiver half of channel, so that the next transmit operation
        // will fail. The failure will abort the worker thread.
        drop(self.rx.take());

        if let Some(t) = self.worker.take() {
            t.join().expect("Joining the worker thread must not fail.");
        }
    }
}

pub fn build_channel(cfg: Config) -> StdioChannel {
    if cfg.channel == ChannelKind::Stdio {
        StdioChannel::build()
    } else if cfg.channel == ChannelKind::Pipe {
        unreachable!()
    } else {
        assert_eq!(cfg.channel, ChannelKind::Socket);
        unreachable!()
    }
}
