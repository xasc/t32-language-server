// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use tree_sitter::{Tree, TreeCursor};

use crate::{
    ls::textdoc::TextDoc,
    protocol::{LocationLink, Position},
    t32::{NodeKind, get_goto_ref_ids, goto_macro_definition, id_into_node},
};

/// Retrieves definitions for `(macro)`, `(subroutine_call_expression)`, and
/// `(command_expression)` nodes.
pub fn find_definition(doc: TextDoc, tree: Tree, position: Position) -> Option<LocationLink> {
    let offset = doc.to_byte_offset(&position);

    let lang = tree.language();
    let allowed_kinds = get_goto_ref_ids(&lang);

    let origin = find_selected_node(&tree, offset, &allowed_kinds)?;
    let origin_range = origin.node().range();

    let (target_range, target_selection_range) = match id_into_node(&lang, origin.node().kind_id())
    {
        NodeKind::Macro => {
            if let Some(macro_def) = goto_macro_definition(&doc.text, &tree, origin) {
                (macro_def.definition, macro_def.r#macro)
            } else {
                return None;
            }
        }
        NodeKind::CommandExpression => todo!(),
        NodeKind::SubroutineCallExpression => todo!(),
        _ => unreachable!("No other node kinds can be traced back to definition."),
    };

    Some(LocationLink {
        origin_selection_range: Some(doc.to_range(origin_range.start_byte, origin_range.end_byte)),
        target_uri: doc.uri.clone(),
        target_range: doc.to_range(target_range.start, target_range.end),
        target_selection_range: doc
            .to_range(target_selection_range.start, target_selection_range.end),
    })
}

fn find_selected_node<'a>(
    tree: &'a Tree,
    offset: usize,
    allowed_kinds: &[u16],
) -> Option<TreeCursor<'a>> {
    let mut cursor = tree.walk();
    while let Some(_) = cursor.goto_first_child_for_byte(offset) {
        let node = cursor.node();
        if !node.byte_range().contains(&offset) {
            return None;
        }

        let id = node.kind_id();
        if let Some(_) = allowed_kinds.iter().find(|k| **k == id) {
            return Some(cursor);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{env, path};
    use url::Url;

    use crate::{protocol::Range, t32};

    #[test]
    fn can_find_private_macro_definition() {
        let file =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();
        let doc = TextDoc::try_from(file).expect("Path must be valid.");

        let tree = t32::parse(doc.text.as_bytes(), None);

        let loc = find_definition(
            doc,
            tree,
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

        assert!(matches!(
            loc,
            Some(LocationLink {
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
            })
        ));
    }
}
