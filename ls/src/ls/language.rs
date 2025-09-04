// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ops::Range;

use tree_sitter::{Tree, TreeCursor};

use crate::{
    ls::doc::{GlobalMacroDefIndex, TextDoc, TextDocData, TextDocs},
    protocol::{LocationLink, Position, Range as LRange, Uri},
    t32::{
        MacroDefinition, MacroDefinitionResult, NodeKind, Subroutine, get_goto_ref_ids,
        goto_external_macro_definition, goto_file, goto_macro_definition,
        goto_subroutine_definition, id_into_node,
    },
    utils::BRange,
};

#[derive(Debug)]
pub enum GotoDefinitionResult {
    Final(Vec<LocationLink>),
    PartialMacro(Uri, String, LRange, Vec<LocationLink>),
}

#[derive(Clone, Debug)]
pub struct ExtMacroDefOrigin {
    pub name: String,
    pub span: LRange,
    pub uri: Uri,
}

impl LocationLink {
    pub fn build(
        doc: &TextDoc,
        origin: Option<Range<usize>>,
        uri: Uri,
        target_range: Range<usize>,
        target_sel: Range<usize>,
    ) -> Self {
        LocationLink {
            origin_selection_range: match origin {
                Some(range) => Some(doc.to_range(range.start, range.end)),
                None => None,
            },
            target_uri: uri,
            target_range: doc.to_range(target_range.start, target_range.end),
            target_selection_range: doc.to_range(target_sel.start, target_sel.end),
        }
    }

