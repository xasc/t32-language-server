// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::{self, Write};

use crate::{
    protocol::{PositionEncodingKind, TraceValue},
    ReturnCode,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelKind {
    Pipe,
    Socket,
    Stdio,
}

pub struct Config {
    pub parent_pid: i64,
    pub channel: ChannelKind,
    pub workspace_root: Option<String>,
    pub workspace_folders: Vec<String>,
    pub trace_level: TraceValue,
    pub position_encoding: PositionEncodingKind,
}

impl Config {
    pub fn build(
        args: &[String],
    ) -> Result<Self, ReturnCode> {
        let mut ppid: Option<i64> = None;
        let mut show_help: bool = false;

        debug_assert!(args.len() > 0);
        let len = args[1..].len();
        for (ii, arg) in args[1..].iter().enumerate() {
            match Self::parse_flag_value::<i64>(
                "--clientProcessId=",
                "-c",
                arg,
                if ii < len - 1 {
                    Some(&args[1..][ii + 1])
                } else {
                    None
                },
            ) {
                Err(err) => return Err(err),
                Ok(Some(num)) => {
                    ppid = Some(num);
                    continue;
                }
                Ok(None) => (),
            }

            if Self::parse_flag("--help", "-h", arg) {
                show_help = true;
            }
        }

        if show_help {
            usage(&mut io::stdout());
            return Err(ReturnCode::OkExit);
        } else if ppid.is_none() {
            error_missing(&mut io::stdout(), "--clientProcessId=PID");
            return Err(ReturnCode::UsageErr);
        }

        Ok(Config {
            parent_pid: ppid.unwrap(),
            channel: ChannelKind::Stdio,
            workspace_root: None,
            workspace_folders: Vec::new(),
            trace_level: TraceValue::Off,
            position_encoding: PositionEncodingKind::Utf16,
        })
    }

    fn parse_flag_value<T: std::str::FromStr>(
        long: &str,
        short: &str,
        arg: &str,
        next: Option<&str>,
    ) -> Result<Option<T>, ReturnCode> {
        if arg == short {
            if let None = next {
                error_format_value(&mut io::stderr(), short);
                return Err(ReturnCode::UsageErr);
            }

            match next.expect("The flag must have a value.").parse::<T>() {
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

        let val: Vec<&str> = arg
            .split(long)
            .map(str::trim)
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

    fn parse_flag(long: &str, short: &str, arg: &str) -> bool {
        arg == short || arg == long
    }
}

fn error_format(writer: &mut impl Write, param: &str) {
    let _ = writeln!(writer, "Error: Invalid format for argument \"{param}\"");
}

fn error_format_value(writer: &mut impl Write, param: &str) {
    let _ = writeln!(
        writer,
        "Error: Invalid format for argument value \"{param}\""
    );
}

fn error_missing(writer: &mut impl Write, param: &str) {
    let _ = writeln!(writer, "Error: Missing argument \"{param}\"");
}

fn usage(writer: &mut impl Write) {
    let _ = writeln!(
        writer,
        r#"Usage: t32-language-server [OPTIONS]

Language server for the Lauterbach TRACE32® script language.


General options:
  -h, --help
    Show this help message and exit.

  -c PID, --clientProcessId=PID
    Process ID of the client that started the server. The server can use the
    PID to monitor the client process and shut itself down if the client
    process dies."#
    )
    .expect("Writer must be configured correctly.");
}
