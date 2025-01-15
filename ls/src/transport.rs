// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use libc;
use std::{
    cmp::min,
    env,
    io::{self, Read, Write},
    path::PathBuf,
    process::{self, Command, Stdio},
    sync::mpsc,
    thread, time,
};

use crate::{
    config::{ChannelKind, Config},
    lsp::{self, ParseState, Token},
    protocol::{ErrorCodes, NumberOrString, ResponseError},
    request::Request,
    response::ResponseResult,
    ReturnCode,
};

enum RecvMessage {
    Req(Request),
    Err(ResponseError),
    Heartbeat,
}

pub struct StdioChannel {
    worker: Option<thread::JoinHandle<()>>,
    rx: Option<mpsc::Receiver<RecvMessage>>,
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
    pub fn build() -> Result<Self, ReturnCode> {
        let (tx, rx) = mpsc::channel::<RecvMessage>();

        let dir = match env::current_exe() {
            Ok(path) => path,
            Err(err) => {
                let _ = io::stderr().write_all(
                    format!("Error: Cannot get directory of this executable: {}", err)
                        .as_bytes(),
                );
                return Err(ReturnCode::NoInputErr);
            }
        };

        let mut bin = PathBuf::from(dir.parent().expect("Executable must have one parent."));
        bin.push("t32-language-server-stdin");

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
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut cin =  listener.stdout.take().unwrap();

        let worker = thread::spawn(move || {
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

                match lsp::parse(&mut decoder.state, &mut decoder.rest, &mut decoder.tokens) {
                    Ok(None) => {
                        if let Err(_) = tx.send(RecvMessage::Heartbeat) {
                            return;
                        }
                    }
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
            }
        });

        Ok(StdioChannel {
            worker: Some(worker),
            rx: Some(rx),
            listener: Some(listener),
        })
    }

    pub fn recv_msg(&self) -> Result<Option<Request>, ResponseError> {
        match self
            .rx
            .as_ref()
            .expect("Must have been populated.")
            .try_recv()
        {
            Ok(msg) => match msg {
                RecvMessage::Req(req) => Ok(Some(req)),
                RecvMessage::Err(err) => Err(err),
                RecvMessage::Heartbeat => Ok(None),
            },
            Err(err) => {
                match err {
                    mpsc::TryRecvError::Empty => Ok(None),
                    // The channel's send half in the worker thread must not disconnect first.
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

    fn read_stdin(cin: &mut impl Read, buf: &mut [u8], decoder: &mut Decoder) -> Result<usize, ResponseError> {
        let len = min(Decoder::CAPACITY - decoder.rest.len(), buf.len());

        match cin.read(&mut buf[..len]) {
            Ok(0) => Ok(0),
            Ok(num) => {
                decoder.rest.extend(&buf[..num]);

                debug_assert!(decoder.rest.len() <= Decoder::CAPACITY);
                Ok(num)
            }
            Err(err) => Err(ResponseError {
                code: ErrorCodes::ParseError as i64,
                message: err.to_string(),
                data: None,
            }),
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

        if let Some(mut p) = self.listener.take() {
            p.kill().expect("Must be able to shut down child process.");
        }

        if let Some(t) = self.worker.take() {
            t.join().expect("Joining the worker thread must not fail.");
        }
    }
}

pub fn build_channel(cfg: Config) -> Result<StdioChannel, ReturnCode> {
    if cfg.channel == ChannelKind::Stdio {
        Ok(StdioChannel::build())?
    } else if cfg.channel == ChannelKind::Pipe {
        unreachable!()
    } else {
        assert_eq!(cfg.channel, ChannelKind::Socket);
        unreachable!()
    }
}
