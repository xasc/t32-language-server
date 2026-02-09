// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::collections::BTreeMap;

use tree_sitter::Tree;

use crate::{
    ls::{
        doc::textdoc::{TextDoc, TextDocStatus},
        tasks::RenameFileOperations,
    },
    protocol::Uri,
    t32::{
        CallExpressions, LangExpressions, MacroDefinition, ParameterDeclaration,
        ParameterDeclarationKind, SubscriptCallKind, SubscriptCalls,
    },
    utils::{BRange, FileLocationIndex, FileLocationMap},
};

#[allow(unused)]
struct CommandIndex(FileLocationIndex);

/// TODO: Reduce size to 64 bit or less.
#[derive(Clone, Copy, Debug)]
struct DocIndex(TextDocStatus, usize);

pub struct TextDocs {
    docs: DocStore,
    trees: TreeStore,

    callers: CallerStore,
    t32: LangExpressionStore,

    macro_index: BTreeMap<String, Vec<Uri>>,
    file_target_index: FileTargetIndex,
    command_index: CommandIndex,

    registry: BTreeMap<Uri, DocIndex>,
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

struct CallerStore {
    open: Vec<Option<Vec<Uri>>>,
    closed: Vec<Option<Vec<Uri>>>,
}

#[derive(Debug)]
struct CallRelations {
    target_slots: Vec<DocIndex>,
    source_uris: Vec<Uri>,
}

#[derive(Debug, Clone)]
pub struct TextDocData {
    pub doc: TextDoc,
    pub tree: Tree,
}

pub struct GlobalMacroDefIndex<'a>(
    pub Vec<Uri>,
    pub Vec<u32>,
    pub Vec<&'a str>,
    pub Vec<&'a MacroDefinition>,
);

impl<'a> CommandIndex {
    pub fn new() -> Self {
        Self(FileLocationIndex::new())
    }

    #[expect(unused)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&'a self, cmd: &str) -> Option<&'a FileLocationMap> {
        let identifier: &str = if cmd
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_digit(10) || c == '_' || c == '.')
        {
            cmd
        } else {
            &cmd.to_lowercase()
        };
        self.0.get(&identifier)
    }

    pub fn add(&mut self, doc: &TextDoc, t32: &LangExpressions) {
        for r#macro in t32.macros.privates.iter() {
            let (start, end) = (r#macro.cmd.start, r#macro.cmd.end);
            let span = doc.to_range(start, end);

            self.0.insert("private", &doc.uri, span);
        }
        for r#macro in t32.macros.locals.iter() {
            let (start, end) = (r#macro.cmd.start, r#macro.cmd.end);
            let span = doc.to_range(start, end);

            self.0.insert("local", &doc.uri, span);
        }
        for r#macro in t32.macros.globals.iter() {
            let (start, end) = (r#macro.cmd.start, r#macro.cmd.end);
            let span = doc.to_range(start, end);

            self.0.insert("global", &doc.uri, span);
        }

        for sub in t32.calls.subroutines.iter() {
            let (start, end) = (sub.call.start, sub.call.end);
            let span = doc.to_range(start, end);

            self.0.insert("gosub", &doc.uri, span);
        }
        if let Some(scripts) = &t32.calls.scripts {
            for (location, kind) in scripts.locations.iter().zip(scripts.kinds.iter()) {
                let (start, end) = (location.call.start, location.call.end);
                let span = doc.to_range(start, end);

                if *kind == SubscriptCallKind::Do {
                    self.0.insert("do", &doc.uri, span);
                } else {
                    self.0.insert("run", &doc.uri, span);
                }
            }
        }

        for ParameterDeclaration { cmd, kind, .. } in t32.parameters.iter() {
            let (start, end) = (cmd.start, cmd.end);
            let span = doc.to_range(start, end);

            if *kind == ParameterDeclarationKind::Entry {
                self.0.insert("entry", &doc.uri, span);
            } else {
                self.0.insert("parameters", &doc.uri, span);
            }
        }

        for cmd in t32.commands.iter() {
            let span = {
                let (start, end) = (cmd.command.inner().start, cmd.command.inner().end);
                doc.to_range(start, end)
            };

            let (start, end) = (cmd.identifier.inner().start, cmd.identifier.inner().end);

            self.0
                .insert(&doc.text[start..end].to_lowercase(), &doc.uri, span);
        }
    }

    pub fn remove(&mut self, src: &str, identifiers: &[String]) {
        debug_assert!(identifiers.iter().all(|i| {
            i.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_digit(10) || c == '_' || c == '.')
        }));

        self.0.remove_key_locs(identifiers, src);
    }

    pub fn rename(&mut self, old: &Uri, new: &Uri) {
        debug_assert!(self.0.get(new).is_none());

        self.0.rename_key(old, new);
        self.0.rename_locs(old, new);
    }
}

impl TextDocData {
    #[expect(unused)]
    pub fn build(doc: TextDoc, tree: Tree) -> Self {
        TextDocData { doc, tree }
    }
}

