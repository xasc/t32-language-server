// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later
//

mod config;
mod ls;
mod protocol;
mod t32;
mod utils;

pub use config::Config;

#[derive(Debug, PartialEq)]
pub enum ReturnCode {
    OkExit = 0,
    ErrExit = 1,
    UsageErr = 65,
    NoInputErr = 66,
    UnavailableErr = 69,
    IoErr = 74,
    ProtocolErr = 76,
}

pub fn run(args: Vec<String>) -> ReturnCode {
    let cfg = match config::Config::build(&args) {
        Ok(conf) => conf,
        Err(rc) => return rc,
    };
    ls::serve(cfg)
}
