// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    io::{self, Write},
    sync::mpsc,
    thread, time,
};

#[cfg(any(windows, unix))]
use std::{cmp::min, io::Read};

#[cfg(any(windows, unix))]
use std::{
    env,
    process::{self, Command, Stdio},
};

use crate::{
    ReturnCode,
    config::{ChannelKind, Config},
    ls::lsp::{self, Message, ParseState, Token},
    ls::response::ErrorResponse,
    protocol::{ErrorCodes, ResponseError},
};

#[cfg(all(target_os = "wasi", target_env = "p1"))]
use crate::stdiotrans::receive_shared;

enum RecvMessage {
    Msg(Message),
    Err(ErrorResponse),
    Heartbeat,
}

pub struct StdioChannel {
    worker: Option<thread::JoinHandle<()>>,
    rx: Option<mpsc::Receiver<RecvMessage>>,

    #[cfg(any(windows, unix))]
    listener: Option<process::Child>,
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
    #[cfg(any(windows, unix))]
    pub fn build() -> Result<Self, ReturnCode> {
        let (tx, rx) = mpsc::channel::<RecvMessage>();

        let bin = env::current_exe().expect("Operation must not fail.");

        // All read operations on stdin are blocking by default. We can move
        // them to a separate thread, but then it becomes impossible to clean
        // it up at the end. It will remain blocked and cannot be aborted
        // cleanly.
        // As workaround we move all stdin read operations to a listener
        // child process that inherits the stdin handle of the parent process. All
        // stdin input is then simply piped back to the parent process. In
        // contrast to a thread, a child process can be cleanly shut down.
        //
        let mut listener = Command::new(bin)
            .args(["--clientProcessId=42", "--mode=stdio-transport"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut cin = listener.stdout.take().unwrap();

        let builder = thread::Builder::new();

        let worker = builder
            .name("Transport Channel".to_string())
            .spawn(move || {
                let mut buf: [u8; Decoder::CAPACITY] = [0; Decoder::CAPACITY];
                let mut decoder = Decoder::build();

                let idle = time::Duration::from_millis(10);

                loop {
                    match Self::read_stdin(&mut cin, &mut buf, &mut decoder) {
                        Ok(0) => {
                            if let Err(_) = tx.send(RecvMessage::Heartbeat) {
                                return;
                            }
                            thread::sleep(idle);
                            continue;
                        }
                        Ok(_) => (),
                        Err(err) => {
                            if let Err(_) = tx.send(RecvMessage::Err(err)) {
                                return;
                            }
                            thread::sleep(idle);
                            continue;
                        }
                    };

                    loop {
                        match lsp::parse(&mut decoder.state, &mut decoder.rest, &mut decoder.tokens)
                        {
                            Ok(None) => {
                                if let Err(_) = tx.send(RecvMessage::Heartbeat) {
                                    return;
                                }
                                break;
                            }
                            Ok(Some(req)) => {
                                if let Err(_) = tx.send(RecvMessage::Msg(req)) {
                                    return;
                                }
                            }
                            Err(err) => {
                                if let Err(_) = tx.send(RecvMessage::Err(err)) {
                                    return;
                                }
                                break;
                            }
                        }
                    }
                }
            })
            .expect("Thread creation must work.");

        Ok(StdioChannel {
            worker: Some(worker),
            rx: Some(rx),
            listener: Some(listener),
        })
    }

    #[cfg(all(target_os = "wasi", target_env = "p1"))]
    pub fn build() -> Result<Self, ReturnCode> {
        let (tx, mut cin) = mpsc::channel::<Vec<u8>>();

        let _ = thread::Builder::new()
            .name("stdin Loopback".to_string())
            .spawn(move || receive_shared(tx))
            .expect("Thread creation must work.");

        let (tx, rx) = mpsc::channel::<RecvMessage>();

        let worker = thread::Builder::new()
            .name("Transport Channel".to_string())
            .spawn(move || {
                let mut decoder = Decoder::build();

                let idle = time::Duration::from_millis(10);

                loop {
                    match Self::read_stdin(&mut cin, &mut decoder) {
                        Ok(0) => {
                            if let Err(_) = tx.send(RecvMessage::Heartbeat) {
                                return;
                            }
                            thread::sleep(idle);
                            continue;
                        }
                        Ok(_) => (),
                        Err(err) => {
                            if let Err(_) = tx.send(RecvMessage::Err(err)) {
                                return;
                            }
                            thread::sleep(idle);
                            continue;
                        }
                    };

                    loop {
                        match lsp::parse(&mut decoder.state, &mut decoder.rest, &mut decoder.tokens)
                        {
                            Ok(None) => {
                                if let Err(_) = tx.send(RecvMessage::Heartbeat) {
                                    return;
                                }
                                break;
                            }
                            Ok(Some(req)) => {
                                if let Err(_) = tx.send(RecvMessage::Msg(req)) {
                                    return;
                                }
                            }
                            Err(err) => {
                                if let Err(_) = tx.send(RecvMessage::Err(err)) {
                                    return;
                                }
                                break;
                            }
                        }
                    }
                }
            })
            .expect("Thread creation must work.");

        Ok(StdioChannel {
            worker: Some(worker),
            rx: Some(rx),
        })
    }

    pub fn recv_msg(&self) -> Result<Option<Message>, ErrorResponse> {
        match self
            .rx
            .as_ref()
            .expect("Must have been populated.")
            .try_recv()
        {
            Ok(recv) => match recv {
                RecvMessage::Msg(msg) => Ok(Some(msg)),
                RecvMessage::Err(err) => Err(err),
                RecvMessage::Heartbeat => Ok(None),
            },
            Err(err) => {
                match err {
                    mpsc::TryRecvError::Empty => Ok(None),
                    // The channel's send half in the worker thread must not disconnect first.
                    mpsc::TryRecvError::Disconnected => panic!(),
                }
            }
        }
    }

    pub fn send_msg(&mut self, msg: Message) {
        let repr = lsp::make_response(msg);
        self.write_stdout(&repr);
    }

    #[cfg(any(windows, unix))]
    fn read_stdin(
        cin: &mut impl Read,
        buf: &mut [u8],
        decoder: &mut Decoder,
    ) -> Result<usize, ErrorResponse> {
        let len = min(Decoder::CAPACITY - decoder.rest.len(), buf.len());

        match cin.read(&mut buf[..len]) {
            Ok(0) => Ok(0),
            Ok(num) => {
                decoder.rest.extend(&buf[..num]);

                debug_assert!(decoder.rest.len() <= Decoder::CAPACITY);
                Ok(num)
            }
            Err(err) => Err(Self::error_read(&err.to_string())),
        }
    }

    #[cfg(all(target_os = "wasi", target_env = "p1"))]
    fn read_stdin(
        cin: &mut mpsc::Receiver<Vec<u8>>,
        decoder: &mut Decoder,
    ) -> Result<usize, ErrorResponse> {
        if decoder.rest.len() >= Decoder::CAPACITY {
            return Ok(Decoder::CAPACITY);
        }

        match cin.try_recv() {
            Ok(mut buf) => {
                let num = buf.len();
                if num > 0 {
                    decoder.rest.append(&mut buf);
                }
                Ok(num)
            }
            Err(err) => {
                match err {
                    mpsc::TryRecvError::Empty => Ok(0),
                    // The loopback reader might panic first. However, we must
                    // not propagate the channel shutdown. The other half of
                    // the channel must trigger the shutodwn.
                    mpsc::TryRecvError::Disconnected => {
                        Err(Self::error_transport(&err.to_string()))
                    }
                }
            }
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

    #[cfg(any(windows, unix))]
    fn error_read(err: &str) -> ErrorResponse {
        ErrorResponse {
            id: None,
            error: ResponseError {
                code: ErrorCodes::ParseError as i64,
                message: err.to_string(),
                data: None,
            },
        }
    }

    #[cfg(all(target_os = "wasi", target_env = "p1"))]
    fn error_transport(err: &str) -> ErrorResponse {
        ErrorResponse {
            id: None,
            error: ResponseError {
                code: ErrorCodes::TransportError as i64,
                message: err.to_string(),
                data: None,
            },
        }
    }
}

impl Drop for StdioChannel {
    fn drop(&mut self) {
        // Close receiver half of transport channel, so that the next transmit
        // operation will fail. The failure will abort the worker thread.
        //
        // If we are using a thread as loopback channel for stdin, then closing
        // the worker thread will automatically terminate the reader thread.
        // However, it is not guaranteed that the reader will ever return from
        // the current read operation. Reading from stdin is always blocking.
        //
        drop(self.rx.take());

        #[cfg(any(windows, unix))]
        if let Some(mut p) = self.listener.take() {
            p.kill().expect("Must be able to shut down child process.");
        }

        if let Some(t) = self.worker.take() {
            t.join().expect("Joining the worker thread must not fail.");
        }
    }
}

pub fn build_channel(cfg: &Config) -> Result<StdioChannel, ReturnCode> {
    if cfg.channel == ChannelKind::Stdio {
        Ok(StdioChannel::build())?
    } else if cfg.channel == ChannelKind::Pipe {
        unreachable!()
    } else {
        assert_eq!(cfg.channel, ChannelKind::Socket);
        unreachable!()
    }
}