impl<'a> TextDocs {
    #[cfg(test)]
    pub fn new() -> Self {
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
            macro_index: BTreeMap::new(),
            file_target_index: FileTargetIndex::new(),
            command_index: CommandIndex::new(),
            registry: BTreeMap::new(),
            free_list: FreeLists {
                open: Vec::new(),
                closed: Vec::new(),
            },
            callers: CallerStore {
                open: Vec::new(),
                closed: Vec::new(),
            },
        }
    }

    pub fn with_capacity(num: usize) -> Self {
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
            macro_index: BTreeMap::new(),
            file_target_index: FileTargetIndex::new(),
            command_index: CommandIndex::new(),
            registry: BTreeMap::new(),
            free_list: FreeLists {
                open: Vec::new(),
                closed: Vec::new(),
            },
            callers: CallerStore {
                open: Vec::new(),
                closed: Vec::with_capacity(num),
            },
        }
    }

    pub fn from_workspace(members: Vec<(TextDoc, Tree, LangExpressions)>) -> Self {
        let mut store = TextDocs::with_capacity(members.len());

        debug_assert_eq!(store.docs.closed.len(), 0);
        debug_assert!(store.file_target_index.is_empty());
        for (doc, tree, t32) in members {
            store.update_macro_index(&doc, &t32.macro_refs);
            store.update_file_target_index(&doc, t32.calls.scripts.as_ref());
            store.update_command_index(&doc, &t32);

            let _ = store.insert_or_update(doc, tree, t32, TextDocStatus::Closed);
        }

        let calls = store.get_call_relations();
        store.register_all_callers(calls);
        store
    }

    pub fn add(&mut self, doc: TextDoc, tree: Tree, expr: LangExpressions, status: TextDocStatus) {
        if let Some(targets) = self.get_called_subscripts(&doc.uri) {
            self.remove_caller(&doc.uri, &targets.clone());
        }
        self.update_macro_index(&doc, &expr.macro_refs);
        self.update_file_target_index(&doc, expr.calls.scripts.as_ref());
        self.update_command_index(&doc, &expr);

        let uri = doc.uri.clone();
        self.insert_or_update(doc, tree, expr, status);

        self.update_callers(&uri, self.get_call_relations());
    }

    pub fn update(&mut self, doc: TextDoc, tree: Tree, expr: LangExpressions) {
        self.update_macro_index(&doc, &expr.macro_refs);
        self.update_file_target_index(&doc, expr.calls.scripts.as_ref());
        self.update_command_index(&doc, &expr);

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

    pub fn rename_files(&mut self, renamed: &RenameFileOperations) {
        debug_assert!(renamed.old.len() > 0);
        debug_assert_eq!(renamed.old.len(), renamed.new.len());

        let RenameFileOperations { old, new } = renamed;
        let mut slots: Vec<Option<DocIndex>> = Vec::with_capacity(old.len());

        for (old, new) in old.iter().zip(new.iter()) {
            if let Some(loc) = self.registry.remove(new) {
                match loc {
                    DocIndex(TextDocStatus::Open, slot) => {
                        debug_assert!(!self.free_list.open.contains(&slot));
                        self.free_list.open.push(slot);
                    }
                    DocIndex(TextDocStatus::Closed, slot) => {
                        debug_assert!(!self.free_list.closed.contains(&slot));
                        self.free_list.closed.push(slot);
                    }
                }
                self.remove_data(loc);
            }

            if let Some(loc) = self.registry.remove(old) {
                self.registry.insert(new.clone(), loc);
                slots.push(Some(loc));
            } else {
                slots.push(None);
            }
        }
        self.rename_docs(&slots, &new);

        self.rename_lang_expr(&old, &new);
        self.rename_callers(&old, &new);
        self.rename_macro_index_refs(&old, &new);
        self.rename_files_in_file_target_index(&old, &new);
        self.rename_files_in_command_index(&old, &new);
    }

    pub fn get_doc(&self, uri: &str) -> Option<&TextDoc> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.docs.open[idx.1].as_ref(),
            Some(idx) => self.docs.closed[idx.1].as_ref(),
            None => None,
        }
    }

    #[expect(unused)]
    pub fn get_tree(&self, uri: &str) -> Option<&Tree> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.trees.open[idx.1].as_ref(),
            Some(idx) => self.trees.closed[idx.1].as_ref(),
            None => None,
        }
    }

    pub fn get_lang_expressions(&self, uri: &str) -> Option<&LangExpressions> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.t32.open[idx.1].as_ref(),
            Some(idx) => self.t32.closed[idx.1].as_ref(),
            None => None,
        }
    }

    pub fn get_callers(&self, uri: &str) -> Option<&Vec<Uri>> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => self.callers.open[idx.1].as_ref(),
            Some(idx) => self.callers.closed[idx.1].as_ref(),
            None => None,
        }
    }

    pub fn get_doc_data(&self, uri: &str) -> Option<(&TextDoc, &Tree, &LangExpressions)> {
        match self.registry.get(uri) {
            Some(idx) if idx.0 == TextDocStatus::Open => {
                if self.docs.open[idx.1].is_none()
                    || self.trees.open[idx.1].is_none()
                    || self.t32.open[idx.1].is_none()
                {
                    None
                } else {
                    Some((
                        &self.docs.open[idx.1].as_ref().unwrap(),
                        &self.trees.open[idx.1].as_ref().unwrap(),
                        &self.t32.open[idx.1].as_ref().unwrap(),
                    ))
                }
            }
            Some(idx) => {
                if self.docs.closed[idx.1].is_none()
                    || self.trees.closed[idx.1].is_none()
                    || self.t32.closed[idx.1].is_none()
                {
                    None
                } else {
                    Some((
                        self.docs.closed[idx.1].as_ref().unwrap(),
                        self.trees.closed[idx.1].as_ref().unwrap(),
                        self.t32.closed[idx.1].as_ref().unwrap(),
                    ))
                }
            }
            None => None,
        }
    }

    pub fn get_all_global_macros(&'a self) -> Option<GlobalMacroDefIndex<'a>> {
        debug_assert_eq!(self.docs.open.len(), self.t32.open.len());

        let (mut files, mut nums, mut names, mut macros) =
            Self::gather_global_macros(&self.docs.open, &self.t32.open);

        let (mut files_, mut nums_, mut names_, mut macros_): (
            Vec<Uri>,
            Vec<u32>,
            Vec<&str>,
            Vec<&MacroDefinition>,
        ) = Self::gather_global_macros(&self.docs.closed, &self.t32.closed);

        files.append(&mut files_);
        nums.append(&mut nums_);
        names.append(&mut names_);
        macros.append(&mut macros_);

        if files.len() > 0 {
            Some(GlobalMacroDefIndex(files, nums, names, macros))
        } else {
            None
        }
    }

    /// Macro name must start with `&`.
    pub fn get_all_scripts_with_macro(&'a self, name: &str) -> Option<&'a Vec<Uri>> {
        self.macro_index.get(name)
    }

    pub fn get_all_target_file_refs(&'a self, uri: &str) -> Option<&'a FileLocationMap> {
        self.file_target_index.get(uri)
    }

    pub fn get_all_command_refs(&'a self, cmd: &str) -> Option<&'a FileLocationMap> {
        self.command_index.get(cmd)
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

    fn remove_data(&mut self, slot: DocIndex) {
        match slot {
            DocIndex(TextDocStatus::Open, idx) => {
                debug_assert!(self.docs.open[idx].is_some());
                debug_assert!(self.trees.open[idx].is_some());
                debug_assert!(self.t32.open[idx].is_some());
                debug_assert!(self.callers.open[idx].is_some());

                self.docs.open[idx] = None;
                self.trees.open[idx] = None;
                self.t32.open[idx] = None;
                self.callers.open[idx] = None;
            }
            DocIndex(TextDocStatus::Closed, idx) => {
                debug_assert!(self.docs.closed[idx].is_some());
                debug_assert!(self.trees.closed[idx].is_some());
                debug_assert!(self.t32.closed[idx].is_some());
                debug_assert!(self.callers.closed[idx].is_some());

                self.docs.closed[idx] = None;
                self.trees.closed[idx] = None;
                self.t32.closed[idx] = None;
                self.callers.closed[idx] = None;
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

    // TODO: Calculate once and update on demand!?
    fn get_call_relations(&self) -> CallRelations {
        let mut targets: Vec<DocIndex> = Vec::new();
        let mut callers: Vec<Uri> = Vec::new();

        fn extract_calls(
            registry: &BTreeMap<Uri, DocIndex>,
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

    fn gather_global_macros(
        docs: &'a Vec<Option<TextDoc>>,
        t32: &'a Vec<Option<LangExpressions>>,
    ) -> (Vec<Uri>, Vec<u32>, Vec<&'a str>, Vec<&'a MacroDefinition>) {
        let mut files: Vec<Uri> = Vec::new();
        let mut nums: Vec<u32> = Vec::new();
        let mut names: Vec<&str> = Vec::new();
        let mut macros: Vec<&MacroDefinition> = Vec::new();

        for (doc, t32) in docs.iter().zip(t32.iter()) {
            if doc.is_none() {
                continue;
            }

            let globals = &t32.as_ref().unwrap().macros.globals;
            if globals.is_empty() {
                continue;
            }

            let num = globals.len();
            nums.push(num as u32);

            let doc = doc.as_ref().unwrap();

            names.reserve(num);
            macros.reserve(num);
            for def in globals {
                names.push(&doc.text[def.r#macro.clone()]);
                macros.push(def);
            }

            let uri = doc.uri.clone();
            files.push(uri);

            debug_assert_eq!(nums.len(), files.len());
            debug_assert_eq!(nums.iter().sum::<u32>() as usize, macros.len());
            debug_assert_eq!(macros.len(), names.len());
        }
        debug_assert_eq!(nums.len(), files.len());
        debug_assert_eq!(nums.iter().sum::<u32>() as usize, macros.len());
        debug_assert_eq!(macros.len(), names.len());

        (files, nums, names, macros)
    }

    fn rename_docs(&mut self, slots: &Vec<Option<DocIndex>>, new: &[Uri]) {
        debug_assert!(new.len() > 0);
        debug_assert_eq!(new.len(), slots.len());

        for (ii, slot) in slots.iter().enumerate().filter(|s| s.1.is_some()) {
            match slot.unwrap() {
                DocIndex(TextDocStatus::Open, idx) => {
                    self.docs.open[idx].as_mut().unwrap().uri = new[ii].clone()
                }
                DocIndex(TextDocStatus::Closed, idx) => {
                    self.docs.closed[idx].as_mut().unwrap().uri = new[ii].clone()
                }
            }
        }
    }

    fn rename_lang_expr(&mut self, old: &[Uri], new: &[Uri]) {
        debug_assert!(old.len() > 0);
        debug_assert_eq!(old.len(), new.len());

        for t32 in self
            .t32
            .open
            .iter_mut()
            .filter(|e| e.is_some())
            .chain(self.t32.closed.iter_mut().filter(|e| e.is_some()))
        {
            let t32 = t32.as_mut().unwrap();
            if t32.calls.scripts.is_none() {
                continue;
            }

            for target in t32
                .calls
                .scripts
                .as_mut()
                .unwrap()
                .targets
                .iter_mut()
                .filter(|t| t.is_some())
            {
                let target = target.as_mut().unwrap();
                if let Some(ii) = old.iter().position(|uri| *uri == *target) {
                    *target = new[ii].clone();
                }
            }
        }
    }

    fn rename_callers(&mut self, old: &[Uri], new: &[Uri]) {
        debug_assert!(old.len() > 0);
        debug_assert_eq!(old.len(), new.len());

        for callers in self
            .callers
            .open
            .iter_mut()
            .filter(|c| c.is_some())
            .chain(self.callers.closed.iter_mut().filter(|c| c.is_some()))
        {
            let callers = callers.as_mut().unwrap();
            for caller in callers {
                if let Some(ii) = old.iter().position(|uri| *uri == *caller) {
                    *caller = new[ii].clone();
                }
            }
        }
    }

    fn rename_macro_index_refs(&mut self, old: &[Uri], new: &[Uri]) {
        debug_assert!(old.len() > 0);
        debug_assert!(old.len() == new.len());

        fn rename_macro_entries_in_index(
            macros: &[String],
            old: &Uri,
            new: &Uri,
            registry: &mut BTreeMap<String, Vec<Uri>>,
        ) {
            for r#macro in macros {
                if let Some(files) = registry.get_mut(r#macro) {
                    for file in files.iter_mut().filter(|f| **f == **old) {
                        *file = new.to_string();
                    }
                }
            }
        }

        for (old_uri, new_uri) in old.iter().zip(new) {
            let (doc, t32) = (self.get_doc(new_uri), self.get_lang_expressions(new_uri));
            let (doc, t32) = (
                doc.expect("Must be called after rename operation."),
                t32.expect("Must be called after rename operation."),
            );

            let macros: Vec<String> = t32
                .macro_refs
                .iter()
                .map(|r| doc.text[r.clone().to_inner()].to_string())
                .collect();
            rename_macro_entries_in_index(&macros, old_uri, new_uri, &mut self.macro_index);
        }
    }

    fn update_macro_index(&mut self, doc: &TextDoc, new: &[BRange]) {
        fn remove_from_macro_idx(
            uri: &str,
            old: &[String],
            new: &[&str],
            registry: &mut BTreeMap<String, Vec<Uri>>,
        ) {
            for r#macro in old.iter().filter(|&o| !new.iter().any(|&n| *n == *o)) {
                if let Some(files) = registry.get_mut(r#macro) {
                    files.retain(|f| *f != *uri);

                    if files.is_empty() {
                        registry.remove(r#macro);
                    }
                }
            }
        }

        fn insert_into_macro_idx(
            uri: &str,
            old: &[String],
            new: &[&str],
            registry: &mut BTreeMap<String, Vec<Uri>>,
        ) {
            for &r#macro in new.iter().filter(|&&n| !old.iter().any(|o| *o == *n)) {
                if let Some(files) = registry.get_mut(r#macro) {
                    if !files.iter().any(|f| *f == *uri) {
                        files.push(uri.to_string());
                    }
                } else {
                    registry.insert(r#macro.to_string(), vec![uri.to_string()]);
                }
            }
        }

        let (old_doc, old_t32) = (self.get_doc(&doc.uri), self.get_lang_expressions(&doc.uri));
        debug_assert!(
            (old_doc.is_some() && old_t32.is_some()) || (old_doc.is_none() && old_t32.is_none())
        );

        let old_macros = if let Some(t32) = old_t32 {
            let doc = old_doc.expect("Must be in sync with lang expression availability.");
            t32.macro_refs
                .iter()
                .map(|r| doc.text[r.clone().to_inner()].to_string())
                .collect()
        } else {
            Vec::new()
        };

        let mut new_macros: Vec<&str> = new
            .iter()
            .map(|r| &doc.text[r.clone().to_inner()])
            .collect();
        new_macros.sort();
        new_macros.dedup();

        debug_assert!(old_doc.is_none() || old_doc.unwrap().uri == doc.uri);

        if old_doc.is_some() && old_macros.len() > 0 {
            remove_from_macro_idx(&doc.uri, &old_macros, &new_macros, &mut self.macro_index);
        }

        if new_macros.len() > 0 {
            insert_into_macro_idx(&doc.uri, &old_macros, &new_macros, &mut self.macro_index);
        }
    }

    fn update_file_target_index(&mut self, doc: &TextDoc, new: Option<&SubscriptCalls>) {
        let old: Vec<Uri> = {
            let mut targets: Vec<Uri> = Vec::new();

            if let Some(t32) = self.get_lang_expressions(&doc.uri)
                && let LangExpressions {
                    calls: CallExpressions { scripts, .. },
                    ..
                } = t32
                && let Some(calls) = scripts
            {
                for target in calls.targets.iter() {
                    let Some(uri) = target else {
                        continue;
                    };
                    targets.push(uri.clone());
                }
            }
            targets
        };

        if !old.is_empty() {
            self.file_target_index.remove(&doc.uri, &old);
        }
        if let Some(calls) = new {
            self.file_target_index.add(doc, calls);
        }
    }

    fn rename_files_in_file_target_index(&mut self, old: &[Uri], new: &[Uri]) {
        debug_assert!(old.len() > 0);
        debug_assert!(old.len() == new.len());

        for (old_uri, new_uri) in old.iter().zip(new) {
            self.file_target_index.rename(old_uri, new_uri);
        }
    }

    fn update_command_index(&mut self, doc: &TextDoc, new: &LangExpressions) {
        let old: Vec<String> = match self.get_doc(&doc.uri) {
            Some(d) => {
                let mut identifiers: Vec<String> = Vec::new();

                if let Some(t32) = self.get_lang_expressions(&doc.uri) {
                    if !t32.macros.privates.is_empty() {
                        identifiers.push("private".to_string());
                    }
                    if !t32.macros.locals.is_empty() {
                        identifiers.push("local".to_string());
                    }
                    if !t32.macros.globals.is_empty() {
                        identifiers.push("global".to_string());
                    }

                    if !t32.calls.subroutines.is_empty() {
                        identifiers.push("gosub".to_string());
                    }
                    if let Some(scripts) = &t32.calls.scripts {
                        if let Some(_) = scripts
                            .kinds
                            .iter()
                            .position(|k| *k == SubscriptCallKind::Do)
                        {
                            identifiers.push("do".to_string());

                            if scripts.kinds.iter().any(|k| *k == SubscriptCallKind::Run) {
                                identifiers.push("run".to_string());
                            }
                        } else {
                            identifiers.push("run".to_string());
                        }
                    }

                    if let Some(_) = t32
                        .parameters
                        .iter()
                        .position(|p| p.kind == ParameterDeclarationKind::Entry)
                    {
                        identifiers.push("entry".to_string());

                        if t32
                            .parameters
                            .iter()
                            .any(|p| p.kind == ParameterDeclarationKind::Parameters)
                        {
                            identifiers.push("parameters".to_string());
                        }
                    } else if !t32.parameters.is_empty() {
                        identifiers.push("parameters".to_string());
                    }

                    for cmd in t32.commands.iter() {
                        let (start, end) =
                            (cmd.identifier.inner().start, cmd.identifier.inner().end);
                        identifiers.push(d.text[start..end].to_lowercase());
                    }
                }
                identifiers
            }
            None => Vec::new(),
        };

        if !old.is_empty() {
            self.command_index.remove(&doc.uri, &old);
        }
        self.command_index.add(doc, new);
    }

    fn rename_files_in_command_index(&mut self, old: &[Uri], new: &[Uri]) {
        debug_assert!(old.len() > 0);
        debug_assert!(old.len() == new.len());

        for (old_uri, new_uri) in old.iter().zip(new) {
            self.command_index.rename(old_uri, new_uri);
        }
    }
}

struct FileTargetIndex(FileLocationIndex);

impl<'a> FileTargetIndex {
    pub fn new() -> Self {
        Self(FileLocationIndex::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&'a self, uri: &str) -> Option<&'a FileLocationMap> {
        self.0.get(uri)
    }

    pub fn add(&mut self, doc: &TextDoc, calls: &SubscriptCalls) {
        for (call, target) in calls.locations.iter().zip(calls.targets.iter()) {
            let Some(uri) = target else {
                continue;
            };
            let (start, end) = (call.target.start, call.target.end);

            let span = doc.to_range(start, end);
            self.0.insert(uri, &doc.uri, span);
        }
    }

    pub fn remove(&mut self, src: &str, targets: &[Uri]) {
        self.0.remove_key_locs(targets, src);
    }

    pub fn rename(&mut self, old: &Uri, new: &Uri) {
        debug_assert!(self.0.get(new).is_none());

        self.0.rename_key(old, new);
        self.0.rename_locs(old, new);
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
                find_commands_and_parameter_declarations, find_subroutines_and_labels, read_doc,
                resolve_call_expressions, textdoc::create_line_map_for_text,
            },
            workspace::{FileIndex, index_files, rename_files},
        },
        protocol::{Position, Range},
        t32::{
            self, CallExpressions, LANGUAGE_ID, find_any_macro_references, find_macro_definitions,
        },
        utils::{files, to_file_uri},
    };

    fn create_file_idx() -> FileIndex {
        let files = files();
        index_files(files)
    }

    fn create_doc_store(files: &Vec<Url>, index: &FileIndex) -> TextDocs {
        let mut members: Vec<(TextDoc, Tree, LangExpressions)> = Vec::new();
        for uri in files {
            let (doc, tree, expr) = read_doc(uri.clone(), index.clone()).expect("Must not fail.");
            members.push((doc, tree, expr));
        }
        TextDocs::from_workspace(members)
    }

    fn create_doc(
        uri: &str,
        text: &str,
        files: Option<FileIndex>,
    ) -> (TextDoc, Tree, LangExpressions) {
        let lines = create_line_map_for_text(&text, None, None);
        let doc = TextDoc {
            uri: uri.to_string(),
            lang_id: LANGUAGE_ID.to_string(),
            version: 1,
            text: text.to_string(),
            lines,
        };
        let tree = t32::parse(text.as_bytes(), None);

        let (subroutines, labels) = find_subroutines_and_labels(text, &tree);
        let (commands, parameters) = find_commands_and_parameter_declarations(text, &tree);

        let calls = if let Some(file_idx) = files {
            resolve_call_expressions(text, &tree, &file_idx)
        } else {
            CallExpressions {
                subroutines: Vec::new(),
                scripts: None,
            }
        };

        let macro_refs = find_any_macro_references(&tree);
        let macros = find_macro_definitions(text, &subroutines, &calls.subroutines, &tree);

        (
            doc,
            tree,
            LangExpressions {
                macros,
                macro_refs,
                subroutines,
                calls,
                parameters,
                labels,
                commands,
            },
        )
    }

    #[test]
    fn can_open_documents() {
        let mut docs = TextDocs::new();

        let text = "PRINT \"Hello, World!\"\n";

        let files = ["file:///a.cmm", "file:///b.cmm"];
        for uri in files.iter() {
            let (doc, tree, expr) = create_doc(*uri, &text, None);
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

        let (doc, tree, expr) = create_doc(files[0], &text, None);
        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(!docs.free_list.open.is_empty());
        assert!(!docs.free_list.closed.is_empty());

        let (doc, tree, expr) = create_doc(files[1], &text, None);
        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(docs.free_list.open.is_empty());
        assert!(!docs.free_list.closed.is_empty());
    }

    #[test]
    fn can_close_documents() {
        let mut docs = TextDocs::new();

        let text = "PRINT \"Hello, World!\"\n";
        let uri = "file:///test.cmm";
        let (doc, tree, expr) = create_doc(uri, &text, None);

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
        let docs = create_doc_store(&files, &file_idx);

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
        let mut docs = create_doc_store(&files, &file_idx);

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

    #[test]
    fn can_rename_files() {
        let files = files();
        let mut file_idx = create_file_idx();
        let mut docs = create_doc_store(&files, &file_idx);

        let old_files = [
            "tests/samples/c.cmm",
            "tests/samples/same.cmm",
            "tests/samples/a/a.cmm",
            "tests/samples/a/d/d.cmmt",
        ];
        let old_dir = "tests/samples/b";

        let new_files = [
            "tests/samples/c1.cmm",
            "tests/samples/a.cmm",
            "tests/samples/a/a1.cmm",
            "tests/samples/a/d/d1.cmmt",
        ];
        let new_dir = "tests/samples/b1";

        let mut renamed = RenameFileOperations {
            old: Vec::with_capacity(old_files.len() + old_dir.len()),
            new: Vec::with_capacity(new_files.len() + new_dir.len()),
        };

        for (old, new) in old_files.iter().zip(new_files.iter()) {
            renamed.old.push(
                Url::from_file_path(path::absolute(old).expect("File must exist."))
                    .unwrap()
                    .to_string(),
            );
            renamed.new.push(
                Url::from_file_path(path::absolute(new).expect("File must exist."))
                    .unwrap()
                    .to_string(),
            );
        }

        renamed.old.push(
            Url::from_directory_path(path::absolute(old_dir).expect("Directory must exist."))
                .unwrap()
                .to_string(),
        );
        renamed.new.push(
            Url::from_directory_path(path::absolute(new_dir).expect("Directory must exist."))
                .unwrap()
                .to_string(),
        );

        // Add non-existent file
        renamed.old.push(
            Url::from_directory_path(
                path::absolute("unknown_file.cmm").expect("Directory must exist."),
            )
            .unwrap()
            .to_string(),
        );
        renamed.new.push(
            Url::from_directory_path(path::absolute("file.cmm").expect("Directory must exist."))
                .unwrap()
                .to_string(),
        );

        let changes = rename_files(renamed.clone(), &mut file_idx);
        docs.rename_files(&changes.renamed);

        for new in new_files
            .iter()
            .chain(["tests/samples/b1/b.cmm", "tests/samples/b1/same.cmm"].iter())
        {
            let path = path::absolute(&new).expect("File must exist.");
            let target = Url::from_file_path(path).unwrap().to_string();

            assert!(docs.get_doc(&target).is_some());
        }

        for old in old_files
            .iter()
            .chain(["tests/samples/b/b.cmm", "tests/samples/b/same.cmm"].iter())
        {
            let path = path::absolute(&old).expect("File must exist.");
            let target = Url::from_file_path(path).unwrap().to_string();

            assert!(docs.get_doc(&target).is_none());
        }
    }

    #[test]
    fn can_update_macro_index() {
        let files = files();
        let file_idx = create_file_idx();
        let mut docs = create_doc_store(&files, &file_idx);

        let uri_a = to_file_uri("tests/samples/a/a.cmm");
        let uri_d = to_file_uri("tests/samples/a/d/d.cmm");
        let uri_a1 = Url::from_file_path(path::absolute("a1.cmm").unwrap())
            .unwrap()
            .to_string();

        let hits = docs
            .get_all_scripts_with_macro("&a")
            .expect("Must find references for macro.");
        assert_eq!(&hits[..], [uri_a.clone(), uri_d.clone()]);

        let text = "LOCAL &a\n&a=3\n";
        let (doc, tree, expr) = create_doc(&uri_a1, &text, None);
        docs.add(doc, tree, expr, TextDocStatus::Open);

        let hits = docs
            .get_all_scripts_with_macro("&a")
            .expect("Must find references for macro.");
        let uris = [uri_a.clone(), uri_d.clone(), uri_a1.clone()];
        assert_eq!(&hits[..], uris);

        let hits = docs
            .get_all_scripts_with_macro("&b")
            .expect("Must find references for macro.");
        let uris = [uri_a.clone()];
        assert_eq!(&hits[..], uris);

        let text = "PRINT \"Hello, World!\"\n";
        let (doc, tree, expr) = create_doc(&uri_a, &text, None);
        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(docs.get_all_scripts_with_macro("&b").is_none());

        let hits = docs
            .get_all_scripts_with_macro("&a")
            .expect("Must find references for macro.");
        let uris = [uri_d, uri_a1];
        assert_eq!(&hits[..], uris);
    }

    #[test]
    fn can_update_file_target_index() {
        let files = files();
        let file_idx = create_file_idx();
        let mut docs = create_doc_store(&files, &file_idx);

        let uri_a = to_file_uri("tests/samples/a/a.cmm");
        let uri_c = to_file_uri("tests/samples/c.cmm");
        let uri_d = to_file_uri("tests/samples/a/d/d.cmm");

        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.contains(&uri_d))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.get(&uri_d).is_some_and(|r| r
                    == &vec![Range {
                        start: Position {
                            line: 4,
                            character: 3,
                        },
                        end: Position {
                            line: 4,
                            character: 13,
                        }
                    }],))
        );

        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.contains(&uri_c))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.get(&uri_c).is_some_and(|r| r
                    == &vec![
                        Range {
                            start: Position {
                                line: 6,
                                character: 3,
                            },
                            end: Position {
                                line: 6,
                                character: 12,
                            }
                        },
                        Range {
                            start: Position {
                                line: 18,
                                character: 3,
                            },
                            end: Position {
                                line: 18,
                                character: 12,
                            }
                        },
                        Range {
                            start: Position {
                                line: 24,
                                character: 7,
                            },
                            end: Position {
                                line: 24,
                                character: 16,
                            }
                        },
                    ]))
        );

        let uri_c1 = to_file_uri("tests/samples/a/c1.cmm");
        let text = "DO a.cmm\n&a=3\n";
        let (doc, tree, expr) = create_doc(&uri_c1, &text, Some(file_idx));
        docs.add(doc, tree, expr, TextDocStatus::Open);

        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.contains(&uri_c1))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.get(&uri_c1).is_some_and(|r| r
                    == &vec![Range {
                        start: Position {
                            line: 0,
                            character: 3,
                        },
                        end: Position {
                            line: 0,
                            character: 8,
                        }
                    }],))
        );

        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.contains(&uri_d))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.get(&uri_d).is_some_and(|r| r
                    == &vec![Range {
                        start: Position {
                            line: 4,
                            character: 3,
                        },
                        end: Position {
                            line: 4,
                            character: 13,
                        }
                    }],))
        );

        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.contains(&uri_c))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.get(&uri_c).is_some_and(|r| r
                    == &vec![
                        Range {
                            start: Position {
                                line: 6,
                                character: 3,
                            },
                            end: Position {
                                line: 6,
                                character: 12,
                            }
                        },
                        Range {
                            start: Position {
                                line: 18,
                                character: 3,
                            },
                            end: Position {
                                line: 18,
                                character: 12,
                            }
                        },
                        Range {
                            start: Position {
                                line: 24,
                                character: 7,
                            },
                            end: Position {
                                line: 24,
                                character: 16,
                            }
                        },
                    ]))
        );

        let uri_c2 = to_file_uri("tests/samples/a/c2.cmm");
        docs.rename_files(&RenameFileOperations {
            old: vec![uri_c1.clone()],
            new: vec![uri_c2.clone()],
        });

        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| !r.contains(&uri_c1,))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.contains(&uri_c2,))
        );
        assert!(
            docs.get_all_target_file_refs(&uri_a)
                .is_some_and(|r| r.get(&uri_c2).is_some_and(|r| r
                    == &vec![Range {
                        start: Position {
                            line: 0,
                            character: 3,
                        },
                        end: Position {
                            line: 0,
                            character: 8,
                        }
                    }],))
        );

        let uri_a1 = to_file_uri("tests/samples/a/a1.cmm");
        docs.rename_files(&RenameFileOperations {
            old: vec![uri_a.clone()],
            new: vec![uri_a1.clone()],
        });

        assert!(docs.get_all_target_file_refs(&uri_a).is_none());
        assert!(docs.get_all_target_file_refs(&uri_a1).is_some());
    }

    #[test]
    fn can_update_command_index() {
        let files = files();
        let file_idx = create_file_idx();
        let mut docs = create_doc_store(&files, &file_idx);

        let uri_c = to_file_uri("tests/samples/c.cmm");

        let map = docs
            .get_all_command_refs("print")
            .expect("Must not be empty.");
        for file in [
            to_file_uri("tests/samples/a/a.cmm"),
            to_file_uri("tests/samples/a/d/d.cmm"),
            to_file_uri("tests/samples/a/d/d.cmmt"),
            uri_c.clone(),
        ] {
            assert!(map.get(&file).is_some());
        }
        assert!(
            map.get(&uri_c)
                .expect("Must not be empty.")
                .contains(&Range {
                    start: Position {
                        line: 4,
                        character: 0
                    },
                    end: Position {
                        line: 5,
                        character: 0
                    }
                })
        );
        assert!(
            map.get(&uri_c)
                .expect("Must not be empty.")
                .contains(&Range {
                    start: Position {
                        line: 31,
                        character: 0
                    },
                    end: Position {
                        line: 32,
                        character: 0
                    }
                })
        );

        let uri_c1 = to_file_uri("tests/samples/a/c1.cmm");
        let text = "PRINT \"TEST\"&a=1.\n";
        let (doc, tree, expr) = create_doc(&uri_c1, &text, Some(file_idx.clone()));

        docs.add(doc, tree, expr, TextDocStatus::Open);

        let map = docs
            .get_all_command_refs("print")
            .expect("Must not be empty.");
        assert!(
            map.get(&uri_c1)
                .expect("Must not be empty.")
                .contains(&Range {
                    start: Position {
                        line: 0,
                        character: 0
                    },
                    end: Position {
                        line: 1,
                        character: 0
                    }
                })
        );

        let text = "SYStem.CPU M68K\nSYStem.Up\n";
        let (doc, tree, expr) = create_doc(&uri_c1, &text, Some(file_idx.clone()));

        docs.update(doc, tree, expr);

        let map = docs
            .get_all_command_refs("print")
            .expect("Must not be empty.");
        assert!(map.get(&uri_c1).is_none());

        let map = docs
            .get_all_command_refs("system.cpu")
            .expect("Must not be empty.");
        assert!(map.get(&uri_c1).is_some());

        let uri_c2 = to_file_uri("tests/samples/a/c2.cmm");

        docs.rename_files(&RenameFileOperations {
            old: vec![uri_c1.clone()],
            new: vec![uri_c2.clone()],
        });

        let map = docs
            .get_all_command_refs("system.cpu")
            .expect("Must not be empty.");
        assert!(map.get(&uri_c1).is_none());
        assert!(map.get(&uri_c2).is_some());
    }
}
