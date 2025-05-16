// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{collections::HashMap, fs};

use url::Url;

use tree_sitter::{InputEdit, Point, Tree};

use crate::{
    ReturnCode,
    ls::workspace::FileIndex,
    protocol::{Position, Range, TextDocumentContentChangeEvent, TextDocumentItem, Uri},
    t32::{
        self, CallExpression, CallExpressions, CallLocations, LANGUAGE_ID, LangExpressions,
        SubscriptCalls, find_call_expressions, find_global_macro_definitions, find_subroutines,
        resolve_subscript_call_targets,
    },
};

#[derive(Debug, PartialEq)]
pub enum TextDocStatus {
    Closed = 0,
    Open = 1,
}

struct DocIndex(TextDocStatus, usize);

pub struct TextDocs {
    docs: DocStore,
    trees: TreeStore,
    file_idx: FileIndex,

    #[allow(dead_code)]
    t32: WaypointStore,

    registry: HashMap<Uri, DocIndex>,
    free_list: FreeLists,
}

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

#[allow(dead_code)]
struct WaypointStore {
    open: Vec<Option<LangExpressions>>,
    closed: Vec<Option<LangExpressions>>,
}

#[derive(Clone, Debug)]
pub struct TextDoc {
    pub uri: Uri,
    pub lang_id: String,
    pub text: String,
    pub version: i64,
    pub lines: LineMap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineMap {
    byte_offsets: Vec<usize>,
    max_utf16_char_offset: Vec<Option<u32>>,
    num_bytes: usize,
}

impl From<TextDocumentItem> for TextDoc {
    fn from(item: TextDocumentItem) -> Self {
        let lines = create_line_map_for_text(&item.text, None, None);

        TextDoc {
            uri: item.uri,
            lang_id: LANGUAGE_ID.to_string(),
            version: item.version,
            text: item.text,
            lines,
        }
    }
}

impl TryFrom<Url> for TextDoc {
    type Error = ReturnCode;

    fn try_from(item: Url) -> Result<Self, ReturnCode> {
        let path = item.to_file_path().expect("Path must be valid.");

        let text = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(_) => return Err(ReturnCode::IoErr),
        };
        let lines = create_line_map_for_text(&text, None, None);

        Ok(TextDoc {
            uri: item.into(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 0,
            text,
            lines,
        })
    }
}

impl TextDoc {
    pub fn update(&mut self, change: Range, new: &str) -> InputEdit {
        let range = make_range_well_formed_range(change, &self.lines, &self.text);

        let start_byte = self.get_byte_offset_at(&range.start);
        let end_byte = self.get_byte_offset_at(&range.end);

        self.text.replace_range(start_byte..end_byte, new);

        let start_pos = Point {
            row: range.start.line as usize,
            column: self.get_column_offset_at(&range.start),
        };
        let end_pos = Point {
            row: range.end.line as usize,
            column: self.get_column_offset_at(&range.end),
        };

        let mut edit = InputEdit {
            start_byte,
            old_end_byte: end_byte,
            new_end_byte: start_byte + new.len(),
            start_position: start_pos,
            old_end_position: end_pos,
            new_end_position: Point {
                row: usize::MAX,
                column: usize::MAX,
            },
        };

        self.update_line_map(&edit);

        let end_line = self.lines.get_line_with_offset(edit.new_end_byte);
        let end_column: usize = edit.new_end_byte - self.lines.byte_offsets[end_line];

        edit.new_end_position = Point {
            row: end_line,
            column: end_column,
        };
        edit
    }

    pub fn to_byte_offset(&self, spot: &Position) -> usize {
        let spot = normalize_position(spot, &self.lines, &self.text);
        self.get_byte_offset_at(&spot)
    }

    pub fn to_position(&self, offset: usize) -> Position {
        if offset >= self.text.len() {
            return Position {
                line: self.lines.byte_offsets.len() as u32,
                character: 0,
            };
        }
        self.get_character_offset_at(offset)
    }

    pub fn to_range(&self, start: usize, end: usize) -> Range {
        let span: (usize, usize);
        if start <= end {
            span = (start, end);
        } else {
            span = (end, start);
        }

        Range {
            start: self.to_position(span.0),
            end: self.to_position(span.1),
        }
    }

    fn get_byte_offset_at(&self, spot: &Position) -> usize {
        if spot.line >= self.lines.byte_offsets.len() as u32 {
            return self.text.len();
        }

        let mut offset = self.lines.byte_offsets[spot.line as usize];
        if spot.character == 0 {
            return offset;
        }

        let mut num_utf16_code_units = 0;
        for ch in self.text[offset..].chars() {
            if num_utf16_code_units >= spot.character as usize {
                break;
            }
            offset += ch.len_utf8();
            num_utf16_code_units += ch.len_utf16();
        }
        offset
    }

