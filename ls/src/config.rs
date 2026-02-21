// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    io::{self, Write},
    str::FromStr,
    time::Duration,
};

use serde::Serialize;

use crate::{
    ReturnCode,
    protocol::{
        PositionEncodingKind, SemanticTokenModifiers, SemanticTokenTypes, SemanticTokensLegend,
        TraceValue, Uri, WorkspaceFolder,
    },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelKind {
    Pipe,
    Socket,
    Stdio,
}

#[derive(PartialEq)]
pub enum OperationMode {
    Server,
    StdioTransport,
}

#[derive(Clone, Debug, Serialize)]
pub enum Workspace {
    Root(Option<Uri>),
    Folders(Option<Vec<WorkspaceFolder>>),
}

pub struct Config {
    pub parent_pid: Option<u32>,
    pub pid_check_interval: Duration,
    pub channel: ChannelKind,
    pub mode: OperationMode,
    pub workspace: Workspace,
    pub workspace_folders_supported: bool,
    pub trace_level: TraceValue,
    pub position_encoding: PositionEncodingKind,
    pub location_links: LocationLinkSupport,
    pub did_rename_files_supported: bool,
    pub semantic_tokens: SemanticTokenSupport,
}

pub struct LocationLinkSupport {
    pub definitions_supported: bool,
}

#[derive(Clone, Debug)]
pub struct SemanticTokenEncoding {
    pub overlapping_tokens: bool,
    pub multiline_tokens: bool,
}

#[derive(Clone, Debug)]
pub struct SemanticTokenSupport {
    pub encoding: SemanticTokenEncoding,
    pub legend: SemanticTokensLegend,
}

impl Config {
    pub fn build(args: &[String]) -> Result<Self, ReturnCode> {
        let mut ppid: Option<u32> = None;
        let mut show_help: bool = false;
        let mut trace_level: TraceValue = TraceValue::Off;
        let mut mode: OperationMode = OperationMode::Server;

        debug_assert!(args.len() > 0);
        let len = args[1..].len();
        for (ii, arg) in args[1..].iter().enumerate() {
            let next = if ii < len - 1 {
                Some(args[1..][ii + 1].as_str())
            } else {
                None
            };

            match Self::parse_flag_value::<u32>("--clientProcessId=", Some("-c"), arg, next) {
                Err(err) => return Err(err),
                Ok(Some(num)) => {
                    ppid = Some(num);
                    continue;
                }
                Ok(None) => (),
            }

            match Self::parse_flag_value::<TraceValue>("--trace=", Some("-t"), arg, next) {
                Err(err) => return Err(err),
                Ok(Some(level)) => {
                    trace_level = level;
                    continue;
                }
                Ok(None) => (),
            }

            match Self::parse_flag_value::<OperationMode>("--mode=", None, arg, next) {
                Err(err) => return Err(err),
                Ok(Some(opmode)) => {
                    mode = opmode;
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
        }

        if ppid.is_none() {
            error_missing(&mut io::stderr(), "--clientProcessId=PID");
            return Err(ReturnCode::UsageErr);
        }

        Ok(Config {
            parent_pid: Some(ppid.unwrap()),
            pid_check_interval: Duration::from_secs(5),
            channel: ChannelKind::Stdio,
            workspace: Workspace::Root(None),
            workspace_folders_supported: false,
            position_encoding: PositionEncodingKind::Utf16,
            location_links: LocationLinkSupport {
                definitions_supported: false,
            },
            did_rename_files_supported: false,
            trace_level,
            mode,
            semantic_tokens: SemanticTokenSupport::default(),
        })
    }

    fn parse_flag_value<T: std::str::FromStr>(
        long: &str,
        short: Option<&str>,
        arg: &str,
        next: Option<&str>,
    ) -> Result<Option<T>, ReturnCode> {
        if let Some(sh) = short
            && sh == arg
        {
            if let None = next {
                error_format_value(&mut io::stderr(), sh);
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

impl Default for SemanticTokenSupport {
    fn default() -> Self {
        Self {
            encoding: SemanticTokenEncoding {
                overlapping_tokens: false,
                multiline_tokens: false,
            },
            legend: SemanticTokensLegend::new(),
        }
    }
}

impl FromStr for OperationMode {
    type Err = ();

    fn from_str(val: &str) -> Result<Self, Self::Err> {
        match val {
            "server" => Ok(OperationMode::Server),
            "stdio-transport" => Ok(OperationMode::StdioTransport),
            _ => Err(()),
        }
    }
}

impl FromStr for SemanticTokenModifiers {
    type Err = ();

    fn from_str(val: &str) -> Result<Self, Self::Err> {
        match val {
            "declaration" => Ok(Self::Declaration),
            "definition" => Ok(Self::Definition),
            "readonly" => Ok(Self::Readonly),
            "static" => Ok(Self::Static),
            "deprecated" => Ok(Self::Deprecated),
            "abstract" => Ok(Self::Abstract),
            "async" => Ok(Self::Async),
            "modification" => Ok(Self::Modification),
            "documentation" => Ok(Self::Documentation),
            "defaultLibrary" => Ok(Self::DefaultLibrary),
            _ => Err(()),
        }
    }
}

impl FromStr for SemanticTokenTypes {
    type Err = ();

    fn from_str(val: &str) -> Result<Self, Self::Err> {
        match val {
            "namespace" => Ok(Self::Namespace),
            "type" => Ok(Self::Type),
            "class" => Ok(Self::Class),
            "enum" => Ok(Self::Enum),
            "interface" => Ok(Self::Interface),
            "struct" => Ok(Self::Struct),
            "typeParameter" => Ok(Self::TypeParameter),
            "parameter" => Ok(Self::Parameter),
            "variable" => Ok(Self::Variable),
            "property" => Ok(Self::Property),
            "enumMember" => Ok(Self::EnumMember),
            "event" => Ok(Self::Event),
            "function" => Ok(Self::Function),
            "method" => Ok(Self::Method),
            "macro" => Ok(Self::Macro),
            "keyword" => Ok(Self::Keyword),
            "modifier" => Ok(Self::Modifier),
            "comment" => Ok(Self::Comment),
            "string" => Ok(Self::String),
            "number" => Ok(Self::Number),
            "regexp" => Ok(Self::Regexp),
            "operator" => Ok(Self::Operator),
            "decorator" => Ok(Self::Decorator),
            "label" => Ok(Self::Label),
            _ => Err(()),
        }
    }
}

impl FromStr for TraceValue {
    type Err = ();

    fn from_str(val: &str) -> Result<Self, Self::Err> {
        match val {
            "off" => Ok(TraceValue::Off),
            "messages" => Ok(TraceValue::Messages),
            "verbose" => Ok(TraceValue::Verbose),
            _ => Err(()),
        }
    }
}

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
    process dies.

  -t LEVEL, --trace=LEVEL
    Set the initial logging level of the server's execution trace. LEVEL must
    be one of 'off,messages,verbose'."#
    )
    .expect("Writer must be configured correctly.");
}
