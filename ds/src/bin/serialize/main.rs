// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

mod config;
mod ltb;
mod persist;

use std::{env, process};

use t32_language_server_serde::PracticeFunctionDefinition;

pub enum ReturnCode {
    OkExit = 0,
    UsageErr = 65,
    NoInputErr = 66,
    CantCreateErr = 73,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let cfg: config::Config = match config::parse_args(args) {
        Ok(c) => c,
        Err(rc) => process::exit(rc as i32),
    };

    let funcs: Vec<PracticeFunctionDefinition> = match ltb::extract_func_defs(&cfg.dir) {
        Ok(d) => d,
        Err(rc) => process::exit(rc as i32),
    };

    if let Err(rc) = persist::write_outfile(&funcs, &cfg.outfile) {
        process::exit(rc as i32);
    }
    process::exit(0)
}
