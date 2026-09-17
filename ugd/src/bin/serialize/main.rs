// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

mod config;
mod ltb;
mod persist;

use std::{env, process};

use t32_language_server_user_guide_data::{
    CopyrightFields, PracticeFunctionDefinition, UserGuideData, UserGuideDocs,
};

#[derive(Clone, Debug)]
pub enum ReturnCode {
    OkExit = 0,
    UsageErr = 64,
    DataErr = 65,
    NoInputErr = 66,
    SoftwareErr = 70,
    CantCreateErr = 73,
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let cfg: config::Config = match config::parse_args(args) {
        Ok(c) => c,
        Err(rc) => process::exit(rc as i32),
    };

    let mut copyrights: CopyrightFields = Vec::new();
    let mut docs: UserGuideDocs = Vec::new();

    let functions: Vec<PracticeFunctionDefinition> =
        match ltb::extract_func_defs(&mut copyrights, &mut docs, &cfg.dir) {
            Ok(d) => d,
            Err(rc) => process::exit(rc as i32),
        };

    let ugd = UserGuideData {
        copyrights,
        functions,
        docs,
    };

    if let Err(rc) = persist::write_outfile(ugd, &cfg.outfile) {
        process::exit(rc as i32);
    }
    process::exit(0)
}