    pub fn from_macro_def(doc: &TextDoc, origin: BRange, r#macro: MacroDefinition) -> Self {
        let (target_range, target_sel) = if let Some(docstring) = r#macro.docstring {
            let start: Range<usize> = Range {
                start: docstring.start,
                end: r#macro.cmd.end,
            };
            (start, r#macro.r#macro)
        } else {
            (r#macro.cmd, r#macro.r#macro)
        };

        LocationLink::build(
            &doc,
            Some(origin.into()),
            doc.uri.clone(),
            target_range,
            target_sel,
        )
    }

    pub fn from_ext_macro_def(doc: &TextDoc, origin: LRange, r#macro: MacroDefinition) -> Self {
        let (target_range, target_sel) = if let Some(docstring) = r#macro.docstring {
            let start: Range<usize> = Range {
                start: docstring.start,
                end: r#macro.cmd.end,
            };
            (start, r#macro.r#macro)
        } else {
            (r#macro.cmd, r#macro.r#macro)
        };

        LocationLink {
            origin_selection_range: Some(origin),
            target_uri: doc.uri.clone(),
            target_range: doc.to_range(target_range.start, target_range.end),
            target_selection_range: doc.to_range(target_sel.start, target_sel.end),
        }
    }
}

/// Retrieves definitions for `(macro)`, `(subroutine_call_expression)`, and
/// `(command_expression)` nodes. `(command_expression)` nodes capture
/// `DO` and `RUN` commands for subscripts calls.
pub fn find_definition(textdoc: TextDocData, position: Position) -> Option<GotoDefinitionResult> {
    let offset = textdoc.doc.to_byte_offset(&position);

    let lang = textdoc.tree.language();
    let allowed_kinds = get_goto_ref_ids(&lang);

    let origin = find_deepest_node(&textdoc.tree, offset, &allowed_kinds)?;

    let origin_span = origin.node().range();

    match id_into_node(&lang, origin.node().kind_id()) {
        NodeKind::Macro => {
            match goto_macro_definition(&textdoc.doc.text, &textdoc.tree, &textdoc.t32, origin)? {
                MacroDefinitionResult::Final(gotos) => {
                    let mut links: Vec<LocationLink> = Vec::with_capacity(gotos.len());
                    for def in gotos {
                        links.push(LocationLink::from_macro_def(
                            &textdoc.doc,
                            origin_span.into(),
                            def,
                        ));
                    }
                    Some(GotoDefinitionResult::Final(links))
                }
                MacroDefinitionResult::Partial(name, gotos) => {
                    let mut links: Vec<LocationLink> = Vec::with_capacity(gotos.len());
                    for def in gotos {
                        links.push(LocationLink::from_macro_def(
                            &textdoc.doc,
                            origin_span.into(),
                            def,
                        ));
                    }

                    let span = textdoc
                        .doc
                        .to_range(origin_span.start_byte, origin_span.end_byte);
                    Some(GotoDefinitionResult::PartialMacro(
                        textdoc.doc.uri,
                        name,
                        span,
                        links,
                    ))
                }
                MacroDefinitionResult::Indeterminate(name) => {
                    let span = textdoc
                        .doc
                        .to_range(origin_span.start_byte, origin_span.end_byte);
                    Some(GotoDefinitionResult::PartialMacro(
                        textdoc.doc.uri,
                        name,
                        span,
                        Vec::new(),
                    ))
                }
            }
        }
        NodeKind::CommandExpression => {
            let uri = goto_file(&textdoc.doc.text, &textdoc.t32.calls.scripts?, origin)?;

            // Point to start of called script file
            Some(GotoDefinitionResult::Final(vec![LocationLink::build(
                &textdoc.doc,
                Some(Range {
                    start: origin_span.start_byte,
                    end: origin_span.end_byte,
                }),
                uri,
                Range { start: 0, end: 1 },
                Range { start: 0, end: 1 },
            )]))
        }
        NodeKind::SubroutineCallExpression => {
            let sub: Subroutine =
                goto_subroutine_definition(&textdoc.doc.text, &textdoc.t32.subroutines?, origin)?;
            let (target_range, target_sel) = if let Some(docstring) = sub.docstring {
                let start: Range<usize> = Range {
                    start: docstring.start,
                    end: sub.definition.end,
                };
                (start, sub.name.clone())
            } else {
                (sub.definition.clone(), sub.name.clone())
            };

            Some(GotoDefinitionResult::Final(vec![LocationLink::build(
                &textdoc.doc,
                Some(Range {
                    start: origin_span.start_byte,
                    end: origin_span.end_byte,
                }),
                textdoc.doc.uri.clone(),
                target_range,
                target_sel,
            )]))
        }
        _ => {
            unreachable!("No other node kinds can be traced back to definition. Must abort early.")
        }
    }
}

pub fn find_external_macro_definition(
    textdoc: TextDocData,
    callers: Vec<Uri>,
    origin: ExtMacroDefOrigin,
) -> (Option<GotoDefinitionResult>, Vec<Uri>) {
    if textdoc.t32.calls.scripts.is_none() {
        return (None, callers);
    }

    let mut targets: Vec<Range<usize>> = Vec::with_capacity(1);

    for (_, loc) in textdoc
        .t32
        .calls
        .scripts
        .as_ref()
        .unwrap()
        .targets
        .iter()
        .zip(textdoc.t32.calls.scripts.as_ref().unwrap().locations.iter())
        .filter(|&(t, _)| t.is_some() && *t.as_ref().unwrap() == origin.uri)
    {
        targets.push(loc.call.clone());
    }
    if targets.is_empty() {
        return (None, callers);
    }

    let ExtMacroDefOrigin {
        name: r#macro,
        span,
        ..
    } = origin;

    match goto_external_macro_definition(
        &textdoc.doc.text,
        &textdoc.tree,
        &textdoc.t32,
        r#macro,
        targets,
    ) {
        Some(MacroDefinitionResult::Final(gotos)) => {
            let mut links: Vec<LocationLink> = Vec::with_capacity(gotos.len());
            for def in gotos {
                links.push(LocationLink::from_ext_macro_def(
                    &textdoc.doc,
                    span.clone(),
                    def,
                ));
            }
            (Some(GotoDefinitionResult::Final(links)), callers)
        }
        Some(MacroDefinitionResult::Partial(name, gotos)) => {
            let mut links: Vec<LocationLink> = Vec::with_capacity(gotos.len());
            for def in gotos {
                links.push(LocationLink::from_ext_macro_def(
                    &textdoc.doc,
                    span.clone(),
                    def,
                ));
            }

            (
                Some(GotoDefinitionResult::PartialMacro(
                    textdoc.doc.uri,
                    name,
                    span.clone(),
                    links,
                )),
                callers,
            )
        }
        Some(MacroDefinitionResult::Indeterminate(name)) => (
            Some(GotoDefinitionResult::PartialMacro(
                textdoc.doc.uri,
                name,
                span,
                Vec::new(),
            )),
            callers,
        ),
        None => (None, callers),
    }
}

pub fn find_global_macro_definitions(
    docs: &TextDocs,
    macros: GlobalMacroDefIndex,
    origin: ExtMacroDefOrigin,
) -> Vec<LocationLink> {
    let mut links: Vec<LocationLink> = Vec::new();

    let mut base: u32 = 0;
    for (uri, num) in macros.0.into_iter().zip(macros.1.into_iter()) {
        if uri == origin.uri {
            base += num;
            continue;
        }

        for (&r#macro, &def) in macros.2[base as usize..(base + num) as usize]
            .into_iter()
            .zip(macros.3[base as usize..(base + num) as usize].into_iter())
        {
            if *r#macro != origin.name {
                continue;
            }
            let doc = docs.get_doc(&uri).expect("Document must exist.");
            links.push(LocationLink::from_ext_macro_def(
                doc,
                origin.span.clone(),
                def.clone(),
            ));
        }
        base += num;
    }
    links
}

fn find_deepest_node<'a>(tree: &'a Tree, offset: usize, stop_at: &[u16]) -> Option<TreeCursor<'a>> {
    let mut cursor = tree.walk();
    let mut sel: Option<TreeCursor> = None;

