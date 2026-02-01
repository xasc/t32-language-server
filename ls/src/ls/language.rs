// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{cmp::Ordering, ops::Range};

use serde::Serialize;

use tree_sitter::{Node, Tree, TreeCursor};

use crate::{
    ls::doc::{GlobalMacroDefIndex, TextDoc, TextDocData, TextDocs},
    protocol::{Location, LocationLink, Position, Range as LRange, Uri},
    t32::{
        FindMacroRefsLangContext, FindRefsLangContext, GotoDefLangContext, MacroDefinition,
        MacroDefinitionResult, MacroScope, NodeKind, Subroutine, find_command_file_target,
        find_label_references, find_macro_definition_references, find_stack_macro_references,
        find_subroutine_call_references, find_subroutine_references, get_find_ref_ids,
        get_goto_def_ids, get_macro_scope, goto_external_macro_definition, goto_file,
        goto_infile_macro_definition, goto_subroutine_definition, id_into_node,
        locate_calls_to_file_target,
    },
    utils::BRange,
};

#[derive(Debug, PartialEq)]
pub enum FindDefintionsForMacroRefResult {
    Final(Vec<MacroDefinitionLocation>, Uri),
    Partial(Vec<MacroDefinitionLocation>, Uri, Vec<Uri>),
}

#[derive(Debug, PartialEq)]
pub enum FindReferencesResult {
    Final(Vec<Location>),
    Partial(FindReferencesPartialResult),
}

#[derive(Debug, PartialEq)]
pub enum FindReferencesPartialResult {
    MacroDefsComplete {
        origin: MacroReferenceOrigin,
        definitions: Vec<(FileLocation, Option<MacroScope>)>,
    },
    MacroDefsIncomplete {
        origin: MacroReferenceOrigin,
        definitions: Vec<(FileLocation, Option<MacroScope>)>,
    },
    FileTarget(String),
}

#[derive(Debug, PartialEq)]
pub enum GotoDefinitionResult {
    Final(Vec<LocationLink>),
    PartialMacro(Uri, String, LRange, Vec<LocationLink>),
}

#[derive(Clone, Debug)]
pub enum MacroDefinitionLocation {
    Private(BRange),
    Local(BRange),
    Global(BRange),
}

