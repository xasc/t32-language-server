// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::fmt;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PracticeFuncParameterPattern {
    Angled(String),
    Braced(Box<PracticeFuncParameterPattern>),
    Choice(Vec<PracticeFuncParameterPattern>),
    Chained(Vec<PracticeFuncParameterPattern>),
    Ellipsis(Vec<PracticeFuncParameterPattern>),

    Literal(String),

    Optional(Vec<PracticeFuncParameterPattern>),
    OptionalOtherParameter(Vec<PracticeFuncParameterPattern>),
    Segmented(Vec<PracticeFuncParameterPattern>),
    Quoted(Box<PracticeFuncParameterPattern>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PracticeFunctionNamePattern {
    Angled(String),
    Chained(Vec<PracticeFunctionNamePattern>),
    Literal(String),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PracticeFunctionNamePickled {
    Sequence(Vec<PracticeFunctionNamePattern>),
}

#[derive(Debug)]
pub struct PracticeFunctionNameIterator<'a> {
    first: Option<&'a PracticeFunctionNamePattern>,
    rest: &'a [PracticeFunctionNamePattern],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PracticeFunctionName {
    pub first: PracticeFunctionNamePattern,
    pub rest: Vec<PracticeFunctionNamePattern>,
}

#[derive(Debug)]
pub struct PracticeFunctionDefinition {
    pub name: PracticeFunctionName,
    pub params: PracticeFunctionParameters,
    pub operation: String,
    pub copyright: u32,
    pub source: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PracticeFunctionDefinitionPickled {
    pub name: PracticeFunctionNamePickled,
    pub params: PracticeFunctionParameters,
    pub operation: String,
    pub copyright: u32,
    pub source: u32,
}

#[derive(Debug)]
pub struct UserGuideData {
    pub copyrights: CopyrightFields,
    pub docs: UserGuideDocs,
    pub functions: Vec<PracticeFunctionDefinition>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UserGuideDataPickled {
    pub copyrights: CopyrightFields,
    pub docs: UserGuideDocs,
    pub functions: Vec<PracticeFunctionDefinitionPickled>,
}

pub type CopyrightFields = Vec<String>;
pub type PracticeFunctionParameters = Vec<PracticeFuncParameterPattern>;
pub type UserGuideDocs = Vec<String>;

pub const DATFILE_FIELD_WIDTH_BITS_TOTAL: usize = size_of::<u64>();
pub const DATFILE_FIELD_WIDTH_NODES_TOTAL: usize = size_of::<u16>();
pub const DATFILE_FIELD_WIDTH_NODE_TAG: usize = size_of::<u8>();
pub const DATFILE_FIELD_WIDTH_NODE_CHAR: usize = size_of::<u32>();
pub const DATFILE_FIELD_WIDTH_SUCCESSOR: usize = size_of::<u16>();

#[macro_export]
macro_rules! funcname {
    ($n:expr) => {
        $crate::PracticeFunctionName {
            first: $n,
            rest: Vec::new(),
        }
    };
    ($n0:expr, $($n1:expr),* $(,)?) => {
        $crate::PracticeFunctionName {
            first: $n0,
            rest: vec![$($n1), *],
        }
    };
}

impl PracticeFunctionName {
    pub fn from_parts(parts: &[PracticeFunctionNamePattern]) -> Option<Self> {
        if let Some((first, rest)) = parts.split_first() {
            Some(Self {
                first: first.clone(),
                rest: rest.to_vec(),
            })
        } else {
            None
        }
    }

    pub fn to_vec(mut self) -> Vec<PracticeFunctionNamePattern> {
        let mut all = vec![self.first];
        all.append(&mut self.rest);
        all
    }

    pub fn has_wildcard(&self) -> bool {
        for part in self.iter() {
            if let PracticeFunctionNamePattern::Literal(_) = part {
                continue;
            }
            return true;
        }
        return false;
    }
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn iter<'a>(&'a self) -> PracticeFunctionNameIterator<'a> {
        PracticeFunctionNameIterator {
            first: Some(&self.first),
            rest: &self.rest,
        }
    }

    pub fn num_parts(&self) -> usize {
        1 + self.rest.len()
    }
    pub fn push(&mut self, new: PracticeFunctionNamePattern) {
        self.rest.push(new);
    }
    pub fn split_first(&self) -> (&PracticeFunctionNamePattern, &[PracticeFunctionNamePattern]) {
        (&self.first, &self.rest)
    }

    pub fn strlen(&self) -> usize {
        let mut iter = self.iter();

        let mut len: usize = iter.next().expect("Cannot be empty").strlen();

        for part in iter {
            len += 1;
            len += part.strlen();
        }
        len
    }

    pub fn to_string(&self) -> String {
        let mut iter = self.iter();

        let mut out: String = iter.next().expect("Cannot be empty.").to_string();

        for part in iter {
            out += ".";
            out += &part.to_string();
        }
        out
    }
}

impl PracticeFunctionNamePattern {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Literal(c) => c.is_empty(),
            Self::Angled(_) | Self::Chained(_) => false,
        }
    }

    pub fn strlen(&self) -> usize {
        match self {
            Self::Angled(c) | Self::Literal(c) => c.len(),
            Self::Chained(seq) => seq
                .iter()
                .map(|p| p.strlen())
                .reduce(|sum, len| sum + len)
                .expect("Must not be empty."),
        }
    }
}

impl UserGuideData {
    pub fn new() -> Self {
        UserGuideData {
            copyrights: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        }
    }
}