    fn get_character_offset_at(&self, spot: usize) -> Position {
        debug_assert!(spot < self.text.len());

        let line = self.lines.get_line_with_offset(spot);
        if line >= self.lines.byte_offsets.len() {
            return Position {
                line: line as u32,
                character: 0,
            };
        }

        let mut offset = self.lines.byte_offsets[line];
        if spot == offset {
            return Position {
                line: line as u32,
                character: 0,
            };
        }

        let mut num_utf16_code_units: usize = 0;

        for ch in self.text[offset..].chars() {
            if offset >= spot {
                break;
            }
            offset += ch.len_utf8();
            num_utf16_code_units += ch.len_utf16();
        }
        Position {
            line: line as u32,
            character: num_utf16_code_units as u32,
        }
    }

    /// Tree-sitter measures columns in bytes.
    fn get_column_offset_at(&self, spot: &Position) -> usize {
        if spot.character == 0 || spot.line >= (self.lines.byte_offsets.len() as u32) {
            return 0;
        }
        let mut column: usize = 0;
        let mut num_utf16_code_units: usize = 0;

        let offset = self.lines.byte_offsets[spot.line as usize];
        for ch in self.text[offset..].chars() {
            if num_utf16_code_units >= spot.character as usize {
                break;
            }
            num_utf16_code_units += ch.len_utf16();
            column += ch.len_utf8();
        }
        column
    }

    fn update_line_map(&mut self, edit: &InputEdit) {
        // We extend the changed text section on each side to the closest line
        // border and recalculate the line offsets only for this modified section.
        // Afterwards we can create the updated line table by inserting the new
        // segment into the existing one and adjusting the offsets accordingly.
        let start_mod_lines = self.lines.get_line_with_offset(edit.start_byte);
        let start_mod_bytes = self.lines.byte_offsets[start_mod_lines];

        let start_unmod_lines = (edit.old_end_position.row + 1).min(self.lines.byte_offsets.len());
        let cutoff = edit.new_end_byte;

        update_line_map_from_text_segment(
            start_mod_bytes,
            start_mod_lines,
            start_unmod_lines,
            cutoff,
            &self.text,
            &mut self.lines,
        );
    }
}

impl LineMap {
    fn align_with_character_border(&self, spot: &Position, text: &str) -> u32 {
        debug_assert_ne!(spot.character, 0);
        debug_assert_ne!(
            spot.character,
            self.max_utf16_char_offset[spot.line as usize].unwrap()
        );

        let offset = self.byte_offsets[spot.line as usize];

        let mut num_utf16_code_units: u32 = 0;
        for ch in text[offset..].chars() {
            let len = ch.len_utf16() as u32;
            if num_utf16_code_units + len > spot.character {
                break;
            }
            num_utf16_code_units += len;
        }
        num_utf16_code_units
    }

    pub fn get_line_with_offset(&self, offset: usize) -> usize {
        let mut left: usize = 0;
        let mut right: usize = self.byte_offsets.len();

        while left < right {
            let idx = (left >> 1) + (right >> 1);

            if self.byte_offsets[idx] > offset {
                right = idx;
            } else {
                left = idx + 1;
            }
        }

        if right > 0 {
            right - 1
        } else {
            unreachable!("First element must be smaller or equal to the offset we are looking for.")
        }
    }
}

impl TextDocs {
    pub fn new(files: FileIndex) -> Self {
        TextDocs {
            docs: DocStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            trees: TreeStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            t32: WaypointStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            registry: HashMap::new(),
            free_list: FreeLists {
                open: Vec::new(),
                closed: Vec::new(),
            },
            file_idx: files,
        }
    }

