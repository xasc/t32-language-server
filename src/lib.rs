// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::{BufRead, Write};

mod parser;
mod protocol;
#[derive(Debug, PartialEq)]
pub enum ReturnCode {
    OkExit = 0,
    DataErr = 64,
    UsageErr = 65,
    IoErr = 74,
    ProtocolErr = 76,
}

pub struct Streams<R: BufRead, W: Write, E: Write> {
    pub reader: R,
    pub writer: W,
    pub error: E,
}

pub struct Config {
    parent_pid: usize,
}

pub fn run<R, W, E>(args: Vec<String>, streams: &mut Streams<R, W, E>) -> ReturnCode
where
    R: BufRead,
    W: Write,
    E: Write,
{
    let cfg = Config::build(&args, &mut streams.writer);
    if let Err(rc) = cfg {
        return rc;
    }

    serve(&mut streams.reader);
    ReturnCode::OkExit
}

impl Config {
    pub fn build(args: &[String], writer: &mut impl Write) -> Result<Self, ReturnCode>
    {
        let mut ppid: Option<usize> = None;
        let mut show_help: bool = false;

        let len = args[1..].len();
        for (ii, arg) in args[1..].iter().enumerate() {
            match Self::parse_flag_value::<usize>(
                "--clientProcessId=",
                "-c",
                arg,
                if ii < len - 1 {
                    Some(&args[ii + 1])
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
            usage(writer);
            return Err(ReturnCode::OkExit);
        } else if ppid.is_none() {
            error_missing("--clientProcessId=PID");
            return Err(ReturnCode::UsageErr);
        }

        Ok(Config {
            parent_pid: ppid.unwrap(),
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
                error_format_value(short);
                return Err(ReturnCode::UsageErr);
            }

            match next.unwrap().parse::<T>() {
                Ok(v) => return Ok(Some(v)),
                Err(_) => {
                    error_format(long);
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

        if val.len() != 2 {
            error_format(long);
            return Err(ReturnCode::UsageErr);
        }

        match val[1].parse::<T>() {
            Ok(v) => Ok(Some(v)),
            Err(_) => {
                error_format(long);
                Err(ReturnCode::UsageErr)
            }
        }
    }

    fn parse_flag(long: &str, short: &str, arg: &str) -> bool {
        arg == short || arg == long
    }
}

fn serve(buf: &mut impl BufRead) -> Option<ReturnCode> {
    let _ = eval(buf);
    None
}

fn eval(buf: &mut impl BufRead) -> Result<(), ReturnCode> {
    parser::parse(buf);
    Ok(())
}

pub fn error(message: &str) {
    println!("Error: {message}");
}

fn error_format(param: &str) {
    eprintln!("Error: Invalid format for argument \"{param}\"");
}

fn error_format_value(param: &str) {
    eprintln!("Error: Invalid format for argument value \"{param}\"");
}

fn error_missing(param: &str) {
    eprintln!("Error: Missing argument \"{param}\"");
}

fn usage(writer: &mut impl Write) {
    writeln!(writer, r#"Usage: t32-language-server [OPTIONS]

Language server for the Lauterbach TRACE32® script language.


General options:
  -h, --help
    Show this help message and exit.

  -c PID, --clientProcessId=PID
    Process ID of the client that started the server. The server can use the
    PID to monitor the client process and shut itself down if the client
    process dies."#).expect("Writer must be configured correctly.");
}
