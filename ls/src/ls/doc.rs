// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod store;
mod textdoc;

use url::Url;

use tree_sitter::Tree;

use crate::{
    ls::workspace::FileIndex,
    protocol::{TextDocumentContentChangeEvent, TextDocumentItem, Uri},
    t32::{
        self, CallExpression, CallExpressions, CallLocations, LangExpressions, SubscriptCalls,
        find_call_expressions, find_macro_definitions, find_parameter_declarations,
        find_subroutines, resolve_subscript_call_targets,
    },
};

pub use store::{GlobalMacroDefIndex, TextDocData, TextDocs};
pub use textdoc::{TextDoc, TextDocStatus};

pub fn import_doc(r#in: TextDocumentItem, files: FileIndex) -> (TextDoc, Tree, LangExpressions) {
    let doc = TextDoc::from(r#in);
    let tree = t32::parse(doc.text.as_bytes(), None);

    let macros = find_macro_definitions(&doc.text, &tree);
    let subroutines = find_subroutines(&doc.text, &tree);
    let parameters = find_parameter_declarations(&doc.text, &tree);
    let calls = resolve_call_expressions(&doc.text, &tree, &files);

    (
        doc,
        tree,
        LangExpressions {
            macros,
            subroutines,
            calls,
            parameters,
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

    let macros = find_macro_definitions(&doc.text, &tree);
    let subroutines = find_subroutines(&doc.text, &tree);
    let parameters = find_parameter_declarations(&doc.text, &tree);
    let calls = resolve_call_expressions(&doc.text, &tree, &files);

    (
        doc,
        tree,
        LangExpressions {
            macros,
            subroutines,
            calls,
            parameters,
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

    let macros = find_macro_definitions(&doc.text, &tree);
    let subroutines = find_subroutines(&doc.text, &tree);
    let parameters = find_parameter_declarations(&doc.text, &tree);
    let calls = resolve_call_expressions(&doc.text, &tree, &files);

    Ok((
        doc,
        tree,
        LangExpressions {
            macros,
            subroutines,
            calls,
            parameters,
        },
    ))
}

pub fn resolve_call_expressions(text: &str, tree: &Tree, files: &FileIndex) -> CallExpressions {
    let CallLocations {
        subroutines,
        scripts,
    } = find_call_expressions(text, &tree);

    let subscripts: Option<SubscriptCalls>;
    if scripts.len() > 0 {
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
        subscripts = Some(SubscriptCalls { locations, targets });
    } else {
        subscripts = None;
    }
    CallExpressions {
        subroutines,
        scripts: subscripts,
    }
}

#[cfg(test)]
mod test {
    use std::path;

    use super::*;

    use crate::{
        ls::workspace::index_files,
        t32::{CallExpressions, MacroDefinitions},
    };

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
    fn can_find_end_of_subroutines_from_labeled_expression() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (doc, _, LangExpressions { subroutines, .. }) =
            read_doc(file, file_idx).expect("Must not fail.");

        assert!(!subroutines.clone().is_none_or(|s| s.is_empty()));

        for name in ["subN", "subO"].iter() {
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
    fn can_find_global_scoped_macros() {
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
    fn can_find_local_scoped_macros() {
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
                .find_map(|s| (doc.text[s.r#macro.clone()] == *"&local_macro").then_some(()))
                .is_some()
        );
    }

    #[test]
    fn can_find_private_scoped_macros() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (
            doc,
            _,
            LangExpressions {
                macros: MacroDefinitions { privates, .. },
                ..
            },
        ) = read_doc(file, file_idx).expect("Must not fail.");

        assert!(!privates.clone().is_none_or(|s| s.is_empty()));
        assert!(
            privates
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (doc.text[s.r#macro.clone()] == *"&private_macro").then_some(()))
                .is_some()
        );
    }

    #[test]
    fn can_find_parameters() {
        let file_idx = FileIndex::new();

        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();

        let (doc, _, LangExpressions { parameters, .. }) =
            read_doc(file, file_idx).expect("Must not fail.");

        assert!(!parameters.clone().is_none_or(|s| s.is_empty()));
        assert!(
            parameters
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (doc.text[s.r#macro.clone()] == *"&b").then_some(()))
                .is_some()
        );
        assert!(
            parameters
                .as_ref()
                .unwrap()
                .iter()
                .find_map(|s| (doc.text[s.r#macro.clone()] == *"&x").then_some(()))
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

        assert!(subroutines.len() > 0);
        assert!(
            subroutines
                .iter()
                .find_map(|s| (doc.text[s.target.clone()] == *"subA").then_some(()))
                .is_some()
        );
        assert!(
            subroutines
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

        let target =
            Url::from_file_path(path::absolute("tests/samples/b/b.cmm").expect("File must exist."))
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
