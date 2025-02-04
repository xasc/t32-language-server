// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;

use tree_sitter::Tree;

use crate::{
    protocol::TextDocumentItem,
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

pub struct TextDoc {
    pub uri: String,
    pub lang_id: String,
    pub text: String,
    pub version: i64,
}

impl From<TextDocumentItem> for TextDoc {
    fn from(item: TextDocumentItem) -> Self {
        TextDoc {
            uri: item.uri,
            lang_id: LANGUAGE_ID.to_string(),
            version: item.version,
            text: item.text,
        }
    }
}

impl TextDoc {
    pub fn parse(&self) -> Tree {
        t32::parse(self.text.as_bytes(), None)
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
}

pub fn import_doc(r#in: TextDocumentItem) -> (TextDoc, Tree) {
    let doc = TextDoc::from(r#in);
    let tree = doc.parse();

    (doc, tree)
}
