// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use tree_sitter::Tree;

use crate::{
    ls::{
        doc::textdoc::{TextDoc, TextDocStatus},
        workspace::FileIndex,
    },
    protocol::Uri,
    t32::LangExpressions,
};

#[derive(Clone, Copy, Debug)]
struct DocIndex(TextDocStatus, usize);

pub struct TextDocs {
    docs: DocStore,
    trees: TreeStore,
    file_idx: FileIndex,

    #[allow(dead_code)]
    callers: CallerStore,

    #[allow(dead_code)]
    t32: LangExpressionStore,

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

struct LangExpressionStore {
    open: Vec<Option<LangExpressions>>,
    closed: Vec<Option<LangExpressions>>,
}

#[allow(dead_code)]
struct CallerStore {
    open: Vec<Option<Vec<Uri>>>,
    closed: Vec<Option<Vec<Uri>>>,
}

#[derive(Debug)]
struct CallRelations {
    target_slots: Vec<DocIndex>,
    source_uris: Vec<Uri>,
}

impl TextDocs {
    #[allow(dead_code)]
    fn new(files: FileIndex) -> Self {
        TextDocs {
            docs: DocStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            trees: TreeStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            t32: LangExpressionStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
            registry: HashMap::new(),
            free_list: FreeLists {
                open: Vec::new(),
                closed: Vec::new(),
            },
            file_idx: files,
            callers: CallerStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
        }
    }

    pub fn with_capacity(files: FileIndex, num: usize) -> Self {
        TextDocs {
            docs: DocStore {
                open: Vec::new(),
                closed: Vec::with_capacity(num),
            },
            trees: TreeStore {
                open: Vec::new(),
                closed: Vec::with_capacity(num),
            },
            t32: LangExpressionStore {
                open: Vec::new(),
                closed: Vec::with_capacity(num),
            },
            registry: HashMap::with_capacity(num),
            free_list: FreeLists {
                open: Vec::new(),
                closed: Vec::new(),
            },
            file_idx: files,
            callers: CallerStore {
                open: Vec::new(),
                closed: Vec::with_capacity(num),
            },
        }
    }

    pub fn from_workspace(
        file_idx: FileIndex,
        members: Vec<(TextDoc, Tree, LangExpressions)>,
    ) -> Self {
        let mut store = TextDocs::with_capacity(file_idx, members.len());

        debug_assert_eq!(store.docs.closed.len(), 0);
        for file in members {
            let _ = store.insert_or_update(file.0, file.1, file.2, TextDocStatus::Closed);
        }

        let calls = store.get_call_relations();
        store.register_all_callers(calls);
        store
    }

    pub fn add(&mut self, doc: TextDoc, tree: Tree, expr: LangExpressions, status: TextDocStatus) {
        if let Some(targets) = self.get_called_subscripts(&doc.uri) {
            self.remove_caller(&doc.uri, &targets.clone());
        }

        let uri = doc.uri.clone();
        self.insert_or_update(doc, tree, expr, status);

        self.update_callers(&uri, self.get_call_relations());
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
        let expr = self.t32.open[idx].take();
        let callers = self.callers.open[idx].take();

        self.free_list.open.push(idx);
        self.registry.remove(uri);

        if self.free_list.closed.is_empty() {
            let len = self.docs.closed.len();

            self.docs.closed.push(Some(doc));
            self.trees.closed.push(Some(tree));
            self.t32.closed.push(expr);
            self.callers.closed.push(callers);

            self.registry
                .insert(uri.to_string(), DocIndex(TextDocStatus::Closed, len));
        } else {
            let slot = self.free_list.closed.pop().unwrap();

            self.docs.closed[slot] = Some(doc);
            self.trees.closed[slot] = Some(tree);
            self.t32.closed[slot] = expr;
            self.callers.closed[slot] = callers;

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
    pub fn get_lang_expressions(&self, uri: &str) -> Option<&LangExpressions> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.t32.open[idx.1].as_ref(),
            Some(idx) => self.t32.closed[idx.1].as_ref(),
            None => None,
        }
    }

