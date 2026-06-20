// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{fs, path::Path};

use serde_json::json;

use t32_language_server_serde::PracticeFunctionDefinition;

use crate::ReturnCode;

pub fn write_outfile(
    funcs: &[PracticeFunctionDefinition],
    outfile: &Path,
) -> Result<(), ReturnCode> {
    if let Some(dir) = outfile.parent() {
        if dir.is_file() {
            eprintln!(
                "ERROR: Cannot create output file. \"{}\" is a file.",
                dir.display()
            );
            return Err(ReturnCode::UsageErr);
        }

        if !dir.is_dir() {
            if let Err(err) = fs::create_dir_all(dir) {
                eprintln!("ERROR: Cannot create output directory: {}", err);
                return Err(ReturnCode::CantCreateErr);
            }
        }
    }

    let payload = json!(funcs);

    if let Err(err) = fs::write(outfile, payload.to_string().as_bytes()) {
        eprintln!("ERROR: Cannot create output file: {}", err);
        return Err(ReturnCode::CantCreateErr);
    }
    Ok(())
}
