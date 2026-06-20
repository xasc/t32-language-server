// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    io::{self, Write},
    path::PathBuf,
};

use crate::ReturnCode;

pub struct Config {
    pub dir: PathBuf,
    pub outfile: PathBuf,
}

const PKG_VERSION: &'static str = env!("CARGO_PKG_VERSION");
const GIT_HEADREF: Option<&'static str> = option_env!("GIT_HEAD_REF");

fn error_format(writer: &mut impl Write, param: &str) {
    let _ = writeln!(writer, "ERROR: Invalid format for argument \"{param}\"");
}

fn error_format_value(writer: &mut impl Write, param: &str) {
    let _ = writeln!(
        writer,
        "ERROR: Invalid format for argument value \"{param}\""
    );
}

fn error_missing(writer: &mut impl Write, param: &str) {
    let _ = writeln!(writer, "ERROR: Missing argument \"{param}\"");
}

pub fn parse_args(args: Vec<String>) -> Result<Config, ReturnCode> {
    let mut show_help: bool = false;
    let mut show_version: bool = false;

    let mut dir: Option<PathBuf> = None;
    let mut outfile: Option<PathBuf> = None;

    let len = args[1..].len();
    for (ii, arg) in args[1..].iter().enumerate() {
        let next = if ii < len - 1 {
            Some(args[1..][ii + 1].as_str())
        } else {
            None
        };

        if parse_flag("--help", "-h", arg) {
            show_help = true;
            break;
        }

        if parse_flag("--version", "-V", arg) {
            show_version = true;
            break;
        }

        if outfile.is_none() {
            match parse_flag_value::<PathBuf>("--output=", Some("-o"), arg, next) {
                Err(err) => return Err(err),
                Ok(p) => {
                    outfile = p;
                    continue;
                }
            }
        }

        if dir.is_none() {
            dir = Some(PathBuf::from(arg.clone()));
        }
    }

    if show_help {
        usage(&mut io::stdout());
        return Err(ReturnCode::OkExit);
    }

    if show_version {
        version(&mut io::stdout());
        return Err(ReturnCode::OkExit);
    }

    if dir.is_none() {
        error_missing(&mut io::stderr(), "DIR");
        return Err(ReturnCode::UsageErr);
    }

    if let Some(d) = &dir
        && !d.is_dir()
    {
        eprintln!("ERROR: Directory \"{}\" does not exist.", d.display());
        return Err(ReturnCode::UsageErr);
    }

    if outfile.is_none() {
        error_missing(&mut io::stderr(), "--output=FILE");
        return Err(ReturnCode::UsageErr);
    }

    Ok(Config {
        dir: dir.unwrap(),
        outfile: outfile.unwrap(),
    })
}

fn parse_flag(long: &str, short: &str, arg: &str) -> bool {
    arg == short || arg == long
}

fn parse_flag_value<'a, T: std::str::FromStr>(
    long: &str,
    short: Option<&str>,
    arg: &'a str,
    next: Option<&str>,
) -> Result<Option<T>, ReturnCode> {
    if let Some(sh) = short
        && sh == arg
    {
        if let None = next {
            error_format_value(&mut io::stderr(), sh);
            return Err(ReturnCode::UsageErr);
        }

        let val = next
            .expect("The flag must have a value.")
            .trim_matches(&['"', '\'']);

        match val.parse::<T>() {
            Ok(v) => return Ok(Some(v)),
            Err(_) => {
                error_format(&mut io::stderr(), long);
                return Err(ReturnCode::UsageErr);
            }
        }
    }

    if !arg.starts_with(long) {
        return Ok(None);
    }

    let trim_parens = |s: &'a str| -> &'a str { s.trim_matches(&['"', '\'']) };

    let val: Vec<&str> = arg
        .split(long)
        .map(str::trim)
        .map(trim_parens)
        .filter(|s| !s.is_empty())
        .collect();

    if val.len() != 1 {
        error_format(&mut io::stderr(), long);
        return Err(ReturnCode::UsageErr);
    }

    match val[0].parse::<T>() {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            error_format(&mut io::stderr(), long);
            Err(ReturnCode::UsageErr)
        }
    }
}

fn usage(writer: &mut impl Write) {
    let _ = writeln!(
        writer,
        r#"Usage: t32ls-serialize [OPTION] DIR

Serialize the Lauterbach TRACE32® command and function reference. The text file
representation of the command and function reference is found in the directory
DIR.


General options:
  -h, --help
    Show this help message and exit.

  -o, --output=FILE
    Location of the serialized output FILE.

  -V, --version
    Print version info and exit."#
    )
    .expect("Writer must be configured correctly.");
}

fn version_str() -> String {
    format!(
        "{}{}",
        PKG_VERSION,
        if let Some(hash) = GIT_HEADREF {
            format!("+{}", hash)
        } else {
            "".to_string()
        }
    )
}

fn version(writer: &mut impl Write) {
    // REUSE-IgnoreStart
    let _ = writeln!(
        writer,
        r#"t32ls-serialize (t32-language-server-serialize), version {}
SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
SPDX-License-Identifier: EUPL-1.2"#,
        version_str(),
    )
    .expect("Writer must be configured correctly.");
    // REUSE-IgnoreEnd
}
