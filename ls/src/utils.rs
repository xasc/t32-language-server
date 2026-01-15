// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{cmp::Ordering, convert::From, ops::Range};

#[cfg(test)]
use std::path;

use tree_sitter::Range as TRange;

#[cfg(test)]
use url::Url;

#[cfg(test)]
use tree_sitter::Tree;

#[cfg(test)]
use crate::{
    ls::{FileIndex, TextDoc, TextDocs, index_files, read_doc},
    protocol::Uri,
    t32::LangExpressions,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BRange(Range<usize>);

impl BRange {
    pub fn to_inner(self) -> Range<usize> {
        self.0
    }

    pub fn inner(&self) -> &Range<usize> {
        &self.0
    }

    pub fn contains(&self, offset: &usize) -> bool {
        self.0.contains(offset)
    }
}

impl From<TRange> for BRange {
    fn from(span: TRange) -> Self {
        BRange(Range {
            start: span.start_byte,
            end: span.end_byte,
        })
    }
}

impl From<Range<usize>> for BRange {
    fn from(span: Range<usize>) -> Self {
        BRange(span)
    }
}

impl From<BRange> for Range<usize> {
    fn from(span: BRange) -> Self {
        span.0
    }
}

impl Eq for BRange {}

impl Ord for BRange {
    fn cmp(&self, other: &Self) -> Ordering {
        let Self(inner) = self;
        let Self(other_inner) = other;

        if inner.start > other_inner.start || inner.end > other_inner.end {
            Ordering::Greater
        } else if inner.start < other_inner.start || inner.end < other_inner.end {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    }
}

impl PartialEq<Range<usize>> for BRange {
    fn eq(&self, other: &Range<usize>) -> bool {
        let Self(range) = self;
        *range == *other
    }
}

impl PartialOrd for BRange {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
pub fn files() -> Vec<Url> {
    vec![
        Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/orphan.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/same.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/a/same.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/a/d/d.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/a/d/d.cmmt").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/b/b.cmm").expect("File must exist."))
            .unwrap(),
        Url::from_file_path(path::absolute("tests/samples/b/same.cmm").expect("File must exist."))
            .unwrap(),
    ]
}

#[cfg(test)]
pub fn to_file_uri(file: &str) -> Uri {
    Url::from_file_path(path::absolute(file).expect("File must exist."))
        .unwrap()
        .to_string()
}

#[cfg(test)]
pub fn create_file_idx() -> FileIndex {
    let files = files();
    index_files(files)
}

#[cfg(test)]
pub fn create_doc_store(files: &Vec<Url>, index: &FileIndex) -> TextDocs {
    let mut members: Vec<(TextDoc, Tree, LangExpressions)> = Vec::new();
    for uri in files {
        let (doc, tree, expr) = read_doc(uri.clone(), index.clone()).expect("Must not fail.");
        members.push((doc, tree, expr));
    }
    TextDocs::from_workspace(members)
}
