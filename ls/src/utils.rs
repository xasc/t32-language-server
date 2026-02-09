// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{cmp::Ordering, collections::BTreeMap, convert::From, ops::Range};

#[cfg(test)]
use std::path;

use tree_sitter::Range as TRange;

#[cfg(test)]
use url::Url;

#[cfg(test)]
use tree_sitter::Tree;

use crate::protocol::{Location, Range as LRange, Uri};

#[cfg(test)]
use crate::{
    ls::{FileIndex, TextDoc, TextDocs, index_files, read_doc},
    t32::LangExpressions,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BRange(Range<usize>);

// TODO: All URIs are starting with "file://". If we know that we are passing
// in a valid URI, we can skip the comparison of the string start.
#[derive(Clone, Debug)]
pub struct FileLocationMap {
    files: Vec<Uri>,
    locations: Vec<Vec<LRange>>,
    mapping: Vec<u32>,
    free_list: Vec<u32>,
}

pub struct FileLocationMapIterator<'a> {
    map: &'a FileLocationMap,
    idx: usize,
}

pub struct FileLocationIndex(BTreeMap<Uri, FileLocationMap>);

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

impl FileLocationMap {
    pub fn new() -> Self {
        FileLocationMap {
            files: Vec::new(),
            locations: Vec::new(),
            mapping: Vec::new(),
            free_list: Vec::new(),
        }
    }

    pub fn get<'a>(&'a self, uri: &str) -> Option<&'a Vec<LRange>> {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        match self.files.binary_search_by(|f| f.as_str().cmp(uri)) {
            Ok(ii) => {
                let slot = self.mapping[ii] as usize;
                Some(&self.locations[slot])
            }
            Err(_) => None,
        }
    }

    #[cfg(test)]
    pub fn contains(&self, uri: &str) -> bool {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        match self.files.binary_search_by(|f| f.as_str().cmp(uri)) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    pub fn iter<'a>(&'a self) -> FileLocationMapIterator<'a> {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        FileLocationMapIterator { map: self, idx: 0 }
    }

    pub fn insert(&mut self, uri: &str, loc: LRange) {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        match self.files.binary_search_by(|f| f.as_str().cmp(uri)) {
            Ok(ii) => {
                let slot = self.mapping[ii] as usize;

                match self.locations[slot].binary_search_by(|r| r.cmp(&loc)) {
                    Ok(_) => return,
                    Err(idx) => self.locations[slot].insert(idx, loc),
                };
            }
            Err(ii) => {
                let slot: usize = if self.free_list.is_empty() {
                    let pos = self.locations.len();

                    self.locations.push(vec![loc]);
                    pos
                } else {
                    let pos: usize = self.free_list.pop().expect("List must no be empty.") as usize;

                    debug_assert!(self.locations[pos].is_empty());
                    self.locations[pos].push(loc);
                    pos
                };
                self.files.insert(ii, uri.to_string());
                self.mapping.insert(ii, slot as u32);
            }
        }
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());
    }

    #[allow(unused)]
    pub fn remove(&mut self, uri: &str) {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        match self.files.binary_search_by(|f| f.as_str().cmp(uri)) {
            Ok(ii) => {
                let slot = self.mapping[ii];

                self.locations[slot as usize].clear();
                self.free_list.push(slot);

                self.files.remove(ii);
                self.mapping.remove(ii);
            }
            Err(_) => (),
        }
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());
    }

    pub fn rename(&mut self, old: &str, new: &str) {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        if let Ok(ii) = self.files.binary_search_by(|f| f.as_str().cmp(old)) {
            self.files[ii] = new.to_string();
        }

        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());
    }

    pub fn to_locations(self) -> Vec<Location> {
        debug_assert!(self.files.len() <= self.locations.len());
        debug_assert_eq!(self.files.len(), self.mapping.len());

        let Self {
            files,
            mut locations,
            mapping,
            ..
        } = self;

        let mut locs: Vec<Location> = Vec::with_capacity(files.len());
        for (file, slot) in files.into_iter().zip(mapping.into_iter()) {
            let mut spans: Vec<LRange> = Vec::new();

            spans.append(&mut locations[slot as usize]);

            for span in spans {
                locs.push(Location {
                    uri: file.clone(),
                    range: span,
                });
            }
        }
        locs
    }
}

impl<'a> FileLocationIndex {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&'a self, uri: &str) -> Option<&'a FileLocationMap> {
        self.0.get(uri)
    }

    pub fn insert(&mut self, key: &str, uri: &str, span: LRange) {
        let Some(locations) = self.0.get_mut(key) else {
            let mut locations = FileLocationMap::new();

            locations.insert(uri, span);
            self.0.insert(key.to_string(), locations);

            return;
        };
        locations.insert(uri, span);
    }

    pub fn remove_key_locs(&mut self, keys: &[String], uri: &str) {
        for key in keys {
            if let Some(locations) = self.0.get_mut(key) {
                locations.remove(uri);
            }
        }
    }

    pub fn rename_key(&mut self, old: &str, new: &str) {
        if let Some(values) = self.0.remove(old) {
            debug_assert!(!self.0.contains_key(new));
            self.0.insert(new.to_string(), values);
        }
    }

    pub fn rename_locs(&mut self, old_file: &str, new_file: &str) {
        for locations in self.0.values_mut() {
            debug_assert!(locations.get(new_file).is_none());
            locations.rename(old_file, new_file);
        }
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

impl<'a> Iterator for FileLocationMapIterator<'a> {
    type Item = (&'a Uri, &'a Vec<LRange>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.map.files.len() {
            let offset = self.idx;
            self.idx += 1;

            let file: &Uri = &self.map.files[offset];
            let pos: usize = self.map.mapping[offset] as usize;

            Some((file, &self.map.locations[pos]))
        } else {
            None
        }
    }
}

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

impl PartialEq<FileLocationMap> for FileLocationMap {
    fn eq(&self, other: &FileLocationMap) -> bool {
        let FileLocationMap {
            files,
            locations,
            mapping,
            free_list: _,
        } = self;
        let FileLocationMap {
            files: other_files,
            locations: other_locations,
            mapping: other_mapping,
            free_list: _,
        } = other;

        if files.len() != other_files.len() {
            return false;
        }

        for (file, slot) in files.iter().zip(mapping.iter()) {
            if let Ok(ii) = other_files.binary_search_by(|f| f.as_str().cmp(file)) {
                let other_slot = other_mapping[ii] as usize;
                if other_locations[other_slot] != locations[*slot as usize] {
                    return false;
                }
            } else {
                return false;
            }
        }
        return true;
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
