// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2
//

mod config;
mod ls;
mod protocol;
mod stdiotrans;
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

use config::OperationMode;

pub fn run(args: Vec<String>) -> ReturnCode {
    let cfg = match config::Config::build(&args) {
        Ok(conf) => conf,
        Err(rc) => return rc,
    };

    if cfg.mode == OperationMode::Server {
        ls::serve(cfg)
    } else {
        stdiotrans::receive();
    }
}
