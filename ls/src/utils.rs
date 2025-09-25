// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{convert::From, ops::Range};

use tree_sitter::Range as TRange;

pub struct BRange(Range<usize>);

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
        BRange(Range {
            start: span.start,
            end: span.end,
        })
    }
}

impl From<BRange> for Range<usize> {
    fn from(span: BRange) -> Self {
        span.0
    }
}