    while let Some(_) = cursor.goto_first_child_for_byte(offset) {
        let node = cursor.node();
        if !node.byte_range().contains(&offset) {
            break;
        }

        let id = node.kind_id();
        if let Some(_) = stop_at.iter().find(|k| **k == id) {
            sel = Some(cursor.clone());
        }
    }
    sel
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{env, path};
    use url::Url;

    use crate::{
        ls::{
            doc::{read_doc, resolve_call_expressions},
            workspace::{self, FileIndex},
        },
        protocol::Range,
        t32::{self, LangExpressions},
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
        workspace::index_files(files())
    }

    fn find_def(file: &str, position: Position) -> Option<GotoDefinitionResult> {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();
        let doc = TextDoc::try_from(uri).expect("Path must be valid.");
        let file_idx = workspace::index_files(files());

        let tree = t32::parse(doc.text.as_bytes(), None);

        let macros = t32::find_macro_definitions(&doc.text, &tree);
        let subroutines = t32::find_subroutines(&doc.text, &tree);
        let parameters = t32::find_parameter_declarations(&doc.text, &tree);
        let calls = resolve_call_expressions(&doc.text, &tree, &file_idx);

        find_definition(
            TextDocData {
                doc,
                tree,
                t32: LangExpressions {
                    macros,
                    subroutines,
                    calls,
                    parameters,
                },
            },
            position,
        )
    }

    fn find_external_macro_def(
        file: &str,
        callers: Vec<Uri>,
        origin: ExtMacroDefOrigin,
    ) -> (Option<GotoDefinitionResult>, Vec<Uri>) {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();
        let doc = TextDoc::try_from(uri).expect("Path must be valid.");
        let file_idx = workspace::index_files(files());

        let tree = t32::parse(doc.text.as_bytes(), None);

        let macros = t32::find_macro_definitions(&doc.text, &tree);
        let subroutines = t32::find_subroutines(&doc.text, &tree);
        let parameters = t32::find_parameter_declarations(&doc.text, &tree);
        let calls = resolve_call_expressions(&doc.text, &tree, &file_idx);

        find_external_macro_definition(
            TextDocData {
                doc,
                tree,
                t32: LangExpressions {
                    macros,
                    subroutines,
                    calls,
                    parameters,
                },
            },
            callers,
            origin,
        )
    }

