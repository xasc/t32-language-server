// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! # Language server child process for non-blocking stdio read operations
//!
//! This applications returns everything it receives via stdin via stdout.
//! All read and write operations are blocking.

use std::{
    io::{self, BufRead, Error, Write},
    thread, time,
};

#[cfg(all(target_os = "wasi", target_env = "p1"))]
use std::sync::mpsc;

#[cfg(all(target_os = "wasi", target_env = "p1"))]
use crate::ReturnCode;

/// For a child process with exclusive access to stdio.
#[cfg(any(windows, unix))]
pub fn receive_excl() -> ! {
    let mut cin = io::stdin().lock();
    let mut cout = io::stdout().lock();
    let mut cerr = io::stderr().lock();

    let idle = time::Duration::from_millis(7);

    loop {
        match cin.fill_buf() {
            Ok(buf) => {
                let len = buf.len();
                if len <= 0 {
                    thread::sleep(idle);
                    continue;
                }

                if let Err(err) = cout.write_all(buf) {
                    to_stderr(&mut cerr, err);
                }
                if let Err(err) = cout.flush() {
                    to_stderr(&mut cerr, err);
                }
                cin.consume(len);
            }
            Err(err) => {
                to_stderr(&mut cerr, err);
                thread::sleep(idle);
            }
        }
    }
}

/// For a background thread with shared access to stdio. Only stdin can be
/// used exclusively. stdout cannot be used and stderr needs to be shared.
#[cfg(all(target_os = "wasi", target_env = "p1"))]
pub fn receive_shared(tx: mpsc::Sender<Vec<u8>>) -> ReturnCode {
    let mut cin = io::stdin().lock();
    let mut cerr = io::stderr();

    let idle = time::Duration::from_millis(7);

    loop {
        match cin.fill_buf() {
            Ok(buf) => {
                let len = buf.len();
                if len <= 0 {
                    thread::sleep(idle);
                    continue;
                }

                if let Err(_) = tx.send(Vec::from(buf)) {
                    return ReturnCode::IoErr;
                }
                cin.consume(len);
            }
            Err(err) => {
                to_stderr(&mut cerr, err);
                thread::sleep(idle);
            }
        }
    }
}

fn to_stderr(cerr: &mut impl Write, err: Error) {
    let _ = cerr.write_all(err.to_string().as_bytes());
    let _ = cerr.flush();
}