    pub fn add(&mut self, doc: TextDoc, tree: Tree, expr: LangExpressions, status: TextDocStatus) {
        if let Some(val) = self.registry.get(&doc.uri) {
            if val.0 == status {
                match status {
                    TextDocStatus::Open => {
                        debug_assert_eq!(
                            self.docs.open[val.1].as_ref().unwrap().lang_id,
                            doc.lang_id
                        );

                        self.docs.open[val.1] = Some(doc);
                        self.trees.open[val.1] = Some(tree);
                        self.t32.open[val.1] = Some(expr);
                    }
                    TextDocStatus::Closed => {
                        self.docs.closed[val.1] = Some(doc);
                        self.trees.closed[val.1] = Some(tree);
                        self.t32.closed[val.1] = Some(expr);
                    }
                }
                return;
            } else {
                match val.0 {
                    TextDocStatus::Open => {
                        self.docs.open[val.1] = None;
                        self.trees.open[val.1] = None;
                        self.t32.open[val.1] = None;

                        self.free_list.open.push(val.1);
                    }
                    TextDocStatus::Closed => {
                        self.docs.closed[val.1] = None;
                        self.trees.closed[val.1] = None;
                        self.t32.closed[val.1] = None;

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
                    self.t32.open.push(Some(expr));

                    self.registry.insert(uri, DocIndex(status, len));
                } else {
                    let slot = self.free_list.open.pop().unwrap();

                    self.docs.open[slot] = Some(doc);
                    self.trees.open[slot] = Some(tree);
                    self.t32.open[slot] = Some(expr);

                    self.registry.insert(uri, DocIndex(status, slot));
                }
            }
            TextDocStatus::Closed => {
                if self.free_list.closed.is_empty() {
                    let len = self.docs.closed.len();

                    self.docs.closed.push(Some(doc));
                    self.trees.closed.push(Some(tree));
                    self.t32.closed.push(Some(expr));

                    self.registry.insert(uri, DocIndex(status, len));
                } else {
                    let slot = self.free_list.closed.pop().unwrap();

                    self.docs.closed[slot] = Some(doc);
                    self.trees.closed[slot] = Some(tree);
                    self.t32.closed[slot] = Some(expr);

                    self.registry.insert(uri, DocIndex(status, slot));
                }
            }
        }
    }

    pub fn update(&mut self, doc: TextDoc, tree: Tree, expr: LangExpressions) {
        if let Some(val) = self.registry.get(&doc.uri) {
            debug_assert_eq!(val.0, TextDocStatus::Open);

            match val.0 {
                TextDocStatus::Open => {
                    debug_assert_eq!(self.docs.open[val.1].as_ref().unwrap().uri, doc.uri);

                    self.docs.open[val.1] = Some(doc);
                    self.trees.open[val.1] = Some(tree);
                    self.t32.open[val.1] = Some(expr);
                }
                TextDocStatus::Closed => {
                    debug_assert_eq!(self.docs.open[val.1].as_ref().unwrap().uri, doc.uri);

                    self.docs.closed[val.1] = Some(doc);
                    self.trees.closed[val.1] = Some(tree);
                    self.t32.closed[val.1] = Some(expr);
                }
            }
            return;
        } else {
            unreachable!("Docs that are updated must already be present.");
        }
    }

    pub fn close(&mut self, uri: &str) {
        debug_assert!(self.is_open(uri));

        let &DocIndex(_, idx) = self
            .registry
            .get(uri)
            .expect("Doc must already be present.");

        let doc = self.docs.open[idx].take().unwrap();
        let tree = self.trees.open[idx].take().unwrap();
        let globals = self.t32.open[idx].take().unwrap();

        self.free_list.open.push(idx);
        self.registry.remove(uri);

        if self.free_list.closed.is_empty() {
            let len = self.docs.closed.len();

            self.docs.closed.push(Some(doc));
            self.trees.closed.push(Some(tree));
            self.t32.closed.push(Some(globals));

            self.registry
                .insert(uri.to_string(), DocIndex(TextDocStatus::Closed, len));
        } else {
            let slot = self.free_list.closed.pop().unwrap();

            self.docs.closed[slot] = Some(doc);
            self.trees.closed[slot] = Some(tree);
            self.t32.closed[slot] = Some(globals);

            self.registry
                .insert(uri.to_string(), DocIndex(TextDocStatus::Closed, slot));
        }
    }

    #[allow(dead_code)]
    pub fn get_doc(&self, uri: &str) -> Option<&TextDoc> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.docs.open[idx.1].as_ref(),
            Some(idx) => self.docs.closed[idx.1].as_ref(),
            None => None,
        }
    }

    #[allow(dead_code)]
    pub fn get_tree(&self, uri: &str) -> Option<&Tree> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.trees.open[idx.1].as_ref(),
            Some(idx) => self.trees.closed[idx.1].as_ref(),
            None => None,
        }
    }

    #[allow(dead_code)]
    pub fn get_waypoints(&self, uri: &str) -> Option<&LangExpressions> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.t32.open[idx.1].as_ref(),
            Some(idx) => self.t32.closed[idx.1].as_ref(),
            None => None,
        }
    }

    pub fn get_file_idx(&self) -> &FileIndex {
        &self.file_idx
    }

    pub fn get_doc_data(&self, uri: &str) -> Option<(&TextDoc, &Tree, &LangExpressions)> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => {
                if self.docs.open[idx.1].is_none() || self.trees.open[idx.1].is_none() {
                    Some((
                        &self.docs.open[idx.1].as_ref().unwrap(),
                        &self.trees.open[idx.1].as_ref().unwrap(),
                        &self.t32.open[idx.1].as_ref().unwrap(),
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
                        &self.t32.open[idx.1].as_ref().unwrap(),
                    ))
                } else {
                    None
                }
            }
            None => None,
        }
    }

    pub fn is_open(&self, uri: &str) -> bool {
        let doc = self.registry.get(uri);

        !doc.is_none_or(|d| d.0 == TextDocStatus::Closed)
    }
}