#[derive(Debug)]
pub struct FindMacroReferencesResult {
    pub uri: Uri,
    pub references: Vec<LRange>,
    pub subscripts: Vec<Uri>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MacroReferenceOrigin {
    pub name: String,
    pub span: LRange,
    pub uri: Uri,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileLocation {
    pub uri: Uri,
    pub range: BRange,
}

impl FindMacroReferencesResult {
    pub fn build(uri: Uri, locations: Vec<LRange>, subscripts: Vec<Uri>) -> Self {
        FindMacroReferencesResult {
            uri,
            references: locations,
            subscripts,
        }
    }
}

impl MacroDefinitionLocation {
    pub fn split(self) -> (MacroScope, BRange) {
        match self {
            Self::Private(span) => (MacroScope::Private, span),
            Self::Local(span) => (MacroScope::Local, span),
            Self::Global(span) => (MacroScope::Global, span),
        }
    }
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

impl Eq for MacroDefinitionLocation {}

impl Ord for MacroDefinitionLocation {
    fn cmp(&self, other: &Self) -> Ordering {
        match self {
            MacroDefinitionLocation::Private(span)
            | MacroDefinitionLocation::Local(span)
            | MacroDefinitionLocation::Global(span) => match other {
                MacroDefinitionLocation::Private(other_span)
                | MacroDefinitionLocation::Local(other_span)
                | MacroDefinitionLocation::Global(other_span) => {
                    let cmp = span.cmp(other_span);
                    if cmp != Ordering::Equal {
                        return cmp;
                    }
                }
            },
        }
        match self {
            MacroDefinitionLocation::Private(_) => match other {
                MacroDefinitionLocation::Private(_) => Ordering::Equal,
                MacroDefinitionLocation::Local(_) | MacroDefinitionLocation::Global(_) => {
                    Ordering::Less
                }
            },
            MacroDefinitionLocation::Local(_) => match other {
                MacroDefinitionLocation::Private(_) => Ordering::Greater,
                MacroDefinitionLocation::Local(_) => Ordering::Equal,
                MacroDefinitionLocation::Global(_) => Ordering::Less,
            },
            MacroDefinitionLocation::Global(_) => match other {
                MacroDefinitionLocation::Private(_) | MacroDefinitionLocation::Local(_) => {
                    Ordering::Greater
                }
                MacroDefinitionLocation::Global(_) => Ordering::Equal,
            },
        }
    }
}

impl PartialEq for MacroDefinitionLocation {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialOrd for MacroDefinitionLocation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl MacroDefinitionLocation {
    pub fn from_macro_def(scope: MacroScope, definition: MacroDefinition) -> Self {
        match scope {
            MacroScope::Private => Self::Private(BRange::from(definition.r#macro)),
            MacroScope::Local => Self::Local(BRange::from(definition.r#macro)),
            MacroScope::Global => Self::Global(BRange::from(definition.r#macro)),
        }
    }

    pub fn from_span(scope: Option<&MacroScope>, span: BRange) -> Self {
        match scope {
            Some(MacroScope::Private) => Self::Private(span),
            // Assume block-global, if no other scope is provided.
            Some(MacroScope::Local) | None => Self::Local(span),
            Some(MacroScope::Global) => Self::Global(span),
        }
    }
}

/// Retrieves definitions for `(macro)`, `(subroutine_call_expression)`, and
/// `(command_expression)` nodes. `(command_expression)` nodes capture
/// `DO` and `RUN` commands for subscripts calls.
///   - Macros may have multiple definitions in other files due to the
///     `LOCAL` keyword. `GLOBAL` macro definitions are ignored.
///   - Subscript calls return the start of the script file.
///   - Subroutine definitions are limited to the current file.
///
pub fn find_definition(
    textdoc: TextDocData,
    t32: GotoDefLangContext,
    position: Position,
) -> Option<GotoDefinitionResult> {
    let offset = textdoc.doc.to_byte_offset(&position);

    let lang = textdoc.tree.language();
    let allowed_kinds = get_goto_def_ids(&lang);

    let origin = find_deepest_node(&textdoc.tree, offset, &allowed_kinds)?;

    let node = origin.node();
    let origin_span = node.range();

    match id_into_node(&lang, origin.node().kind_id()) {
        NodeKind::Macro => {
            match goto_infile_macro_definition(&textdoc.doc.text, &textdoc.tree, &t32, origin)? {
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
                MacroDefinitionResult::Partial(gotos) => {
                    let mut links: Vec<LocationLink> = Vec::with_capacity(gotos.len());
                    for def in gotos {
                        links.push(LocationLink::from_macro_def(
                            &textdoc.doc,
                            origin_span.into(),
                            def,
                        ));
                    }
                    let name = textdoc.doc.text[node.byte_range()].to_string();
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
                MacroDefinitionResult::Indeterminate => {
                    let name = textdoc.doc.text[node.byte_range()].to_string();
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
            let uri = goto_file(&textdoc.doc.text, &t32.calls.scripts?, origin)?;

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
        // TODO: `GOSUB &macro` must look for macro definition instead of subroutine.
        NodeKind::SubroutineCallExpression => {
            let sub: Subroutine =
                goto_subroutine_definition(&textdoc.doc.text, &t32.subroutines, origin)?;
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
        _ => None,
    }
}

pub fn find_external_macro_definition(
    textdoc: TextDocData,
    t32: GotoDefLangContext,
    callers: Vec<Uri>,
    origin: MacroReferenceOrigin,
    backtrace: Uri,
) -> (Option<GotoDefinitionResult>, Vec<Uri>) {
    let Some(subscripts) = &t32.calls.scripts else {
        return (None, callers);
    };

    let MacroReferenceOrigin {
        name: r#macro,
        span,
        ..
    } = origin;

    let targets: Vec<BRange> = locate_calls_to_file_target(subscripts, &backtrace);
    if targets.is_empty() {
        return (None, callers);
    }

    // TODO: `RUN` clears the PRACTICE stack, so it cannot propagate
    // `LOCAL` macros.
    // TODO: Add support for `GOTO` → `(label)` transitions.
    match goto_external_macro_definition(&textdoc.doc.text, &textdoc.tree, &t32, &r#macro, targets)
    {
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
        Some(MacroDefinitionResult::Partial(gotos)) => {
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
                    r#macro,
                    span.clone(),
                    links,
                )),
                callers,
            )
        }
        Some(MacroDefinitionResult::Indeterminate) => (
            Some(GotoDefinitionResult::PartialMacro(
                textdoc.doc.uri,
                r#macro,
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
    origin: MacroReferenceOrigin,
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

/// Retrieves references for `(macro)`, `(subroutine_call_expression)`,
/// `(labeled_expression)`, `(subroutine_block)`, and `(command_expression)`
/// nodes.
///    - Macro references may be located in other files if `LOCAL` was used
///      to define the macro. `GLOBAL` macros are handled separately.
///    - Subroutine references are restricted to the current file.
///    - Subscript calls should return all similar calls in other script files.
///      Similarly, for all other commands the instances in other files should
///      be included. Both are not covered here.
///
pub fn find_references(
    textdoc: TextDocData,
    t32: FindRefsLangContext,
    position: Position,
) -> Option<FindReferencesResult> {
    let offset = textdoc.doc.to_byte_offset(&position);

    let lang = textdoc.tree.language();
    let allowed_kinds = get_find_ref_ids(&lang);

    let origin = find_deepest_node(&textdoc.tree, offset, &allowed_kinds)?;
    let node = origin.node();

    match id_into_node(&lang, node.kind_id()) {
        NodeKind::CommandExpression => {
            // Selecting one of the command arguments in a `DO` or `RUN`
            // command is sufficient to trigger retrieval of all references
            // to the same file. However, if the command is selected, then
            // all references with the same command will be returned.
            if command_argument_selected(origin.clone(), offset)
                && let Some(target) = find_command_file_target(&textdoc.doc.text, origin)
            {
                Some(FindReferencesResult::Partial(
                    FindReferencesPartialResult::FileTarget(target),
                ))
            } else {
                todo!()
            }
        }
        NodeKind::LabeledExpression => {
            if !position_aligned_with_node_start(&position, &textdoc.doc, &node) {
                return None;
            }

            if t32.subroutines.is_empty() {
                return None;
            }

            let mut loc: Vec<Location> = Vec::new();

            let refs = if let Some(refs) = find_subroutine_references(
                &textdoc.doc.text,
                &t32.subroutines,
                &origin,
                &textdoc.tree,
            ) {
                Some(refs)
            } else {
                find_label_references(&textdoc.doc.text, &t32.labels, &origin, &textdoc.tree)
            };

            for r#ref in refs? {
                loc.push(Location {
                    uri: textdoc.doc.uri.clone(),
                    range: textdoc.doc.to_range(r#ref.start, r#ref.end),
                });
            }
            Some(FindReferencesResult::Final(loc))
        }
        NodeKind::Macro => {
            let t32 = GotoDefLangContext::from(t32);
            match goto_infile_macro_definition(&textdoc.doc.text, &textdoc.tree, &t32, origin)? {
                MacroDefinitionResult::Final(defs) => {
                    debug_assert!(defs.len() > 0);
                    let name = textdoc.doc.text[defs[0].r#macro.clone()].to_string();

                    let mut origins: Vec<(FileLocation, Option<MacroScope>)> = Vec::new();
                    for def in defs {
                        let scope = get_macro_scope(&t32.macros, &def.r#macro);
                        origins.push((
                            FileLocation {
                                uri: textdoc.doc.uri.clone(),
                                range: BRange::from(def.r#macro),
                            },
                            scope,
                        ));
                    }

                    Some(FindReferencesResult::Partial(
                        FindReferencesPartialResult::MacroDefsComplete {
                            origin: MacroReferenceOrigin {
                                name,
                                span: textdoc.doc.to_range(node.start_byte(), node.end_byte()),
                                uri: textdoc.doc.uri,
                            },
                            definitions: origins,
                        },
                    ))
                }
                MacroDefinitionResult::Partial(defs) => {
                    let mut origins: Vec<(FileLocation, Option<MacroScope>)> = Vec::new();
                    for def in defs {
                        let scope = get_macro_scope(&t32.macros, &def.r#macro);
                        origins.push((
                            FileLocation {
                                uri: textdoc.doc.uri.clone(),
                                range: BRange::from(def.r#macro),
                            },
                            scope,
                        ));
                    }

                    Some(FindReferencesResult::Partial(
                        FindReferencesPartialResult::MacroDefsIncomplete {
                            origin: MacroReferenceOrigin {
                                name: textdoc.doc.text[node.byte_range()].to_string(),
                                span: textdoc.doc.to_range(node.start_byte(), node.end_byte()),
                                uri: textdoc.doc.uri,
                            },
                            definitions: origins,
                        },
                    ))
                }
                MacroDefinitionResult::Indeterminate => Some(FindReferencesResult::Partial(
                    FindReferencesPartialResult::MacroDefsIncomplete {
                        origin: MacroReferenceOrigin {
                            name: textdoc.doc.text[node.byte_range()].to_string(),
                            span: textdoc.doc.to_range(node.start_byte(), node.end_byte()),
                            uri: textdoc.doc.uri,
                        },
                        definitions: Vec::new(),
                    },
                )),
            }
        }
        NodeKind::SubroutineCallExpression => {
            if t32.subroutines.is_empty() {
                return None;
            }

            let mut loc: Vec<Location> = Vec::new();
            for r#ref in find_subroutine_call_references(
                &textdoc.doc.text,
                &t32.subroutines,
                origin,
                &textdoc.tree,
            )? {
                loc.push(Location {
                    uri: textdoc.doc.uri.clone(),
                    range: textdoc.doc.to_range(r#ref.start, r#ref.end),
                });
            }
            Some(FindReferencesResult::Final(loc))
        }
        NodeKind::SubroutineBlock => {
            if !position_aligned_with_node_start(&position, &textdoc.doc, &node) {
                return None;
            }

            if t32.subroutines.is_empty() {
                return None;
            }

            let mut loc: Vec<Location> = Vec::new();
            for r#ref in find_subroutine_references(
                &textdoc.doc.text,
                &t32.subroutines,
                &origin,
                &textdoc.tree,
            )? {
                loc.push(Location {
                    uri: textdoc.doc.uri.clone(),
                    range: textdoc.doc.to_range(r#ref.start, r#ref.end),
                });
            }
            Some(FindReferencesResult::Final(loc))
        }
        _ => None,
    }
}

pub fn find_external_definitions_for_macro_ref(
    textdoc: TextDocData,
    t32: GotoDefLangContext,
    callers: Vec<Uri>,
    origin: MacroReferenceOrigin,
    backtrace: Uri,
) -> FindDefintionsForMacroRefResult {
    let Some(subscripts) = &t32.calls.scripts else {
        return FindDefintionsForMacroRefResult::Final(Vec::new(), textdoc.doc.uri);
    };

    let MacroReferenceOrigin { name: r#macro, .. } = origin;

    let targets: Vec<BRange> = locate_calls_to_file_target(subscripts, &backtrace);
    if targets.is_empty() {
        return FindDefintionsForMacroRefResult::Final(Vec::new(), textdoc.doc.uri);
    }

    match goto_external_macro_definition(&textdoc.doc.text, &textdoc.tree, &t32, &r#macro, targets)
    {
        Some(MacroDefinitionResult::Final(defs)) => {
            let locations: Vec<MacroDefinitionLocation> = {
                let mut locs: Vec<MacroDefinitionLocation> = Vec::with_capacity(defs.len());
                for def in defs {
                    let Some(scope) = get_macro_scope(&t32.macros, &def.r#macro) else {
                        unreachable!(
                            "Retrieved macro definition must already have been registered."
                        );
                    };
                    locs.push(MacroDefinitionLocation::from_macro_def(scope, def));
                }
                locs
            };
            FindDefintionsForMacroRefResult::Final(locations, textdoc.doc.uri)
        }
        Some(MacroDefinitionResult::Partial(defs)) => {
            let locations: Vec<MacroDefinitionLocation> = {
                let mut locs: Vec<MacroDefinitionLocation> = Vec::with_capacity(defs.len());
                for def in defs {
                    let Some(scope) = get_macro_scope(&t32.macros, &def.r#macro) else {
                        unreachable!(
                            "Retrieved macro definition must already have been registered."
                        );
                    };
                    locs.push(MacroDefinitionLocation::from_macro_def(scope, def));
                }
                locs
            };
            FindDefintionsForMacroRefResult::Partial(locations, textdoc.doc.uri, callers)
        }
        Some(MacroDefinitionResult::Indeterminate) => {
            if callers.is_empty() {
                FindDefintionsForMacroRefResult::Final(Vec::new(), textdoc.doc.uri)
            } else {
                FindDefintionsForMacroRefResult::Partial(Vec::new(), textdoc.doc.uri, callers)
            }
        }
        None => FindDefintionsForMacroRefResult::Final(Vec::new(), textdoc.doc.uri),
    }
}

pub fn find_macro_references_from_origin(
    textdoc: TextDocData,
    t32: FindMacroRefsLangContext,
    name: String,
    origins: Vec<MacroDefinitionLocation>,
) -> FindMacroReferencesResult {
    let mut locs: Vec<LRange> = Vec::new();
    let mut callees: Vec<Uri> = Vec::new();

    for origin in origins {
        let (lifetime, loc) = origin.split();

        let (spans, mut scripts) = find_macro_definition_references(
            &textdoc.doc.text,
            &textdoc.tree,
            &t32,
            &name,
            lifetime,
            loc,
        );

        locs.reserve(spans.len());
        for span in spans {
            let inner = span.to_inner();
            locs.push(textdoc.doc.to_range(inner.start, inner.end));
        }
        scripts.retain(|s| !callees.contains(s));
        callees.append(&mut scripts);
    }
    FindMacroReferencesResult::build(textdoc.doc.uri, locs, callees)
}

pub fn find_infile_macro_references(
    textdoc: TextDocData,
    t32: FindMacroRefsLangContext,
    name: String,
) -> FindMacroReferencesResult {
    let doc = &textdoc.doc;

    // It does not matter here what exact scope the respective macro has.
    // Either `GLOBAL` or `LOCAL` will do.
    let (spans, scripts) =
        find_stack_macro_references(&doc.text, &textdoc.tree, &t32, MacroScope::Local, &name);

    let mut locs: Vec<LRange> = Vec::with_capacity(spans.len());
    for span in spans {
        let inner = span.to_inner();
        locs.push(textdoc.doc.to_range(inner.start, inner.end));
    }
    FindMacroReferencesResult::build(textdoc.doc.uri, locs, scripts)
}

pub fn command_argument_selected(mut cursor: TreeCursor, offset: usize) -> bool {
    debug_assert_eq!(
        cursor.node().kind_id(),
        NodeKind::CommandExpression.into_id(&cursor.node().language())
    );

    if cursor.goto_first_child_for_byte(offset).is_none() {
        return false;
    }

    let node = cursor.node();
    let lang = node.language();

    // Selection is on command
    if node.kind_id() != NodeKind::ArgumentList.into_id(&lang) {
        return false;
    }

    if cursor.goto_first_child_for_byte(offset).is_none() {
        return false;
    }

    if cursor.node().byte_range().contains(&offset) {
        true
    } else {
        false
    }
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
        if let Some(_) = stop_at.iter().find(|&&k| k == id) {
            sel = Some(cursor.clone());
        }
    }
    sel
}

fn position_aligned_with_node_start(position: &Position, doc: &TextDoc, node: &Node) -> bool {
    let start = doc.to_position(node.start_byte());
    start.line == position.line
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{env, path};
    use url::Url;

    use crate::{
        ls::{
            doc::read_doc,
            workspace::{self, FileIndex},
        },
        protocol::Range as LRange,
        t32::LangExpressions,
        utils::{files, to_file_uri},
    };

    fn create_file_idx() -> FileIndex {
        workspace::index_files(files())
    }

    fn find_def(file: &str, position: Position) -> Option<GotoDefinitionResult> {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();

        let file_idx = workspace::index_files(files());
        let (doc, tree, t32) = read_doc(uri, file_idx).unwrap();

        find_definition(
            TextDocData { doc, tree },
            GotoDefLangContext::from(t32),
            position,
        )
    }

    fn find_refs(file: &str, position: Position) -> Option<FindReferencesResult> {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();

        let file_idx = workspace::index_files(files());
        let (doc, tree, t32) = read_doc(uri, file_idx).unwrap();

        find_references(
            TextDocData { doc, tree },
            FindRefsLangContext::from(t32),
            position,
        )
    }

    fn find_external_macro_def(
        file: &str,
        callers: Vec<Uri>,
        origin: MacroReferenceOrigin,
        backtrace: Uri,
    ) -> (Option<GotoDefinitionResult>, Vec<Uri>) {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();

        let file_idx = workspace::index_files(files());
        let (doc, tree, t32) = read_doc(uri, file_idx).unwrap();

        find_external_macro_definition(
            TextDocData { doc, tree },
            GotoDefLangContext::from(t32),
            callers,
            origin,
            backtrace,
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
            panic!();
        };

        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 6,
                        character: 0,
                    },
                    end: Position {
                        line: 7,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
            panic!();
        };
        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 15,
                        character: 0,
                    },
                    end: Position {
                        line: 19,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
            panic!();
        };
        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 136,
                        character: 0,
                    },
                    end: Position {
                        line: 137,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
            panic!();
        };
        assert!(matches!(
            &loc[..],
            [LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 28,
                        character: 4,
                    },
                    end: Position {
                        line: 29,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
        let uri: Url = Url::from_file_path(path::absolute(file).unwrap()).unwrap();

        let loc = loc.expect("Must not be empty.");

        assert_eq!(
            loc,
            GotoDefinitionResult::PartialMacro(
                uri.to_string(),
                "&local_macro".to_string(),
                LRange {
                    start: Position {
                        line: 38,
                        character: 4,
                    },
                    end: Position {
                        line: 38,
                        character: 16,
                    },
                },
                vec![
                    LocationLink {
                        origin_selection_range: Some(LRange {
                            start: Position {
                                line: 38,
                                character: 4,
                            },
                            end: Position {
                                line: 38,
                                character: 16,
                            },
                        }),
                        target_uri: uri.to_string(),
                        target_range: LRange {
                            start: Position {
                                line: 42,
                                character: 0,
                            },
                            end: Position {
                                line: 43,
                                character: 0,
                            },
                        },
                        target_selection_range: LRange {
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
                        origin_selection_range: Some(LRange {
                            start: Position {
                                line: 38,
                                character: 4,
                            },
                            end: Position {
                                line: 38,
                                character: 16,
                            },
                        }),
                        target_uri: uri.to_string(),
                        target_range: LRange {
                            start: Position {
                                line: 38,
                                character: 4,
                            },
                            end: Position {
                                line: 39,
                                character: 0,
                            },
                        },
                        target_selection_range: LRange {
                            start: Position {
                                line: 38,
                                character: 4,
                            },
                            end: Position {
                                line: 38,
                                character: 16,
                            },
                        },
                    },
                ]
            )
        );
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
                    origin_selection_range: Some(LRange {
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
                    target_range: LRange {
                        start: Position {
                            line: 65,
                            character: 4,
                        },
                        end: Position {
                            line: 66,
                            character: 0,
                        },
                    },
                    target_selection_range: LRange {
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
                    origin_selection_range: Some(LRange {
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
                    target_range: LRange {
                        start: Position {
                            line: 72,
                            character: 4,
                        },
                        end: Position {
                            line: 73,
                            character: 0,
                        },
                    },
                    target_selection_range: LRange {
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
                    origin_selection_range: Some(LRange {
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
                    target_range: LRange {
                        start: Position {
                            line: 61,
                            character: 0,
                        },
                        end: Position {
                            line: 62,
                            character: 0,
                        },
                    },
                    target_selection_range: LRange {
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
                    origin_selection_range: Some(LRange {
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
                    target_range: LRange {
                        start: Position {
                            line: 73,
                            character: 4,
                        },
                        end: Position {
                            line: 74,
                            character: 0,
                        },
                    },
                    target_selection_range: LRange {
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

        for (loc, link) in [
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
            Position {
                line: 170,
                character: 7,
            },
        ]
        .into_iter()
        .zip([
            LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 98,
                        character: 4,
                    },
                    end: Position {
                        line: 99,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 82,
                        character: 4,
                    },
                    end: Position {
                        line: 83,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                origin_selection_range: Some(LRange {
                    start: Position {
                        line: 170,
                        character: 4,
                    },
                    end: Position {
                        line: 170,
                        character: 13,
                    },
                }),
                target_uri: uri.to_string(),
                target_range: LRange {
                    start: Position {
                        line: 164,
                        character: 0,
                    },
                    end: Position {
                        line: 165,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
                    start: Position {
                        line: 164,
                        character: 0,
                    },
                    end: Position {
                        line: 164,
                        character: 9,
                    },
                },
            },
        ]) {
            let result = find_def("tests/samples/a/a.cmm", loc);
            assert!(result.is_some_and(|r| match r {
                GotoDefinitionResult::Final(links)
                | GotoDefinitionResult::PartialMacro(_, _, _, links) => links.contains(&link),
            }));
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
            panic!();
        };
        assert!(matches!(
            &loc[0],
            LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
                target_selection_range: LRange {
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

        for ((file, parents, origin, backtrace), _link) in [
            (
                "tests/samples/a/a.cmm",
                callers.clone(),
                MacroReferenceOrigin {
                    name: "&local_macro".to_string(),
                    span: LRange {
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
                Url::from_file_path(
                    path::absolute("tests/samples/c.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
            ),
            (
                "tests/samples/c.cmm",
                callers.clone(),
                MacroReferenceOrigin {
                    name: "&from_c_cmm".to_string(),
                    span: LRange {
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
                Url::from_file_path(
                    path::absolute("tests/samples/a/a.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
            ),
        ]
        .into_iter()
        .zip([
            LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 42,
                        character: 0,
                    },
                    end: Position {
                        line: 43,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 22,
                        character: 4,
                    },
                    end: Position {
                        line: 23,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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
                find_external_macro_def(file, parents, origin, backtrace)
            else {
                panic!();
            };

            assert!(matches!(&loc[..], _link));
            assert_eq!(successors, callers);
        }
    }

    #[test]
    fn can_find_macro_global_macro_definition() {
        let file_idx = create_file_idx();

        let files = files();
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
            MacroReferenceOrigin {
                name: "&global_macro".to_string(),
                span: LRange {
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

        let _uri = to_file_uri("tests/samples/a/a.cmm");

        assert!(matches!(
            &links[..],
            [LocationLink {
                origin_selection_range: Some(LRange {
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
                target_range: LRange {
                    start: Position {
                        line: 41,
                        character: 0,
                    },
                    end: Position {
                        line: 42,
                        character: 0,
                    },
                },
                target_selection_range: LRange {
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

    #[test]
    fn can_find_references_for_subroutine_call() {
        let refs = find_refs(
            "tests/samples/a/a.cmm",
            Position {
                line: 67,
                character: 10,
            },
        );

        let Some(FindReferencesResult::Final(refs)) = refs else {
            panic!();
        };

        let uri = to_file_uri("tests/samples/a/a.cmm");

        for (loc, expected) in refs.into_iter().zip([
            Location {
                uri: uri.clone(),
                range: LRange {
                    start: Position {
                        line: 54,
                        character: 11,
                    },
                    end: Position {
                        line: 54,
                        character: 15,
                    },
                },
            },
            Location {
                uri: uri.clone(),
                range: LRange {
                    start: Position {
                        line: 67,
                        character: 10,
                    },
                    end: Position {
                        line: 67,
                        character: 14,
                    },
                },
            },
            Location {
                uri: uri.clone(),
                range: LRange {
                    start: Position {
                        line: 75,
                        character: 10,
                    },
                    end: Position {
                        line: 75,
                        character: 14,
                    },
                },
            },
        ]) {
            assert_eq!(loc, expected);
        }
    }

    #[test]
    fn can_find_references_for_subroutine_defintion() {
        let uri = to_file_uri("tests/samples/a/a.cmm");

        for (loc, expected) in [
            Position {
                line: 113,
                character: 11,
            },
            Position {
                line: 80,
                character: 0,
            },
        ]
        .into_iter()
        .zip([
            [
                Location {
                    uri: uri.clone(),
                    range: LRange {
                        start: Position {
                            line: 113,
                            character: 11,
                        },
                        end: Position {
                            line: 113,
                            character: 15,
                        },
                    },
                },
                Location {
                    uri: uri.clone(),
                    range: LRange {
                        start: Position {
                            line: 110,
                            character: 10,
                        },
                        end: Position {
                            line: 110,
                            character: 14,
                        },
                    },
                },
            ],
            [
                Location {
                    uri: uri.clone(),
                    range: LRange {
                        start: Position {
                            line: 80,
                            character: 0,
                        },
                        end: Position {
                            line: 80,
                            character: 4,
                        },
                    },
                },
                Location {
                    uri: uri.clone(),
                    range: LRange {
                        start: Position {
                            line: 118,
                            character: 6,
                        },
                        end: Position {
                            line: 118,
                            character: 10,
                        },
                    },
                },
            ],
        ]) {
            let refs = find_refs("tests/samples/a/a.cmm", loc);

            let Some(FindReferencesResult::Final(refs)) = refs else {
                panic!();
            };

            for (loc, expected) in refs.into_iter().zip(expected) {
                assert_eq!(loc, expected);
            }
        }
    }

    #[test]
    fn can_find_references_for_label() {
        let refs = find_refs(
            "tests/samples/a/a.cmm",
            Position {
                line: 157,
                character: 3,
            },
        );

        let Some(FindReferencesResult::Final(refs)) = refs else {
            panic!();
        };

        let uri = to_file_uri("tests/samples/a/a.cmm");
        for (loc, expected) in refs.into_iter().zip([
            Location {
                uri: uri.clone(),
                range: LRange {
                    start: Position {
                        line: 157,
                        character: 0,
                    },
                    end: Position {
                        line: 157,
                        character: 6,
                    },
                },
            },
            Location {
                uri: uri.clone(),
                range: LRange {
                    start: Position {
                        line: 160,
                        character: 5,
                    },
                    end: Position {
                        line: 160,
                        character: 11,
                    },
                },
            },
        ]) {
            assert_eq!(loc, expected);
        }
    }

    #[test]
    fn can_find_infile_references_for_macro_definition() {
        let file_idx = create_file_idx();

        let uri =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();
        let (doc, tree, t32) = read_doc(uri, file_idx).unwrap();

        for ((name, range, scope), (refs, scripts)) in [
            (
                "&private_macro",
                Range {
                    start: 134usize,
                    end: 148usize,
                },
                MacroScope::Private,
            ),
            (
                "&local_macro",
                Range {
                    start: 509usize,
                    end: 521usize,
                },
                MacroScope::Local,
            ),
            (
                "&implicit",
                Range {
                    start: 1586usize,
                    end: 1595usize,
                },
                MacroScope::Local,
            ),
            (
                "&x",
                Range {
                    start: 919usize,
                    end: 921usize,
                },
                MacroScope::Local,
            ),
        ]
        .into_iter()
        .zip(
            [
                (
                    vec![
                        LRange {
                            start: Position {
                                line: 6,
                                character: 8,
                            },
                            end: Position {
                                line: 6,
                                character: 22,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 8,
                                character: 0,
                            },
                            end: Position {
                                line: 8,
                                character: 14,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 11,
                                character: 3,
                            },
                            end: Position {
                                line: 11,
                                character: 17,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 164,
                                character: 10,
                            },
                            end: Position {
                                line: 164,
                                character: 24,
                            },
                        },
                    ],
                    Vec::<Uri>::new(),
                ),
                (
                    vec![
                        LRange {
                            start: Position {
                                line: 38,
                                character: 4,
                            },
                            end: Position {
                                line: 38,
                                character: 16,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 42,
                                character: 6,
                            },
                            end: Position {
                                line: 42,
                                character: 18,
                            },
                        },
                    ],
                    vec![
                        to_file_uri("tests/samples/b/b.cmm"),
                        to_file_uri("tests/samples/c.cmm"),
                    ],
                ),
                (
                    vec![
                        LRange {
                            start: Position {
                                line: 164,
                                character: 0,
                            },
                            end: Position {
                                line: 164,
                                character: 9,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 170,
                                character: 4,
                            },
                            end: Position {
                                line: 170,
                                character: 13,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 170,
                                character: 14,
                            },
                            end: Position {
                                line: 170,
                                character: 23,
                            },
                        },
                    ],
                    Vec::new(),
                ),
                (
                    vec![
                        LRange {
                            start: Position {
                                line: 82,
                                character: 10,
                            },
                            end: Position {
                                line: 82,
                                character: 12,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 84,
                                character: 11,
                            },
                            end: Position {
                                line: 84,
                                character: 13,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 91,
                                character: 11,
                            },
                            end: Position {
                                line: 91,
                                character: 13,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 98,
                                character: 15,
                            },
                            end: Position {
                                line: 98,
                                character: 17,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 100,
                                character: 11,
                            },
                            end: Position {
                                line: 100,
                                character: 13,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 107,
                                character: 11,
                            },
                            end: Position {
                                line: 107,
                                character: 13,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 109,
                                character: 10,
                            },
                            end: Position {
                                line: 109,
                                character: 12,
                            },
                        },
                        LRange {
                            start: Position {
                                line: 115,
                                character: 11,
                            },
                            end: Position {
                                line: 115,
                                character: 13,
                            },
                        },
                    ],
                    Vec::new(),
                ),
            ]
            .into_iter(),
        ) {
            let result = find_macro_references_from_origin(
                TextDocData {
                    doc: doc.clone(),
                    tree: tree.clone(),
                },
                FindMacroRefsLangContext::from(t32.clone()),
                name.to_string(),
                vec![MacroDefinitionLocation::from_span(
                    Some(&scope),
                    BRange::from(range),
                )],
            );

            assert_eq!(result.references.len(), refs.len());
            for r#ref in result.references {
                assert!(refs.contains(&r#ref));
            }

            assert_eq!(result.subscripts.len(), scripts.len());
            for file in result.subscripts {
                assert!(scripts.contains(&file));
            }
        }
    }

    #[test]
    fn can_find_references_for_macros_on_stack() {
        let file_idx = create_file_idx();

        let uri =
            Url::from_file_path(path::absolute("tests/samples/a/a.cmm").expect("File must exist."))
                .unwrap();
        let (doc, tree, t32) = read_doc(uri, file_idx).unwrap();

        let result = find_infile_macro_references(
            TextDocData { doc, tree },
            FindMacroRefsLangContext::from(t32),
            "&from_c_cmm".to_string(),
        );

        assert!(result.references.contains(&LRange {
            start: Position {
                line: 139,
                character: 7
            },
            end: Position {
                line: 139,
                character: 18
            },
        }));

        for target in [
            to_file_uri("tests/samples/b/b.cmm"),
            to_file_uri("tests/samples/c.cmm"),
        ] {
            assert!(result.subscripts.contains(&target));
        }
    }

    #[test]
    fn can_find_external_definitions_for_macro_ref() {
        let origin = MacroReferenceOrigin {
            name: "&from_c_cmm".to_string(),
            span: LRange {
                start: Position {
                    line: 162,
                    character: 7,
                },
                end: Position {
                    line: 162,
                    character: 18,
                },
            },
            uri: Url::from_file_path(
                path::absolute("tests/samples/a/a.cmm").expect("File must exist."),
            )
            .unwrap()
            .to_string(),
        };

        for ((uri, origin, callers, backtrace), result) in [
            (
                Url::from_file_path(
                    path::absolute("tests/samples/a/d/d.cmm").expect("File must exist."),
                )
                .unwrap(),
                origin.clone(),
                vec![
                    Url::from_file_path(
                        path::absolute("tests/samples/c.cmm").expect("File must exist."),
                    )
                    .unwrap()
                    .to_string(),
                ],
                Url::from_file_path(
                    path::absolute("tests/samples/a/a.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
            ),
            (
                Url::from_file_path(
                    path::absolute("tests/samples/c.cmm").expect("File must exist."),
                )
                .unwrap(),
                origin.clone(),
                vec![
                    Url::from_file_path(
                        path::absolute("tests/samples/c.cmm").expect("File must exist."),
                    )
                    .unwrap()
                    .to_string(),
                ],
                Url::from_file_path(
                    path::absolute("tests/samples/a/d/d.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
            ),
        ]
        .into_iter()
        .zip([
            FindDefintionsForMacroRefResult::Partial(
                Vec::new(),
                Url::from_file_path(
                    path::absolute("tests/samples/a/d/d.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
                vec![
                    Url::from_file_path(
                        path::absolute("tests/samples/c.cmm").expect("File must exist."),
                    )
                    .unwrap()
                    .to_string(),
                ],
            ),
            FindDefintionsForMacroRefResult::Final(
                vec![MacroDefinitionLocation::Local(
                    Range::<usize> {
                        start: 497,
                        end: 508,
                    }
                    .into(),
                )],
                Url::from_file_path(
                    path::absolute("tests/samples/c.cmm").expect("File must exist."),
                )
                .unwrap()
                .to_string(),
            ),
        ]) {
            let file_idx = create_file_idx();

            let (doc, tree, t32) = read_doc(uri, file_idx).unwrap();

            let defs = find_external_definitions_for_macro_ref(
                TextDocData { doc, tree },
                t32.into(),
                callers,
                origin,
                backtrace,
            );
            assert_eq!(defs, result);
        }
    }

    #[test]
    fn can_return_subscript_call_file_target_for_find_reference_req() {
        let refs = find_refs(
            "tests/samples/a/a.cmm",
            Position {
                line: 49,
                character: 6,
            },
        );
        assert!(refs.is_some_and(|r| r
            == FindReferencesResult::Partial(FindReferencesPartialResult::FileTarget(
                "../b/b.cmm".to_string()
            ))));
    }
}
