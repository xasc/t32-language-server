//! # Simple Listener Process for stdin
//!
//! This applications returns everything it receives via stdin via stdout.
//! All read and write operations are blocking.

use std::io::{self, BufRead, Error, Write};

fn main() {
    let mut cin = io::stdin().lock();
    let mut cout = io::stdout().lock();
    let mut cerr = io::stderr().lock();

    loop {
        match cin.fill_buf() {
            Ok(buf) => {
                let len = buf.len();
                if len <= 0 {
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
            }
        }
    }
}

fn to_stderr(cerr: &mut impl Write, err: Error) {
    let _ = cerr.write_all(err.to_string().as_bytes());
    let _ = cerr.flush();
}
