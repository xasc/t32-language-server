// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PracticeFuncArgumentPattern {
    Angled(String),
    Braced(Box<PracticeFuncArgumentPattern>),
    Choice(Vec<PracticeFuncArgumentPattern>),
    Chained(Vec<PracticeFuncArgumentPattern>),
    Ellipsis(Vec<PracticeFuncArgumentPattern>),

    Literal(String),

    Optional(Vec<PracticeFuncArgumentPattern>),
    OptionalOtherArg(Vec<PracticeFuncArgumentPattern>),
    Segmented(Vec<PracticeFuncArgumentPattern>),
    Quoted(Box<PracticeFuncArgumentPattern>),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PracticeFuncNamePattern {
    Angled(String),
    Chained(Vec<PracticeFuncNamePattern>),
    Literal(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PracticeFunctionDefinition {
    pub name: PracticeFunctionName,
    pub args: PracticeFunctionArguments,
    pub operation: String,
    pub copyright: u32,
    pub source: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UserGuideData {
    pub copyrights: CopyrightFields,
    pub docs: UserGuideDocs,
    pub functions: Vec<PracticeFunctionDefinition>,
}

pub type CopyrightFields = Vec<String>;
pub type PracticeFunctionArguments = Vec<PracticeFuncArgumentPattern>;
pub type PracticeFunctionName = Vec<PracticeFuncNamePattern>;
pub type UserGuideDocs = Vec<String>;

pub const DATFILE_FIELD_WIDTH_BITS_TOTAL: usize = size_of::<u64>();
pub const DATFILE_FIELD_WIDTH_NODES_TOTAL: usize = size_of::<u16>();
pub const DATFILE_FIELD_WIDTH_NODE_TAG: usize = size_of::<u8>();
pub const DATFILE_FIELD_WIDTH_NODE_CHAR: usize = size_of::<u32>();
pub const DATFILE_FIELD_WIDTH_SUCCESSOR: usize = size_of::<u16>();

impl UserGuideData {
    pub fn new() -> Self {
        UserGuideData {
            copyrights: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        }
    }
}

impl fmt::Display for PracticeFuncArgumentPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PracticeFuncArgumentPattern::Angled(p) => write!(f, "<{}>", p),
            PracticeFuncArgumentPattern::Quoted(p) => write!(f, "\"{}\"", p),
            PracticeFuncArgumentPattern::Optional(p) => {
                write!(f, "[{}", p[0])?;
                for arg in p[1..].iter() {
                    match arg {
                        PracticeFuncArgumentPattern::OptionalOtherArg(_) => write!(f, "{}", arg)?,
                        _ => write!(f, ", {}", arg)?,
                    }
                }
                write!(f, "]")
            }
            PracticeFuncArgumentPattern::OptionalOtherArg(p) => {
                write!(f, "[, {}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, ", {}", arg)?;
                }
                write!(f, "]")
            }
            PracticeFuncArgumentPattern::Choice(p) => {
                write!(f, "{}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, " | {}", arg)?;
                }
                Ok(())
            }
            PracticeFuncArgumentPattern::Braced(p) => write!(f, "{{{}}}", p),
            PracticeFuncArgumentPattern::Literal(p) => write!(f, "{}", p),
            PracticeFuncArgumentPattern::Segmented(p) => {
                write!(f, "{}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, ".{}", arg)?;
                }
                Ok(())
            }
            PracticeFuncArgumentPattern::Chained(p) => {
                for arg in p.iter() {
                    write!(f, "{}", arg)?
                }
                Ok(())
            }
            PracticeFuncArgumentPattern::Ellipsis(p) => {
                write!(f, "{}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, " .. {}", arg)?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for PracticeFuncNamePattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PracticeFuncNamePattern::Literal(p) => write!(f, "{}", p),
            PracticeFuncNamePattern::Angled(p) => write!(f, "<{}>", p),
            PracticeFuncNamePattern::Chained(p) => {
                for seg in p {
                    write!(f, "{}", seg)?;
                }
                Ok(())
            }
        }
    }
}

pub fn format_practice_func_args(args: &PracticeFunctionArguments) -> String {
    let mut out: String = String::new();

    let len = args.len();
    if len > 0 {
        out = args[0].to_string();
    }

    if len < 2 {
        return out;
    }

    for arg in args[1..].iter() {
        match arg {
            PracticeFuncArgumentPattern::OptionalOtherArg(_) => out += &arg.to_string(),
            _ => out += &format!(", {}", arg),
        }
    }
    out
}

pub fn format_practice_func_name(name: &PracticeFunctionName) -> String {
    let mut out = String::new();

    if !name.is_empty() {
        out = name[0].to_string();
    }

    for part in name[1..].iter() {
        out += ".";
        out += &part.to_string();
    }
    out
}
