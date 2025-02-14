// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;

use tree_sitter::{Tree, InputEdit};

use crate::{
    protocol::{Position, Range, TextDocumentContentChangeEvent, TextDocumentItem},
    t32::{self, LANGUAGE_ID},
};

#[derive(Debug, PartialEq)]
pub enum TextDocStatus {
    Closed = 0,
    Open = 1,
}

struct DocIndex(TextDocStatus, usize);

struct FreeLists {
    open: Vec<usize>,
    closed: Vec<usize>,
}

struct DocStore {
    open: Vec<Option<TextDoc>>,
    closed: Vec<Option<TextDoc>>,
}

struct TreeStore {
    open: Vec<Option<Tree>>,
    closed: Vec<Option<Tree>>,
}

pub struct TextDocs {
    docs: DocStore,
    trees: TreeStore,
    registry: BTreeMap<String, DocIndex>,
    free_list: FreeLists,
}

#[derive(Clone, Debug)]
pub struct TextDoc {
    pub uri: String,
    pub lang_id: String,
    pub text: String,
    pub version: i64,
    pub lines: LineMap,
}

#[derive(Clone, Debug, PartialEq)]
struct LineMap {
    byte_offsets: Vec<usize>,
    max_utf16_char_offset: Vec<Option<u32>>,
}

impl From<TextDocumentItem> for TextDoc {
    fn from(item: TextDocumentItem) -> Self {
        let lines = create_line_map_for_text(&item.text, None);

        TextDoc {
            uri: item.uri,
            lang_id: LANGUAGE_ID.to_string(),
            version: item.version,
            text: item.text,
            lines,
        }
    }
}

impl TextDoc {
    pub fn parse(&self) -> Tree {
        t32::parse(self.text.as_bytes(), None)
    }

    pub fn update(&mut self, change: &Range, new: &str) -> InputEdit {
        debug_assert_ne!(change.start, change.end);

        let mut start = self.get_byte_offset_at(&change.start);
        let mut end = self.get_byte_offset_at(&change.end);

        if end < start {
            (end, start) = (start, end);
        }
        self.text.replace_range(start..end, new);
    }

    fn get_byte_offset_at(&self, spot: &Position) -> usize {
        let spot = normalize_position(spot, &self.lines);
        if spot.line >= self.lines.byte_offsets.len() as u32 {
            return self.text.len();
        }

        let mut offset = self.lines.byte_offsets[spot.line as usize];
        if spot.character == 0 {
            return offset;
        }

        let mut num_inline_code_units = 0;
        for ch in self.text[offset..].chars() {
            offset += ch.len_utf8();
            num_inline_code_units += ch.len_utf16();

            if num_inline_code_units >= spot.character as usize {
                break;
            }
        }
        offset
    }
}

impl TextDocs {
    pub fn build() -> Self {
        TextDocs {
            docs: DocStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            trees: TreeStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            registry: BTreeMap::new(),
            free_list: FreeLists {
                open: Vec::new(),
                closed: Vec::new(),
            },
        }
    }

    pub fn add(&mut self, doc: TextDoc, tree: Tree, status: TextDocStatus) {
        if let Some(val) = self.registry.get(&doc.uri) {
            if val.0 == status {
                match status {
                    TextDocStatus::Open => {
                        self.docs.open[val.1] = Some(doc);
                        self.trees.open[val.1] = Some(tree);
                    }
                    TextDocStatus::Closed => {
                        self.docs.closed[val.1] = Some(doc);
                        self.trees.closed[val.1] = Some(tree);
                    }
                }
                return;
            } else {
                match val.0 {
                    TextDocStatus::Open => {
                        self.docs.open[val.1] = None;
                        self.trees.open[val.1] = None;

                        self.free_list.open.push(val.1);
                    }
                    TextDocStatus::Closed => {
                        self.docs.closed[val.1] = None;
                        self.trees.closed[val.1] = None;

                        self.free_list.closed.push(val.1);
                    }
                }
                self.registry.remove(&doc.uri);
            }
        }

        let uri = doc.uri.clone();
        match status {
            TextDocStatus::Open => {
                if self.free_list.open.is_empty() {
                    let len = self.docs.open.len();

                    self.docs.open.push(Some(doc));
                    self.trees.open.push(Some(tree));

                    self.registry.insert(uri, DocIndex(status, len));
                } else {
                    let slot = self.free_list.open.pop().unwrap();

                    self.docs.open[slot] = Some(doc);
                    self.trees.open[slot] = Some(tree);

                    self.registry.insert(uri, DocIndex(status, slot));
                }
            }
            TextDocStatus::Closed => {
                if self.free_list.closed.is_empty() {
                    let len = self.docs.closed.len();

                    self.docs.closed.push(Some(doc));
                    self.trees.closed.push(Some(tree));

                    self.registry.insert(uri, DocIndex(status, len));
                } else {
                    let slot = self.free_list.closed.pop().unwrap();

                    self.docs.closed[slot] = Some(doc);
                    self.trees.closed[slot] = Some(tree);

                    self.registry.insert(uri, DocIndex(status, slot));
                }
            }
        }
    }

