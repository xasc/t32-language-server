// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ops::Range;

use tree_sitter::{Tree, TreeCursor};

use crate::{
    ls::doc::TextDoc,
    protocol::{LocationLink, Position, Uri},
    t32::{LangExpressions, NodeKind, get_goto_ref_ids, goto_macro_definition, id_into_node},
};

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
}

/// Retrieves definitions for `(macro)`, `(subroutine_call_expression)`, and
/// `(command_expression)` nodes.
pub fn find_definition(
    doc: TextDoc,
    tree: Tree,
    t32: LangExpressions,
    position: Position,
) -> Option<Vec<LocationLink>> {
    let offset = doc.to_byte_offset(&position);

    let lang = tree.language();
    let allowed_kinds = get_goto_ref_ids(&lang);

    let origin = find_deepest_node(&tree, offset, &allowed_kinds)?;
    let origin_range = origin.node().range();

    let mut links: Vec<LocationLink> = Vec::with_capacity(1);
    match id_into_node(&lang, origin.node().kind_id()) {
        NodeKind::Macro => {
            for def in goto_macro_definition(&doc.text, &tree, &t32, origin)? {
                let (target_range, target_sel) = if let Some(docstring) = def.docstring {
                    let start: Range<usize> = Range {
                        start: docstring.start,
                        end: def.definition.end,
                    };

                    (start, def.r#macro)
                } else {
                    (def.definition, def.r#macro)
                };

                links.push(LocationLink::build(
                    &doc,
                    Some(Range {
                        start: origin_range.start_byte,
                        end: origin_range.end_byte,
                    }),
                    doc.uri.clone(),
                    target_range,
                    target_sel,
                ));
            }
        }
        NodeKind::CommandExpression => todo!(),
        NodeKind::SubroutineCallExpression => todo!(),
        _ => unreachable!("No other node kinds can be traced back to definition."),
    };

    if links.len() > 0 { Some(links) } else { None }
}

fn find_deepest_node<'a>(
    tree: &'a Tree,
    offset: usize,
    allowed_kinds: &[u16],
) -> Option<TreeCursor<'a>> {
    let mut cursor = tree.walk();
    let mut sel: Option<TreeCursor> = None;

    while let Some(_) = cursor.goto_first_child_for_byte(offset) {
        let node = cursor.node();
        if !node.byte_range().contains(&offset) {
            break;
        }

        let id = node.kind_id();
        if let Some(_) = allowed_kinds.iter().find(|k| **k == id) {
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
        ls::{doc::resolve_call_expressions, workspace},
        protocol::Range,
        t32,
    };

    fn find_def(file: &str, position: Position) -> Option<Vec<LocationLink>> {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();
        let doc = TextDoc::try_from(uri).expect("Path must be valid.");
        let files = workspace::FileIndex::new();

        let tree = t32::parse(doc.text.as_bytes(), None);

        let macros = t32::find_global_macro_definitions(&doc.text, &tree);
        let subroutines = t32::find_subroutines(&doc.text, &tree);
        let calls = resolve_call_expressions(&doc.text, &tree, &files);

        find_definition(
            doc,
            tree,
            LangExpressions {
                macros,
                subroutines,
                calls,
            },
            position,
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

        let loc = loc.expect("Must not be empty.");
        assert_eq!(loc.len(), 1);
        assert!(matches!(
            &loc[0],
            LocationLink {
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
            }
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

        let loc = loc.expect("Must not be empty.");
        assert_eq!(loc.len(), 1);
        assert!(matches!(
            &loc[0],
            LocationLink {
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
            }
        ));
    }

    #[test]
    fn can_find_macro_definition_from_subroutine() {
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

        let loc = loc.expect("Must not be empty.");
        assert_eq!(loc.len(), 1);
        assert!(matches!(
            &loc[0],
            LocationLink {
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
            }
        ));
    }

    #[test]
    fn can_find_external_macro_definition_from_subroutine() {
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

        let loc = loc.expect("Must not be empty.");
        assert_eq!(loc.len(), 1);
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
    fn can_find_macro_definition_over_subroutine_calls() {
        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 58,
                character: 22,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let loc = loc.expect("Must not be empty.");
        assert_eq!(loc.len(), 2);
        assert!(matches!(
            &loc[0],
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
                target_uri: _uri,
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
            }
        ));

        assert!(matches!(
            &loc[1],
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
                target_uri: _uri,
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
            }
        ));

        let loc = find_def(
            "tests/samples/a/a.cmm",
            Position {
                line: 58,
                character: 30,
            },
        );

        let file = env::current_dir()
            .unwrap()
            .join("tests")
            .join("samples")
            .join("a")
            .join("a.cmm");
        let _uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let loc = loc.expect("Must not be empty.");
        assert_eq!(loc.len(), 2);
        assert!(matches!(
            &loc[0],
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
                target_uri: _uri,
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
            }
        ));
        assert!(matches!(
            &loc[1],
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
                target_uri: _uri,
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
            }
        ));
    }
}