impl fmt::Display for PracticeFuncParameterPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PracticeFuncParameterPattern::Angled(p) => write!(f, "<{}>", p),
            PracticeFuncParameterPattern::Quoted(p) => write!(f, "\"{}\"", p),
            PracticeFuncParameterPattern::Optional(p) => {
                write!(f, "[{}", p[0])?;
                for arg in p[1..].iter() {
                    match arg {
                        PracticeFuncParameterPattern::OptionalOtherParameter(_) => {
                            write!(f, "{}", arg)?
                        }
                        _ => write!(f, ", {}", arg)?,
                    }
                }
                write!(f, "]")
            }
            PracticeFuncParameterPattern::OptionalOtherParameter(p) => {
                write!(f, "[, {}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, ", {}", arg)?;
                }
                write!(f, "]")
            }
            PracticeFuncParameterPattern::Choice(p) => {
                write!(f, "{}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, " | {}", arg)?;
                }
                Ok(())
            }
            PracticeFuncParameterPattern::Braced(p) => write!(f, "{{{}}}", p),
            PracticeFuncParameterPattern::Literal(p) => write!(f, "{}", p),
            PracticeFuncParameterPattern::Segmented(p) => {
                write!(f, "{}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, ".{}", arg)?;
                }
                Ok(())
            }
            PracticeFuncParameterPattern::Chained(p) => {
                for arg in p.iter() {
                    write!(f, "{}", arg)?
                }
                Ok(())
            }
            PracticeFuncParameterPattern::Ellipsis(p) => {
                write!(f, "{}", p[0])?;
                for arg in p[1..].iter() {
                    write!(f, " .. {}", arg)?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for PracticeFunctionNamePattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PracticeFunctionNamePattern::Literal(p) => write!(f, "{}", p),
            PracticeFunctionNamePattern::Angled(p) => write!(f, "<{}>", p),
            PracticeFunctionNamePattern::Chained(p) => {
                for seg in p {
                    write!(f, "{}", seg)?;
                }
                Ok(())
            }
        }
    }
}

impl From<PracticeFunctionDefinition> for PracticeFunctionDefinitionPickled {
    fn from(def: PracticeFunctionDefinition) -> Self {
        PracticeFunctionDefinitionPickled {
            name: def.name.into(),
            params: def.params,
            operation: def.operation,
            copyright: def.copyright,
            source: def.source,
        }
    }
}

impl From<PracticeFunctionName> for PracticeFunctionNamePickled {
    fn from(name: PracticeFunctionName) -> Self {
        PracticeFunctionNamePickled::Sequence(name.to_vec())
    }
}

impl From<UserGuideData> for UserGuideDataPickled {
    fn from(ug: UserGuideData) -> Self {
        let mut funcs: Vec<PracticeFunctionDefinitionPickled> =
            Vec::with_capacity(ug.functions.len());
        for def in ug.functions {
            funcs.push(def.into());
        }

        UserGuideDataPickled {
            copyrights: ug.copyrights,
            docs: ug.docs,
            functions: funcs,
        }
    }
}

impl std::ops::Index<usize> for PracticeFunctionName {
    type Output = PracticeFunctionNamePattern;

    fn index(&self, index: usize) -> &Self::Output {
        if index == 0 {
            &self.first
        } else {
            &self.rest[index - 1]
        }
    }
}

impl IntoIterator for PracticeFunctionName {
    type Item = PracticeFunctionNamePattern;
    type IntoIter = std::iter::Chain<
        std::iter::Once<PracticeFunctionNamePattern>,
        std::vec::IntoIter<PracticeFunctionNamePattern>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(self.first).into_iter().chain(self.rest)
    }
}

impl<'a> IntoIterator for &'a PracticeFunctionName {
    type Item = &'a PracticeFunctionNamePattern;
    type IntoIter = PracticeFunctionNameIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PracticeFunctionNameIterator {
            first: Some(&self.first),
            rest: &self.rest,
        }
    }
}

impl<'a> Iterator for PracticeFunctionNameIterator<'a> {
    type Item = &'a PracticeFunctionNamePattern;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(n) = self.first.take() {
            Some(n)
        } else if let Some((first, rest)) = self.rest.split_first() {
            self.rest = rest;
            Some(first)
        } else {
            None
        }
    }
}

impl PartialEq for PracticeFunctionNamePattern {
    fn eq(&self, other: &Self) -> bool {
        match self {
            PracticeFunctionNamePattern::Literal(s) => {
                if let PracticeFunctionNamePattern::Literal(o) = other {
                    *s == *o
                } else {
                    false
                }
            }
            PracticeFunctionNamePattern::Angled(s) => {
                if let PracticeFunctionNamePattern::Angled(o) = other {
                    *s == *o
                } else {
                    false
                }
            }
            PracticeFunctionNamePattern::Chained(s) => {
                if let PracticeFunctionNamePattern::Chained(o) = other {
                    *s == *o
                } else {
                    false
                }
            }
        }
    }
}

pub fn format_practice_func_args(args: &PracticeFunctionParameters) -> String {
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
            PracticeFuncParameterPattern::OptionalOtherParameter(_) => out += &arg.to_string(),
            _ => out += &format!(", {}", arg),
        }
    }
    out
}

pub fn format_practice_func_name(name: &PracticeFunctionName) -> String {
    let mut iter = name.iter();

    let mut out: String = iter.next().expect("Cannot be empty.").to_string();

    for part in iter {
        out += ".";
        out += &part.to_string();
    }
    out
}