    #[test]
    fn can_find_private_macro_definition() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 8,
                character: 0,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let GotoDefinitionResult::Final(loc) = loc.expect("Must not be empty.") else {
            unreachable!();
        };

        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 8,
                        character: 0,
                    },
                    end: Position {
                        line: 8,
                        character: 14,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 6,
                        character: 0,
                    },
                    end: Position {
                        line: 7,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 6,
                        character: 8,
                    },
                    end: Position {
                        line: 6,
                        character: 22,
                    },
                },
            }]
        ));
    }

    #[test]
    fn can_find_macro_definition_with_docstring() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 22,
                character: 21,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let GotoDefinitionResult::Final(loc) = loc.expect("Must not be empty.") else {
            unreachable!();
        };
        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 22,
                        character: 13,
                    },
                    end: Position {
                        line: 22,
                        character: 26,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 15,
                        character: 0,
                    },
                    end: Position {
                        line: 19,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 18,
                        character: 12,
                    },
                    end: Position {
                        line: 18,
                        character: 25,
                    },
                },
            }]
        ));
    }

    #[test]
    fn can_find_global_macro_definition() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 133,
                character: 14,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let GotoDefinitionResult::Final(loc) = loc.expect("Must not be empty.") else {
            unreachable!();
        };
        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 133,
                        character: 11,
                    },
                    end: Position {
                        line: 133,
                        character: 25,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 136,
                        character: 0,
                    },
                    end: Position {
                        line: 137,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 136,
                        character: 7,
                    },
                    end: Position {
                        line: 136,
                        character: 21,
                    },
                },
            }]
        ));
    }

    #[test]
    fn can_find_macro_definition_inside_subroutine() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 29,
                character: 11,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let GotoDefinitionResult::Final(loc) = loc.expect("Must not be empty.") else {
            unreachable!();
        };
        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 29,
                        character: 10,
                    },
                    end: Position {
                        line: 29,
                        character: 12,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 28,
                        character: 4,
                    },
                    end: Position {
                        line: 29,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 28,
                        character: 12,
                    },
                    end: Position {
                        line: 28,
                        character: 14,
                    },
                },
            }]
        ));
    }

    #[test]
    fn can_find_outside_macro_definition_for_subroutine() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 38,
                character: 10,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let GotoDefinitionResult::Final(loc) = loc.expect("Must not be empty.") else {
            unreachable!();
        };
        assert!(matches!(
            &loc[0],
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 38,
                        character: 4,
                    },
                    end: Position {
                        line: 38,
                        character: 16,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 42,
                        character: 0,
                    },
                    end: Position {
                        line: 43,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 42,
                        character: 6,
                    },
                    end: Position {
                        line: 42,
                        character: 18,
                    },
                },
            }
        ));
    }

    #[test]
    fn can_find_macro_definition_across_subroutine_calls() {
        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");

        let uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        for (loc, _link) in [
            Position {
                line: 58,
                character: 22,
            },
            Position {
                line: 58,
                character: 30,
            },
        ]
        .into_iter()
        .zip([
            [
                LocationLink {
                    origin_selection_range: Some(Range {
                        start: Position {
                            line: 58,
                            character: 11,
                        },
                        end: Position {
                            line: 58,
                            character: 23,
                        },
                    }),
                    target_uri: uri.to_string(),
                    target_range: Range {
                        start: Position {
                            line: 65,
                            character: 4,
                        },
                        end: Position {
                            line: 66,
                            character: 0,
                        },
                    },
                    target_selection_range: Range {
                        start: Position {
                            line: 65,
                            character: 10,
                        },
                        end: Position {
                            line: 65,
                            character: 22,
                        },
                    },
                },
                LocationLink {
                    origin_selection_range: Some(Range {
                        start: Position {
                            line: 58,
                            character: 11,
                        },
                        end: Position {
                            line: 58,
                            character: 23,
                        },
                    }),
                    target_uri: uri.to_string(),
                    target_range: Range {
                        start: Position {
                            line: 72,
                            character: 4,
                        },
                        end: Position {
                            line: 73,
                            character: 0,
                        },
                    },
                    target_selection_range: Range {
                        start: Position {
                            line: 72,
                            character: 10,
                        },
                        end: Position {
                            line: 72,
                            character: 22,
                        },
                    },
                },
            ],
            [
                LocationLink {
                    origin_selection_range: Some(Range {
                        start: Position {
                            line: 58,
                            character: 26,
                        },
                        end: Position {
                            line: 58,
                            character: 38,
                        },
                    }),
                    target_uri: uri.to_string(),
                    target_range: Range {
                        start: Position {
                            line: 61,
                            character: 0,
                        },
                        end: Position {
                            line: 62,
                            character: 0,
                        },
                    },
                    target_selection_range: Range {
                        start: Position {
                            line: 61,
                            character: 6,
                        },
                        end: Position {
                            line: 61,
                            character: 18,
                        },
                    },
                },
                LocationLink {
                    origin_selection_range: Some(Range {
                        start: Position {
                            line: 58,
                            character: 26,
                        },
                        end: Position {
                            line: 58,
                            character: 38,
                        },
                    }),
                    target_uri: uri.to_string(),
                    target_range: Range {
                        start: Position {
                            line: 73,
                            character: 4,
                        },
                        end: Position {
                            line: 74,
                            character: 0,
                        },
                    },
                    target_selection_range: Range {
                        start: Position {
                            line: 73,
                            character: 10,
                        },
                        end: Position {
                            line: 73,
                            character: 22,
                        },
                    },
                },
            ],
        ]) {
            let def = find_def("tests/samples/a/a.cmm", loc);
            assert!(matches!(def, _link));
        }
    }

    #[test]
    fn can_identify_implicit_macro_definitions() {
        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");

        let uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        for (loc, _link) in [
            Position {
                line: 84,
                character: 11,
            },
            Position {
                line: 91,
                character: 12,
            },
            Position {
                line: 100,
                character: 11,
            },
            Position {
                line: 107,
                character: 11,
            },
            Position {
                line: 115,
                character: 11,
            },
        ]
        .into_iter()
        .zip([
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 84,
                        character: 11,
                    },
                    end: Position {
                        line: 84,
                        character: 13,
                    },
                }),
                target_uri: uri.to_string(),
                target_range: Range {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 82,
                        character: 10,
                    },
                    end: Position {
                        line: 82,
                        character: 12,
                    },
                },
            },
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 91,
                        character: 11,
                    },
                    end: Position {
                        line: 91,
                        character: 13,
                    },
                }),
                target_uri: uri.to_string(),
                target_range: Range {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 82,
                        character: 10,
                    },
                    end: Position {
                        line: 82,
                        character: 12,
                    },
                },
            },
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 100,
                        character: 11,
                    },
                    end: Position {
                        line: 100,
                        character: 13,
                    },
                }),
                target_uri: uri.to_string(),
                target_range: Range {
                    start: Position {
                        line: 98,
                        character: 4,
                    },
                    end: Position {
                        line: 99,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 98,
                        character: 15,
                    },
                    end: Position {
                        line: 98,
                        character: 17,
                    },
                },
            },
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 107,
                        character: 11,
                    },
                    end: Position {
                        line: 107,
                        character: 13,
                    },
                }),
                target_uri: uri.to_string(),
                target_range: Range {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 82,
                        character: 10,
                    },
                    end: Position {
                        line: 82,
                        character: 12,
                    },
                },
            },
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 115,
                        character: 11,
                    },
                    end: Position {
                        line: 115,
                        character: 13,
                    },
                }),
                target_uri: uri.to_string(),
                target_range: Range {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 82,
                        character: 10,
                    },
                    end: Position {
                        line: 82,
                        character: 12,
                    },
                },
            },
        ]) {
            let def = find_def("tests/samples/a/a.cmm", loc);
            assert!(matches!(def, _link));
        }
    }

    #[test]
    fn can_break_recursion_loops_for_subroutine_macro_defs() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 127,
                character: 13,
            },
        );
        assert!(loc.is_none());
    }

    #[test]
    fn can_find_subscript_call_target() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 49,
                character: 8,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let GotoDefinitionResult::Final(loc) = loc.expect("Must not be empty.") else {
            unreachable!();
        };
        assert!(matches!(
            &loc[0],
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 49,
                        character: 0,
                    },
                    end: Position {
                        line: 50,
                        character: 0,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
            }
        ));
    }

    #[test]
    fn can_find_macro_definition_covering_subscript_call() {
        let mut callers: Vec<Uri> = Vec::new();
        for file in ["tests/samples/c.cmm"].into_iter() {
            callers.push(
                Url::from_file_path(path::absolute(file).expect("File must exist."))
                    .unwrap()
                    .to_string(),
            );
        }

        for (job, _link) in [
            (
                "tests/samples/a/a.cmm",
                callers.clone(),
                ExtMacroDefOrigin {
                    name: "&local_macro".to_string(),
                    span: Range {
                        start: Position {
                            line: 17,
                            character: 6,
                        },
                        end: Position {
                            line: 17,
                            character: 18,
                        },
                    },
                    uri: Url::from_file_path(
                        path::absolute("tests/samples/c.cmm").expect("File must exist."),
                    )
                    .unwrap()
                    .to_string(),
                },
            ),
            (
                "tests/samples/c.cmm",
                callers.clone(),
                ExtMacroDefOrigin {
                    name: "&from_c_cmm".to_string(),
                    span: Range {
                        start: Position {
                            line: 139,
                            character: 7,
                        },
                        end: Position {
                            line: 139,
                            character: 18,
                        },
                    },
                    uri: Url::from_file_path(
                        path::absolute("tests/samples/a/a.cmm").expect("File must exist."),
                    )
                    .unwrap()
                    .to_string(),
                },
            ),
        ]
        .into_iter()
        .zip([
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 17,
                        character: 6,
                    },
                    end: Position {
                        line: 17,
                        character: 18,
                    },
                }),
                target_uri: Url::from_file_path(
                    path::absolute("tests/samples/a/a.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
                target_range: Range {
                    start: Position {
                        line: 42,
                        character: 0,
                    },
                    end: Position {
                        line: 43,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 42,
                        character: 6,
                    },
                    end: Position {
                        line: 42,
                        character: 18,
                    },
                },
            },
            LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 139,
                        character: 7,
                    },
                    end: Position {
                        line: 139,
                        character: 18,
                    },
                }),
                target_uri: Url::from_file_path(
                    path::absolute("tests/samples/a/a.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
                target_range: Range {
                    start: Position {
                        line: 22,
                        character: 4,
                    },
                    end: Position {
                        line: 23,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 22,
                        character: 10,
                    },
                    end: Position {
                        line: 22,
                        character: 21,
                    },
                },
            },
        ]) {
            let (Some(GotoDefinitionResult::Final(loc)), successors) =
                find_external_macro_def(job.0, job.1, job.2)
            else {
                unreachable!();
            };

            assert!(matches!(&loc[..], _link));
            assert_eq!(successors, callers);
        }
    }

    #[test]
    fn can_find_macro_global_macro_definition() {
        let files = files();
        let file_idx = create_file_idx();

        let mut members: Vec<(TextDoc, Tree, LangExpressions)> = Vec::new();
        for uri in files {
            let (doc, tree, expr) =
                read_doc(uri.clone(), file_idx.clone()).expect("Must not fail.");
            members.push((doc, tree, expr));
        }

        let docs = TextDocs::from_workspace(members);

        let globals = docs.get_all_global_macros().expect("Must not fail.");

        let links = find_global_macro_definitions(
            &docs,
            globals,
            ExtMacroDefOrigin {
                name: "&global_macro".to_string(),
                span: Range {
                    start: Position {
                        line: 31,
                        character: 7,
                    },
                    end: Position {
                        line: 31,
                        character: 20,
                    },
                },
                uri: Url::from_file_path(
                    path::absolute("tests/samples/c.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
            },
        );

        let _uri =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."));

        assert!(matches!(
            &links[..],
            [LocationLink {
                origin_selection_range: Some(Range {
                    start: Position {
                        line: 31,
                        character: 7,
                    },
                    end: Position {
                        line: 31,
                        character: 20,
                    },
                }),
                target_uri: _uri,
                target_range: Range {
                    start: Position {
                        line: 41,
                        character: 0,
                    },
                    end: Position {
                        line: 42,
                        character: 0,
                    },
                },
                target_selection_range: Range {
                    start: Position {
                        line: 41,
                        character: 7,
                    },
                    end: Position {
                        line: 41,
                        character: 20,
                    },
                },
            }]
        ));
    }
}
