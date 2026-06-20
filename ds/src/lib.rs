// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::fmt;

use serde::Serialize;

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PracticeFuncNamePattern {
    Angled(String),
    Chained(Vec<PracticeFuncNamePattern>),
    Literal(String),
}

#[derive(Debug, Serialize)]
pub struct PracticeFunctionDefinition {
    pub name: PracticeFunctionName,
    pub args: PracticeFunctionArguments,
    pub operation: String,
    pub copyright: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct PracticeFunctionArguments {
    args: Vec<PracticeFuncArgumentPattern>,
}

#[derive(Debug, Serialize)]
pub struct PracticeFunctionName {
    components: Vec<PracticeFuncNamePattern>,
}

impl PracticeFunctionArguments {
    pub fn from_params(args: Vec<PracticeFuncArgumentPattern>) -> Self {
        Self { args }
    }
}

impl PracticeFunctionName {
    pub fn from_parts(parts: Vec<PracticeFuncNamePattern>) -> Self {
        Self { components: parts }
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

impl fmt::Display for PracticeFunctionArguments {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let len = self.args.len();
        if len > 0 {
            write!(f, "{}", self.args[0])?;
        }

        if len < 2 {
            return Ok(());
        }

        for arg in self.args[1..].iter() {
            match arg {
                PracticeFuncArgumentPattern::OptionalOtherArg(_) => write!(f, "{}", arg)?,
                _ => write!(f, ", {}", arg)?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for PracticeFunctionName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.components.is_empty() {
            write!(f, "{}", self.components[0])?;
        }

        for part in self.components[1..].iter() {
            write!(f, ".{}", part)?;
        }
        Ok(())
    }
}