    #[allow(dead_code)]
    pub fn get_callers(&self, uri: &str) -> Option<&Vec<Uri>> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.callers.open[idx.1].as_ref(),
            Some(idx) => self.callers.closed[idx.1].as_ref(),
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

    fn insert_or_update(
        &mut self,
        doc: TextDoc,
        tree: Tree,
        expr: LangExpressions,
        status: TextDocStatus,
    ) {
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
                        self.callers.open[val.1] = None;
                    }
                    TextDocStatus::Closed => {
                        self.docs.closed[val.1] = Some(doc);
                        self.trees.closed[val.1] = Some(tree);
                        self.t32.closed[val.1] = Some(expr);
                        self.callers.closed[val.1] = None;
                    }
                }
                return;
            } else {
                match val.0 {
                    TextDocStatus::Open => {
                        self.docs.open[val.1] = None;
                        self.trees.open[val.1] = None;
                        self.t32.open[val.1] = None;
                        self.callers.open[val.1] = None;

                        self.free_list.open.push(val.1);
                    }
                    TextDocStatus::Closed => {
                        self.docs.closed[val.1] = None;
                        self.trees.closed[val.1] = None;
                        self.t32.closed[val.1] = None;
                        self.callers.closed[val.1] = None;

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
                    self.callers.open.push(None);

                    self.registry.insert(uri, DocIndex(status, len));
                } else {
                    let slot = self.free_list.open.pop().unwrap();

                    self.docs.open[slot] = Some(doc);
                    self.trees.open[slot] = Some(tree);
                    self.t32.open[slot] = Some(expr);
                    self.callers.open[slot] = None;

                    self.registry.insert(uri, DocIndex(status, slot));
                }
            }
            TextDocStatus::Closed => {
                if self.free_list.closed.is_empty() {
                    let len = self.docs.closed.len();

                    self.docs.closed.push(Some(doc));
                    self.trees.closed.push(Some(tree));
                    self.t32.closed.push(Some(expr));
                    self.callers.closed.push(None);

                    self.registry.insert(uri, DocIndex(status, len));
                } else {
                    let slot = self.free_list.closed.pop().unwrap();

                    self.docs.closed[slot] = Some(doc);
                    self.trees.closed[slot] = Some(tree);
                    self.t32.closed[slot] = Some(expr);
                    self.callers.closed[slot] = None;

                    self.registry.insert(uri, DocIndex(status, slot));
                }
            }
        }
    }

    fn register_all_callers(&mut self, calls: CallRelations) {
        for (DocIndex(status, target), source) in calls
            .target_slots
            .into_iter()
            .zip(calls.source_uris.into_iter())
        {
            let callers = match status {
                TextDocStatus::Open => &mut self.callers.open[target],
                TextDocStatus::Closed => &mut self.callers.closed[target],
            };

            if let Some(files) = callers {
                if files.contains(&source) {
                    continue;
                }
                files.push(source);
            } else {
                *callers = Some(vec![source]);
            }
        }
    }

    fn remove_caller(&mut self, uri: &Uri, targets: &Vec<Option<Uri>>) {
        for target in targets {
            if target.is_none() {
                continue;
            }

            if let Some(DocIndex(status, idx)) = self.registry.get(target.as_ref().unwrap()) {
                let callers = match status {
                    TextDocStatus::Open => &mut self.callers.open[*idx],
                    TextDocStatus::Closed => &mut self.callers.closed[*idx],
                };

                if let Some(files) = callers {
                    if let Some(pos) = files.iter().position(|f| f == uri) {
                        files.swap_remove(pos);
                    }
                }
            };
        }
    }

    fn update_callers(&mut self, uri: &Uri, calls: CallRelations) {
        let Some(DocIndex(status, idx)) = self.registry.get(uri) else {
            return;
        };

        let callers = match status {
            TextDocStatus::Open => &mut self.callers.open[*idx],
            TextDocStatus::Closed => &mut self.callers.closed[*idx],
        };

        for (DocIndex(group, target), source) in calls
            .target_slots
            .into_iter()
            .zip(calls.source_uris.into_iter())
        {
            if target != *idx || group != *status {
                continue;
            }

            if let Some(files) = callers {
                if files.contains(&source) {
                    continue;
                }
                files.push(source);
            } else {
                *callers = Some(vec![source]);
            }
        }
    }

    fn get_call_relations(&self) -> CallRelations {
        let mut targets: Vec<DocIndex> = Vec::new();
        let mut callers: Vec<Uri> = Vec::new();

        fn extract_calls(
            registry: &HashMap<Uri, DocIndex>,
            docs: &Vec<Option<TextDoc>>,
            lang: &Vec<Option<LangExpressions>>,
        ) -> (Vec<DocIndex>, Vec<Uri>) {
            let mut targets: Vec<DocIndex> = Vec::new();
            let mut callers: Vec<Uri> = Vec::new();

            for (ii, expr) in lang.iter().enumerate() {
                if expr.is_none() {
                    continue;
                }

                let scripts = &expr.as_ref().unwrap().calls.scripts;
                if scripts.is_none() {
                    continue;
                }
                let scripts = scripts.as_ref().unwrap();

                let file = docs[ii].as_ref().expect("Slot must not be empty.");

                for call in &scripts.targets {
                    if let Some(target) = call {
                        let slot = registry.get(target);
                        if slot.is_none() {
                            continue;
                        }
                        targets.push(*slot.unwrap());
                        callers.push(file.uri.clone());
                    }
                }
            }
            (targets, callers)
        }

        let (mut t, mut c) = extract_calls(&self.registry, &self.docs.open, &self.t32.open);
        targets.append(&mut t);
        callers.append(&mut c);

        let (mut t, mut c) = extract_calls(&self.registry, &self.docs.closed, &self.t32.closed);
        targets.append(&mut t);
        callers.append(&mut c);

        debug_assert_eq!(targets.len(), callers.len());
        CallRelations {
            target_slots: targets,
            source_uris: callers,
        }
    }

    fn get_called_subscripts(&self, uri: &Uri) -> Option<&Vec<Option<Uri>>> {
        let subscripts = match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.t32.open[idx.1]
                .as_ref()
                .expect("Slot must not be empty.")
                .calls
                .scripts
                .as_ref(),
            Some(idx) => self.t32.closed[idx.1]
                .as_ref()
                .expect("Slot must not be empty.")
                .calls
                .scripts
                .as_ref(),
            None => None,
        };

        if let Some(scripts) = subscripts {
            Some(&scripts.targets)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::path;

    use url::Url;

    use crate::{
        ls::{
            doc::{
                find_global_macro_definitions, find_subroutines, read_doc,
                textdoc::create_line_map_for_text,
            },
            workspace::index_files,
        },
        t32::{self, CallExpressions, LANGUAGE_ID},
    };

    fn files() -> Vec<Url> {
        vec![
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
        ]
    }

    fn create_file_idx() -> FileIndex {
        let files = files();
        index_files(files)
    }

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

    #[test]
    fn can_open_documents() {
        let mut docs = TextDocs::new(FileIndex::new());

        let files = ["file:///a.cmm", "file:///b.cmm"];
        for uri in files.iter() {
            let (doc, tree, expr) = create_doc(*uri);
            docs.add(doc, tree, expr, TextDocStatus::Open);

            assert!(docs.is_open(*uri));
        }
        assert!(docs.free_list.open.is_empty());
        assert!(docs.free_list.closed.is_empty());

        for uri in files.iter() {
            docs.close(*uri);
        }
        assert!(!docs.free_list.open.is_empty());
        assert!(docs.free_list.closed.is_empty());

        let (doc, tree, expr) = create_doc(files[0]);
        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(!docs.free_list.open.is_empty());
        assert!(!docs.free_list.closed.is_empty());

        let (doc, tree, expr) = create_doc(files[1]);
        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(docs.free_list.open.is_empty());
        assert!(!docs.free_list.closed.is_empty());
    }

    #[test]
    fn can_close_documents() {
        let mut docs = TextDocs::new(FileIndex::new());

        let uri = "file:///test.cmm";
        let (doc, tree, expr) = create_doc(uri);

        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(docs.free_list.closed.is_empty());

        docs.close(uri);

        assert!(!docs.free_list.open.is_empty());
        assert!(!docs.is_open(uri));
    }

    #[test]
    fn can_import_workspace() {
        let files = files();
        let file_idx = create_file_idx();

        let mut members: Vec<(TextDoc, Tree, LangExpressions)> = Vec::new();
        for uri in files {
            let (doc, tree, expr) =
                read_doc(uri.clone(), file_idx.clone()).expect("Must not fail.");
            members.push((doc, tree, expr));
        }

        let docs = TextDocs::from_workspace(FileIndex::new(), members);

        let checks: Vec<(String, String)> = vec![
            (
                "tests/samples/a/a.cmm".to_string(),
                "tests/samples/c.cmm".to_string(),
            ),
            (
                "tests/samples/a/d/d.cmmt".to_string(),
                "tests/samples/c.cmm".to_string(),
            ),
            (
                "tests/samples/same.cmm".to_string(),
                "tests/samples/c.cmm".to_string(),
            ),
        ];

        for (file, caller) in checks {
            let target =
                Url::from_file_path(path::absolute(&file).expect("File must exist.")).unwrap();

            let all_callers = docs
                .get_callers(&target.to_string())
                .expect("Must not be empty.");
            assert!(
                all_callers.contains(
                    &Url::from_file_path(path::absolute(&caller).expect("File must exist."))
                        .unwrap()
                        .to_string()
                )
            );
        }

        let sanity: Vec<(String, String)> = vec![
            (
                "tests/samples/a/a.cmm".to_string(),
                "tests/samples/b/b.cmm".to_string(),
            ),
            (
                "tests/samples/a/d/d.cmmt".to_string(),
                "tests/samples/a/a.cmm".to_string(),
            ),
            (
                "tests/samples/same.cmm".to_string(),
                "tests/samples/a/a.cmm".to_string(),
            ),
        ];

        for (file, caller) in sanity {
            let target =
                Url::from_file_path(path::absolute(&file).expect("File must exist.")).unwrap();

            let all_callers = docs
                .get_callers(&target.to_string())
                .expect("Must not be empty.");
            assert!(
                !all_callers.contains(
                    &Url::from_file_path(path::absolute(&caller).expect("File must exist."))
                        .unwrap()
                        .to_string()
                )
            );
        }
    }

    #[test]
    fn can_update_callers() {
        let files = files();
        let file_idx = create_file_idx();

        let mut members: Vec<(TextDoc, Tree, LangExpressions)> = Vec::new();
        for uri in files {
            let (doc, tree, expr) =
                read_doc(uri.clone(), file_idx.clone()).expect("Must not fail.");
            members.push((doc, tree, expr));
        }
        let mut docs = TextDocs::from_workspace(FileIndex::new(), members);

        let uri =
            Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
                .unwrap();

        let (doc, tree, expr) = read_doc(uri.clone(), file_idx.clone()).expect("Must not fail.");

        docs.add(doc, tree, expr, TextDocStatus::Open);

        let caller =
            Url::from_file_path(path::absolute("tests/samples/c.cmm").expect("File must exist."))
                .unwrap();
        let (doc, tree, expr) = read_doc(caller.clone(), file_idx.clone()).expect("Must not fail.");

        docs.add(doc, tree, expr, TextDocStatus::Open);

        let all_callers = docs
            .get_callers(&uri.to_string())
            .expect("Must not be empty.");

        assert!(!all_callers.contains(&caller.to_string()));
    }
}