pub fn import_doc(r#in: TextDocumentItem, files: FileIndex) -> (TextDoc, Tree, LangExpressions) {
    let doc = TextDoc::from(r#in);
    let tree = t32::parse(doc.text.as_bytes(), None);

    let macros = find_global_macro_definitions(&doc.text, &tree);
    let subroutines = find_subroutines(&doc.text, &tree);
    let calls = resolve_call_expressions(&doc.text, &tree, &files);

    (
        doc,
        tree,
        LangExpressions {
            macros,
            subroutines,
            calls,
        },
    )
}

pub fn update_doc(
    mut doc: TextDoc,
    mut tree: Tree,
    files: FileIndex,
    changes: Vec<TextDocumentContentChangeEvent>,
) -> (TextDoc, Tree, LangExpressions) {
    for change in changes {
        let edits = doc.update(change.range, &change.text);

        tree.edit(&edits);
        t32::parse(doc.text.as_bytes(), Some(&tree));
    }

    let macros = find_global_macro_definitions(&doc.text, &tree);
    let subroutines = find_subroutines(&doc.text, &tree);
    let calls = resolve_call_expressions(&doc.text, &tree, &files);

    (
        doc,
        tree,
        LangExpressions {
            macros,
            subroutines,
            calls,
        },
    )
}

pub fn read_doc(r#in: Url, files: FileIndex) -> Result<(TextDoc, Tree, LangExpressions), Uri> {
    let uri = r#in.to_string();
    let doc = match TextDoc::try_from(r#in) {
        Ok(text) => text,
        Err(_) => return Err(uri),
    };
    let tree = t32::parse(doc.text.as_bytes(), None);

    let macros = find_global_macro_definitions(&doc.text, &tree);
    let subroutines = find_subroutines(&doc.text, &tree);
    let calls = resolve_call_expressions(&doc.text, &tree, &files);

    Ok((
        doc,
        tree,
        LangExpressions {
            macros,
            subroutines,
            calls,
        },
    ))
}

pub fn resolve_call_expressions(text: &str, tree: &Tree, files: &FileIndex) -> CallExpressions {
    let CallLocations {
        subroutines,
        scripts,
    } = find_call_expressions(text, &tree);

    let subscripts: Option<SubscriptCalls>;
    if scripts.is_some() {
        let scripts = scripts.unwrap();

        let mut locations: Vec<CallExpression> = Vec::with_capacity(scripts.len());
        let mut targets: Vec<Option<Uri>> = Vec::with_capacity(scripts.len());

        for expr in scripts.into_iter() {
            if let Some(calls) =
                resolve_subscript_call_targets(text, &tree, expr.target.start, files)
            {
                for call in calls.into_iter() {
                    locations.push(expr.clone());
                    targets.push(Some(call));
                }
            } else {
                locations.push(expr);
                targets.push(None);
            }
        }
        debug_assert_eq!(targets.len(), locations.len());
        subscripts = Some(SubscriptCalls::build(locations, targets));
    } else {
        subscripts = None;
    }
    CallExpressions::build(subroutines, subscripts)
}

/// Clients only need to support UTF-16 encoding to character offsets, so
/// this is the common denominator we need to support.
fn create_line_map_for_text(text: &str, bias: Option<usize>, cutoff: Option<usize>) -> LineMap {
    const NEWLINE_LEN: usize = '\n'.len_utf8();
    const CARRIAGE_RETURN_LEN: usize = '\r'.len_utf8();

    let bias = bias.unwrap_or(0);
    let cutoff = cutoff.unwrap_or(usize::MAX);

    let mut byte_offsets = vec![bias];
    let mut max_utf16_char_offset: Vec<Option<u32>> = vec![None];
    let mut num_bytes: usize = 0;

    let mut num_inline_code_units: u32 = 0;
    let mut stop_after_eol: bool = false;
    let mut chars = text.char_indices().peekable();

    while let Some((offset, ch)) = chars.next() {
        /* The character offset can never move past the start of the first
         * character of the end-of-line sequence.
         */
        let len = max_utf16_char_offset.len();
        max_utf16_char_offset[len - 1] = Some(num_inline_code_units);

        num_bytes += ch.len_utf8();

        if offset >= cutoff {
            stop_after_eol = true;
        }

        if ch == '\r' {
            if stop_after_eol {
                if let Some(&(_, '\n')) = chars.peek() {
                    num_bytes += NEWLINE_LEN;
                }
                break;
            }
            max_utf16_char_offset.push(None);
            num_inline_code_units = 0;

            if let Some(&(off, '\n')) = chars.peek() {
                num_bytes += NEWLINE_LEN;
                byte_offsets.push(bias + off + NEWLINE_LEN);
                chars.next();
            } else {
                byte_offsets.push(bias + offset + CARRIAGE_RETURN_LEN);
            }
        } else if ch == '\n' {
            if stop_after_eol {
                break;
            }
            max_utf16_char_offset.push(None);
            num_inline_code_units = 0;

            byte_offsets.push(bias + offset + NEWLINE_LEN);
        } else {
            num_inline_code_units += ch.len_utf16() as u32;
        }
    }
    debug_assert!(byte_offsets.len() == max_utf16_char_offset.len());

    LineMap {
        byte_offsets,
        max_utf16_char_offset,
        num_bytes,
    }
}

fn update_line_map_from_text_segment(
    start_byte: usize,
    start_line: usize,
    end_line: usize,
    cutoff: usize,
    text: &str,
    lines: &mut LineMap,
) {
    let mut new_segment =
        create_line_map_for_text(&text[start_byte..], Some(start_byte), Some(cutoff));

    let mut upper = LineMap {
        byte_offsets: Vec::with_capacity(lines.byte_offsets.len() - end_line),
        max_utf16_char_offset: Vec::with_capacity(lines.byte_offsets.len() - end_line),
        num_bytes: 0,
    };

    if upper.num_bytes > 0 {
        for off in lines.byte_offsets.drain(end_line..) {
            upper.byte_offsets.push(new_segment.num_bytes + off);
        }
        for off in lines.max_utf16_char_offset[end_line..].into_iter() {
            upper.max_utf16_char_offset.push(*off);
        }
    }

    drop(lines.byte_offsets.drain(start_line..));
    drop(lines.max_utf16_char_offset.drain(start_line..));

    lines.byte_offsets.append(&mut new_segment.byte_offsets);
    lines
        .max_utf16_char_offset
        .append(&mut new_segment.max_utf16_char_offset);

    if upper.num_bytes > 0 {
        lines.byte_offsets.append(&mut upper.byte_offsets);
        lines
            .max_utf16_char_offset
            .append(&mut upper.max_utf16_char_offset);
    }
    lines.num_bytes = text.len();
}

fn make_range_well_formed_range(range: Range, lines: &LineMap, text: &str) -> Range {
    let range = normalize_range(&range, lines, text);

    if range.end.line < range.start.line
        || (range.end.line == range.start.line && range.end.character < range.start.character)
    {
        Range {
            start: range.end,
            end: range.start,
        }
    } else {
        range
    }
}

fn normalize_range(range: &Range, lines: &LineMap, text: &str) -> Range {
    Range {
        start: normalize_position(&range.start, lines, text),
        end: normalize_position(&range.end, lines, text),
    }
}

fn normalize_position(spot: &Position, lines: &LineMap, text: &str) -> Position {
    let num_lines = lines.byte_offsets.len() as u32;

    let &Position {
        mut line,
        mut character,
    } = spot;

    // If the character offset points past the last character, we do not revert it
    // back to the last character, as the LSP specification states. Instead, we
    // move it to the first position of the next line.
    // It is still a reasonable interpretation for a sequence of characters and
    // allows us to create a range that only includes the last character in a string
    // and to have an empty range for appending to new text after the end of the
    // string.
    let max_char_offset = lines.max_utf16_char_offset[line as usize].unwrap_or(0);
    if spot.line >= num_lines {
        if text_ends_with_eol(&lines) {
            line = num_lines;
            character = 0;
        } else {
            line = num_lines - 1;
            character = lines.max_utf16_char_offset[(num_lines - 1) as usize].unwrap();
        }
    } else if character > max_char_offset {
        if text_ends_with_eol(&lines) {
            line += 1;
            character = 0;
        } else {
            character = lines.max_utf16_char_offset[(num_lines - 1) as usize].unwrap() + 1;
        }
    } else if character > 0 && character < max_char_offset {
        character = lines.align_with_character_border(&spot, text);
    }
    Position { line, character }
}

fn text_ends_with_eol(lines: &LineMap) -> bool {
    lines.max_utf16_char_offset[lines.max_utf16_char_offset.len() - 1].is_none()
}

#[cfg(test)]
mod test {
    use std::path;

    use super::*;

    use crate::{
        ls::workspace::index_files,
        t32::{CallExpressions, MacroDefinitions},
    };

    fn create_doc(uri: &str) -> (TextDoc, Tree, LangExpressions) {
        let text = "PRINT \"Hello, World!\"\n";
        let lines = create_line_map_for_text(&text, None, None);
        let doc = TextDoc {
            uri: uri.to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };
        let tree = t32::parse(text.as_bytes(), None);

        let macros = find_global_macro_definitions(&doc.text, &tree);
        let subroutines = find_subroutines(&doc.text, &tree);
        let calls = CallExpressions {
            subroutines: None,
            scripts: None,
        };

        (
            doc,
            tree,
            LangExpressions {
                macros,
                subroutines,
                calls,
            },
        )
    }

    fn assert_file_in_subscript_calls(file: &str, subscripts: &SubscriptCalls) {
        assert!(
            subscripts
                .targets
                .iter()
                .find_map(|dst| dst.clone().is_some_and(|d| d == file).then_some(()))
                .is_some()
        );
    }

    fn assert_file_not_in_subscript_calls(file: &str, subscripts: &SubscriptCalls) {
        assert!(
            subscripts
                .targets
                .iter()
                .find_map(|dst| dst.clone().is_some_and(|d| d == file).then_some(()))
                .is_none()
        );
    }

    fn create_file_idx() -> FileIndex {
        let files: Vec<Url> = vec![
            Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
                .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/same.cmm").expect("File must exist."),
            )
            .unwrap(),
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/a/same.cmm").expect("File must exist."),
            )
            .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/a/d/d.cmmt").expect("File must exist."),
            )
            .unwrap(),
            Url::from_file_path(path::absolute("tests/samples/b/b.cmm").expect("File must exist."))
                .unwrap(),
            Url::from_file_path(
                path::absolute("tests/samples/b/same.cmm").expect("File must exist."),
            )
            .unwrap(),
        ];
        index_files(files)
    }

    #[test]
    fn uses_bytes_and_utf16_code_units_for_offsets() {
        let text = "a𐐀b";

        let map = create_line_map_for_text(&text, None, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![0],
                max_utf16_char_offset: vec![Some(3)],
                num_bytes: text.len(),
            }
        );
    }

    #[test]
    fn can_calculate_offsets_for_all_eol_variants() {
        let text = "Line 1\nLine 🚀\nline ɣ\n";

        let map = create_line_map_for_text(&text, None, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    0,
                    "Line 1\n".len(),
                    "Line 1\nLine 🚀\n".len(),
                    "Line 1\nLine 🚀\nline ɣ\n".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(7), Some(6), None],
                num_bytes: text.len(),
            }
        );

        let text = "Line 1\rLine 🚀\rline ɣ\r";

        let map = create_line_map_for_text(&text, None, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    0,
                    "Line 1\r".len(),
                    "Line 1\rLine 🚀\r".len(),
                    "Line 1\rLine 🚀\nline ɣ\r".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(7), Some(6), None],
                num_bytes: text.len(),
            }
        );

        let text = "Line 1\r\nLine 🚀\r\nline ɣ\r\n";

        let map = create_line_map_for_text(&text, None, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    0,
                    "Line 1\r\n".len(),
                    "Line 1\r\nLine 🚀\r\n".len(),
                    "Line 1\r\nLine 🚀\r\nline ɣ\r\n".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(7), Some(6), None],
                num_bytes: text.len(),
            }
        );
    }

    #[test]
    fn can_handle_text_not_ending_with_newline() {
        let text = "Line 1\nabcd";

        let map = create_line_map_for_text(&text, None, None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![0, "Line 1\n".len()],
                max_utf16_char_offset: vec![Some(6), Some(3)],
                num_bytes: text.len(),
            }
        );
    }

    #[test]
    fn can_shift_byte_offset() {
        let text = "Line A\rLine B\rLine C\r";

        let bias = 52;
        let map = create_line_map_for_text(&text, Some(bias), None);
        assert_eq!(
            map,
            LineMap {
                byte_offsets: vec![
                    bias,
                    bias + "Line A\r".len(),
                    bias + "Line A\rLine B\r".len(),
                    bias + "Line A\rLine B\nLine C\r".len()
                ],
                max_utf16_char_offset: vec![Some(6), Some(6), Some(6), None],
                num_bytes: text.len(),
            }
        );
    }

    #[test]
    fn can_perform_incremenal_text_update() {
        let text = "fn test() {}";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 0,
                    character: 8,
                },
                end: Position {
                    line: 0,
                    character: 8,
                },
            },
            &"a: u32",
        );

        assert_eq!(
            delta,
            InputEdit {
                start_byte: 8,
                old_end_byte: 8,
                new_end_byte: 14,
                start_position: Point::new(0, 8),
                old_end_position: Point::new(0, 8),
                new_end_position: Point::new(0, 14),
            }
        );
        assert_eq!(doc.text, "fn test(a: u32) {}");

        let text = "Line 1\r\nLine 2\r\nLine 3\r\n";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 2,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 1,
                    character: 3,
                },
                end: Position {
                    line: 2,
                    character: 6,
                },
            },
            &"E A\r\nLINEB",
        );
        assert_eq!(
            delta,
            InputEdit {
                start_byte: 11,
                old_end_byte: 22,
                new_end_byte: 21,
                start_position: Point::new(1, 3),
                old_end_position: Point::new(2, 6),
                new_end_position: Point::new(2, 5),
            }
        );
        assert_eq!(doc.text, "Line 1\r\nLinE A\r\nLINEB\r\n");

        let text = "Line 1\nLine 2\nLine 3\n";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 3,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            &"new line\n",
        );
        assert_eq!(
            delta,
            InputEdit {
                start_byte: 0,
                old_end_byte: 1,
                new_end_byte: 9,
                start_position: Point::new(0, 0),
                old_end_position: Point::new(0, 1),
                new_end_position: Point::new(1, 0),
            }
        );
        assert_eq!(doc.text, "new line\nine 1\nLine 2\nLine 3\n");

        let text = "Line 1\nLine 2\nLine 3\n";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 4,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 2,
                    character: 6,
                },
                end: Position {
                    line: 2,
                    character: 7,
                },
            },
            &"NEW LINE\n",
        );
        assert_eq!(
            delta,
            InputEdit {
                start_byte: 20,
                old_end_byte: 21,
                new_end_byte: 29,
                start_position: Point::new(2, 6),
                old_end_position: Point::new(3, 0),
                new_end_position: Point::new(3, 0),
            }
        );
        assert_eq!(doc.text, "Line 1\nLine 2\nLine 3NEW LINE\n");
    }

    #[test]
    fn can_handle_edits_that_append_text() {
        let text = "a𐐀b";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 0,
                    character: 4,
                },
                end: Position {
                    line: 0,
                    character: 4,
                },
            },
            &"#NEW LINE",
        );
        assert_eq!(
            delta,
            InputEdit {
                start_byte: 6,
                old_end_byte: 6,
                new_end_byte: "a𐐀b#NEW LINE".len(),
                start_position: Point::new(0, "a𐐀b".len()),
                old_end_position: Point::new(0, "a𐐀b".len()),
                new_end_position: Point::new(0, "a𐐀b#NEW LINE".len()),
            }
        );
        assert_eq!(doc.text, "a𐐀b#NEW LINE");
    }

    #[test]
    fn can_handle_edits_that_prepend_text() {
        let text = "a𐐀b";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            &"NEW LINE#",
        );
        assert_eq!(
            delta,
            InputEdit {
                start_byte: 0,
                old_end_byte: 0,
                new_end_byte: "NEW LINE#".len(),
                start_position: Point::new(0, 0),
                old_end_position: Point::new(0, 0),
                new_end_position: Point::new(0, "NEW LINE#".len()),
            }
        );
        assert_eq!(doc.text, "NEW LINE#a𐐀b");
    }

    #[test]
    fn can_handle_edits_that_remove_text() {
        let text = "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.\nIƚ ιʂ ϝσɾ PRACTICE.\n";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 0,
                    character: "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗."
                        .chars()
                        .map(|ch| ch.len_utf16())
                        .sum::<usize>() as u32,
                },
                end: Position {
                    line: 1,
                    character: "Iƚ ιʂ ϝσɾ PRACTICE.\n"
                        .chars()
                        .map(|ch| ch.len_utf16())
                        .sum::<usize>() as u32
                        + 4,
                },
            },
            &"",
        );

        assert_eq!(
            delta,
            InputEdit {
                start_byte: "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.".len(),
                old_end_byte: "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.\nIƚ ιʂ ϝσɾ PRACTICE.\n".len(),
                new_end_byte: "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.".len(),
                start_position: Point::new(0, "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.".len()),
                old_end_position: Point::new(2, 0),
                new_end_position: Point::new(0, "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.".len()),
            }
        );
        assert_eq!(doc.text, "𝕿𝖍𝖎𝖘 𝖎𝖘 𝖆 𝖑𝖆𝖓𝖌𝖚𝖆𝖌𝖊 𝖘𝖊𝖗𝖛𝖊𝖗.");
    }

    #[test]
    fn can_deal_with_malformed_ranges() {
        let text = "Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium,\ntotam rem aperiam,\neaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo.";
        let lines = create_line_map_for_text(&text, None, None);

        let mut doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };

        let delta = doc.update(
            Range {
                start: Position {
                    line: 1,
                    character: 3,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            &"Lorem Ipsum#",
        );
        assert_eq!(
            delta,
            InputEdit {
                start_byte: 0,
                old_end_byte: "Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque laudantium,\ntot".len(),
                new_end_byte: "Lorem Ipsum#".len(),
                start_position: Point::new(0, 0),
                old_end_position: Point::new(1, 3),
                new_end_position: Point::new(0, "Lorem Ipsum#".len()),
            }
        );
        assert_eq!(
            doc.text,
            "Lorem Ipsum#am rem aperiam,\neaque ipsa quae ab illo inventore veritatis et quasi architecto beatae vitae dicta sunt explicabo."
        );
    }

    #[test]
    fn can_calculate_position() {
        let text = "Line 1A\nLine 2B\nLine 3C";
        let lines = create_line_map_for_text(&text, None, None);

        let doc = TextDoc {
            uri: "file:///C:/doc.rs".to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };

        assert_eq!(
            doc.to_position(3),
            Position {
                line: 0,
                character: 3
            }
        );
        assert_eq!(
            doc.to_position(12),
            Position {
                line: 1,
                character: 4
            }
        );
        assert_eq!(
            doc.to_position(5000),
            Position {
                line: 3,
                character: 0
            }
        );
    }

    #[test]
    fn can_open_documents() {
        let mut docs = TextDocs::new(FileIndex::new());

        let uri_a = "file:///a.cmm";
        let (doc, tree, globals) = create_doc(uri_a);

        docs.add(doc, tree, globals, TextDocStatus::Open);

        assert!(docs.is_open(uri_a));

        let uri_b = "file:///b.cmm";
        let (doc, tree, globals) = create_doc(uri_b);

        docs.add(doc, tree, globals, TextDocStatus::Open);

        assert!(docs.is_open(uri_b));
        assert!(docs.free_list.open.is_empty());
        assert!(docs.free_list.closed.is_empty());

        docs.close(uri_a);
        docs.close(uri_b);

        assert!(!docs.free_list.open.is_empty());
        assert!(docs.free_list.closed.is_empty());

        let uri_a = "file:///a.cmm";
        let (doc, tree, globals) = create_doc(uri_a);

        docs.add(doc, tree, globals, TextDocStatus::Open);

        assert!(!docs.free_list.open.is_empty());
        assert!(!docs.free_list.closed.is_empty());

        let uri_b = "file:///b.cmm";
        let (doc, tree, globals) = create_doc(uri_b);

        docs.add(doc, tree, globals, TextDocStatus::Open);

        assert!(docs.free_list.open.is_empty());
        assert!(!docs.free_list.closed.is_empty());
    }

    #[test]
    fn can_close_documents() {
        let mut docs = TextDocs::new(FileIndex::new());

        let uri_a = "file:///test.cmm";
        let (doc, tree, globals) = create_doc(uri_a);

        docs.add(doc, tree, globals, TextDocStatus::Open);

        assert!(docs.free_list.closed.is_empty());

        docs.close(uri_a);

        assert!(!docs.free_list.open.is_empty());
        assert!(!docs.is_open(uri_a));
    }

    #[test]
    fn can_find_subroutines() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (doc, _, LangExpressions { subroutines, .. }) =
            read_doc(file, file_idx).expect("Must not fail.");

        assert!(!subroutines.clone().is_none_or(|s| s.is_empty()));

        for name in ["subA", "subB"].iter() {
            assert!(
                subroutines
                    .as_ref()
                    .unwrap()
                    .iter()
                    .find_map(|s| (doc.text[s.name.clone()] == **name).then_some(()))
                    .is_some()
            );
        }
    }

    #[test]
    fn can_find_global_macros() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            doc,
            _,
            LangExpressions {
                macros: MacroDefinitions { globals, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(!globals.clone().is_none_or(|s| s.is_empty()));
        assert!(
            globals
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (doc.text[s.r#macro.clone()] == *"&global_macro").then_some(()))
                .is_some()
        );
    }

    #[test]
    fn can_find_local_macros() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            doc,
            _,
            LangExpressions {
                macros: MacroDefinitions { locals, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(!locals.clone().is_none_or(|s| s.is_empty()));
        assert!(
            locals
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (doc.text[s.clone()] == *"&local_macro").then_some(()))
                .is_some()
        );
    }

    #[test]
    fn can_find_subroutine_calls() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            doc,
            _,
            LangExpressions {
                calls: CallExpressions { subroutines, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(!subroutines.clone().is_none_or(|s| s.is_empty()));
        assert!(
            subroutines
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (doc.text[s.target.clone()] == *"subA").then_some(()))
                .is_some()
        );
        assert!(
            subroutines
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (s.docstring.is_some()
                    && doc.text[s.docstring.as_ref().unwrap().clone()]
                        == *"// This is a subroutine call\n")
                    .then_some(()))
                .is_some()
        );
    }

    #[test]
    fn can_find_script_calls() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            doc,
            _,
            LangExpressions {
                calls: CallExpressions { scripts, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(!scripts.clone().is_none_or(|s| s.locations.is_empty()));
        assert!(
            scripts
                .as_ref()
                .unwrap()
                .locations
                .iter()
                .find_map(|c| (doc.text[c.target.clone()] == *"../b/b.cmm").then_some(()))
                .is_some()
        );
        assert!(
            scripts
                .as_ref()
                .unwrap()
                .locations
                .iter()
                .find_map(|c| (doc.text[c.target.clone()] == *"../c.cmm").then_some(()))
                .is_some()
        );
        assert!(
            scripts
                .as_ref()
                .unwrap()
                .locations
                .iter()
                .find_map(|c| (c.docstring.is_some()
                    && doc.text[c.docstring.as_ref().unwrap().clone()]
                        == *"// This is subscript call\n")
                    .then_some(()))
                .is_some()
        );
    }

    #[test]
    fn can_resolve_script_call_targets() {
        let file_idx = create_file_idx();
        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            _doc,
            _tree,
            LangExpressions {
                calls: CallExpressions { scripts, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(scripts.is_some());

        let target =
            Url::from_file_path(path::absolute("tests/samples/b/b.cmm").expect("File must exist."))
                .unwrap()
                .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());

        let target =
            Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
                .unwrap()
                .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());

        let missing = Url::from_file_path(
            path::absolute("tests/samples/a/d/d.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_not_in_subscript_calls(&missing, scripts.as_ref().unwrap());
    }

    #[test]
    fn can_resolve_ambiguous_script_call_targets() {
        let file_idx = create_file_idx();
        let file =
            Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
                .unwrap();

        let (
            _doc,
            _tree,
            LangExpressions {
                calls: CallExpressions { scripts, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(scripts.is_some());

        let target = Url::from_file_path(
            path::absolute("tests/samples/same.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());

        let target = Url::from_file_path(
            path::absolute("tests/samples/a/same.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());

        let target = Url::from_file_path(
            path::absolute("tests/samples/b/same.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());
    }

    #[test]
    fn can_resolve_script_call_targets_with_relative_path() {
        let file_idx = create_file_idx();
        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            _doc,
            _tree,
            LangExpressions {
                calls: CallExpressions { scripts, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(scripts.is_some());

        let target = Url::from_file_path(
            path::absolute("tests/samples/b/b.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());

        let file_idx = create_file_idx();
        let file =
            Url::from_file_path(path::absolute("tests/samples/b/b.cmm").expect("File must exist."))
                .unwrap();

        let (
            _doc,
            _tree,
            LangExpressions {
                calls: CallExpressions { scripts, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(scripts.is_some());

        let target = Url::from_file_path(
            path::absolute("tests/samples/a/same.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_in_subscript_calls(&target, scripts.as_ref().unwrap());

        let missing = Url::from_file_path(
            path::absolute("tests/samples/a/b/same.cmm").expect("File must exist."),
        )
        .unwrap()
        .to_string();

        assert_file_not_in_subscript_calls(&missing, scripts.as_ref().unwrap());
    }
}