    pub fn get_doc(&self, uri: &str) -> Option<&TextDoc> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.docs.open[idx.1].as_ref(),
            Some(idx) => self.docs.closed[idx.1].as_ref(),
            None => None,
        }
    }

    pub fn get_tree(&self, uri: &str) -> Option<&Tree> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.trees.open[idx.1].as_ref(),
            Some(idx) => self.trees.closed[idx.1].as_ref(),
            None => None,
        }
    }

    pub fn get_doc_and_tree(&self, uri: &str) -> Option<(&TextDoc, &Tree)> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => {
                if self.docs.open[idx.1].is_none() || self.trees.open[idx.1].is_none() {
                    Some((
                        &self.docs.open[idx.1].as_ref().unwrap(),
                        &self.trees.open[idx.1].as_ref().unwrap(),
                    ))
                } else {
                    None
                }
            }
            Some(idx) => {
                if self.docs.closed[idx.1].is_none() || self.trees.closed[idx.1].is_none() {
                    Some((
                        &self.docs.open[idx.1].as_ref().unwrap(),
                        &self.trees.open[idx.1].as_ref().unwrap(),
                    ))
                } else {
                    None
                }
            }
            None => None,
        }
    }

    pub fn is_open(&self, uri: &str) -> bool {
        self.registry.contains_key(uri)
    }
}

pub fn import_doc(r#in: TextDocumentItem) -> (TextDoc, Tree) {
    let doc = TextDoc::from(r#in);
    let tree = doc.parse();

    (doc, tree)
}

pub fn update_doc(mut doc: TextDoc, tree: Tree, changes: Vec<TextDocumentContentChangeEvent>) -> (TextDoc, Tree) {
    for change in changes {
        doc.update(&change.range, &change.text);
    }

    (doc, tree)
}

/// Clients only need to support UTF-16 encoding to character offsets, so
/// this is the common denominator we need to support.
fn create_line_map_for_text(text: &str, bias: Option<usize>) -> LineMap {
    debug_assert!(text.len() > 0);

    const NEWLINE_LEN: usize = '\n'.len_utf8();
    const CARRIAGE_RETURN_LEN: usize = '\r'.len_utf8();

    let bias = bias.unwrap_or(0);

    let mut byte_offsets = vec![bias];
    let mut max_utf16_char_offset: Vec<Option<u32>> = vec![None];

    let mut num_inline_code_units: u32 = 0;
    let mut chars = text.char_indices().peekable();

    while let Some((offset, char)) = chars.next() {
        /* The character offset can never move past the start of the first
         * character of the end-of-line sequence.
         */
        let len = max_utf16_char_offset.len();
        max_utf16_char_offset[len - 1] = Some(num_inline_code_units);

        if char == '\r' {
            max_utf16_char_offset.push(None);
            num_inline_code_units = 0;

            if let Some(&(off, '\n')) = chars.peek() {
                byte_offsets.push(bias + off + NEWLINE_LEN);
                chars.next();
            } else {
                byte_offsets.push(bias + offset + CARRIAGE_RETURN_LEN);
            }
        } else if char == '\n' {
            max_utf16_char_offset.push(None);
            num_inline_code_units = 0;

            byte_offsets.push(bias + offset + NEWLINE_LEN);
        } else {
            num_inline_code_units += char.len_utf16() as u32;
        }
    }

    debug_assert!(byte_offsets.len() == max_utf16_char_offset.len());

    LineMap {
        byte_offsets,
        max_utf16_char_offset,
    }
}

fn normalize_position(spot: &Position, lines: &LineMap) -> Position {
    let num_lines = lines.byte_offsets.len() as u32;

    let Position { mut line, mut character } = spot;
    if spot.line >= num_lines {
        line = num_lines;
        character = 0;
    }
    else if character > lines.max_utf16_char_offset[line as usize].unwrap_or(0) {
        character = lines.max_utf16_char_offset[line as usize].unwrap_or(0);
    }
    Position { line, character }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn uses_bytes_and_utf16_code_units_for_offsets() {
        let text = "a𐐀b";

        let map = create_line_map_for_text(&text, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![0],
                max_utf16_char_offset: vec![Some(3)]
            }
        );
    }

    #[test]
    fn can_calculate_offsets_for_all_eol_variants() {
        let text = "Line 1\nLine 🚀\nline ɣ\n";

        let map = create_line_map_for_text(&text, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    0,
                    "Line 1\n".len(),
                    "Line 1\nLine 🚀\n".len(),
                    "Line 1\nLine 🚀\nline ɣ\n".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(7), Some(6), None]
            }
        );

        let text = "Line 1\rLine 🚀\rline ɣ\r";

        let map = create_line_map_for_text(&text, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    0,
                    "Line 1\r".len(),
                    "Line 1\rLine 🚀\r".len(),
                    "Line 1\rLine 🚀\nline ɣ\r".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(7), Some(6), None]
            }
        );

        let text = "Line 1\r\nLine 🚀\r\nline ɣ\r\n";

        let map = create_line_map_for_text(&text, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    0,
                    "Line 1\r\n".len(),
                    "Line 1\r\nLine 🚀\r\n".len(),
                    "Line 1\r\nLine 🚀\r\nline ɣ\r\n".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(7), Some(6), None]
            }
        );
    }

    #[test]
    fn can_handle_text_not_ending_with_newline() {
        let text = "Line 1\nabcd";

        let map = create_line_map_for_text(&text, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![0, "Line 1\n".len()],
                max_utf16_char_offset: vec![Some(6), Some(3)]
            }
        );
    }

    #[test]
    fn can_shift_byte_offset() {
        let text = "Line A\rLine B\rLine C\r";

        let bias = 52;
        let map = create_line_map_for_text(&text, Some(bias));
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    bias,
                    bias + "Line A\r".len(),
                    bias + "Line A\rLine B\r".len(),
                    bias + "Line A\rLine B\nLine C\r".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(6), Some(6), None]
            }
        );
    }
}
