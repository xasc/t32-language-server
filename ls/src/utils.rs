// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{cmp::Ordering, convert::From, ops::Range};

use tree_sitter::Range as TRange;

#[derive(Clone, Debug, PartialEq)]
pub struct BRange(Range<usize>);

impl BRange {
    pub fn to_inner(self) -> Range<usize> {
        self.0
    }

    pub fn inner(&self) -> &Range<usize> {
        &self.0
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
