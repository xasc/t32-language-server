// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use tree_sitter::Language;
use tree_sitter::Tree;

use crate::ls::TextDoc;
use crate::protocol::Position;
use crate::protocol::Range;
use crate::t32::ast::NodeKind;
use crate::t32::ast::find_deepest_node;
use crate::t32::ast::id_into_node;
use crate::t32::expressions::FunctionCallExpression;
use crate::t32::expressions::extract_function_call;

#[derive(Debug)]
pub enum HoverTarget {
    BuiltinFunction { name: String, full: Range },
}

pub fn validate_hover_target(doc: TextDoc, tree: Tree, position: Position) -> Option<HoverTarget> {
    let offset = doc.to_byte_offset(&position);

    let lang = tree.language();
    let allowed_kinds = get_hover_ids(&lang);

    let cursor = tree.walk();
    let mut origin = find_deepest_node(cursor, offset, &allowed_kinds)?;

    match id_into_node(&lang, origin.node().kind_id()) {
        NodeKind::CallExpression => {
            let FunctionCallExpression { target, call } = extract_function_call(&mut origin)?;
            debug_assert_eq!(
                origin.node().kind_id(),
                NodeKind::CallExpression.into_id(&lang)
            );

            let Some(name) = doc.text.get(target.inner().start..target.inner().end) else {
                return None;
            };

            Some(HoverTarget::BuiltinFunction {
                name: name.to_string(),
                full: doc.to_range(call.inner().start, call.inner().end),
            })
        }
        _ => None,
    }
}

pub fn get_hover_ids(lang: &Language) -> [u16; 1] {
    [NodeKind::CallExpression.into_id(lang)]
}
