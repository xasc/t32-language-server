// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Folding Range Kind Client Capability
//! ===========================================
//!
//! The property `foldingRangeKind` signals what folding range kinds the client
//! supports. `foldingRangeKind` is an optional property. If it is missing,
//! then the client does *not* guarantee that it will handle any folding range
//! kinds, that the server might return, well. Hence, the server cannot safely
//! populate the corresponding field in its response, if the client has not
//! signaled support for any folding range kind.
//! If the property `foldingRangeKind` is present, then folding range kinds
//! that the client does not support will be mapped to a default value.
//!

use crate::{
    ReturnCode,
    config::{CodeFoldingEncoding, CodeFoldingSupport},
    ls::{
        Message, Notification, Response, Task, Tasks, TextDocs,
        doc::TextDocData,
        response::NullResponse,
        tasks::{trace_doc_unknown, try_schedule},
    },
    protocol::{
        FoldingRange, FoldingRangeKind, FoldingRangeParams, LogTraceParams, NumberOrString,
        TraceValue,
    },
    t32::list_code_folds,
};
pub fn process_folding_range_req(
    id: NumberOrString,
    params: FoldingRangeParams,
    trace_level: TraceValue,
    capabilities: CodeFoldingSupport,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_folding_range_req(id.clone())));
    }

    let (doc, tree) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, _)) => (doc, tree),
        None => {
            if trace_level != TraceValue::Off {
                outgoing.push(Some(trace_doc_unknown(&params.text_document.uri)));
            }
            outgoing.push(Some(Message::Response(Response::NullResponse(
                NullResponse { id },
            ))));
            return Ok(());
        }
    };

    try_schedule(
        &mut ts.runner,
        Task::CodeFolds(
            id,
            capabilities.encoding,
            capabilities.folding_range_kinds,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            mark_code_folds,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

fn log_folding_range_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received semantic tokens for full file request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}

fn mark_code_folds(
    encoding: CodeFoldingEncoding,
    fold_kinds: Vec<FoldingRangeKind>,
    textdoc: TextDocData,
) -> Vec<FoldingRange> {
    let mut folds = list_code_folds(&textdoc.doc, &textdoc.tree);
    if !folds.is_empty() {
        folds = strip_unsupported_fold_range_types(&fold_kinds, folds);
        folds = encode_code_folds(&encoding, folds);
    }
    folds
}

fn strip_unsupported_fold_range_types(
    fold_kinds: &[FoldingRangeKind],
    mut folds: Vec<FoldingRange>,
) -> Vec<FoldingRange> {
    let (encodes_comment, encodes_region): (bool, bool) = {
        if fold_kinds.is_empty() {
            (false, false)
        } else {
            (
                fold_kinds.contains(&FoldingRangeKind::Comment),
                fold_kinds.contains(&FoldingRangeKind::Region),
            )
        }
    };

    for fold in folds.iter_mut() {
        if let Some(kind) = &fold.kind {
            if (*kind == FoldingRangeKind::Comment && !encodes_comment)
                || (*kind == FoldingRangeKind::Region && !encodes_region)
            {
                fold.kind = None;
            }
        }
    }
    folds
}

fn encode_code_folds(
    encoding: &CodeFoldingEncoding,
    mut folds: Vec<FoldingRange>,
) -> Vec<FoldingRange> {
    if !encoding.line_folds_only {
        return folds;
    }

    for fold in folds.iter_mut() {
        // If the client only supports folding of complete lines, we must
        // shorten the end line. End character 0 means the complete prior line
        // is selected. However, instead the complete line with the end
        // character is selected and the code fold is too long.
        if let Some(end) = fold.end_character
            && end <= 0
            && fold.end_line > fold.start_line
        {
            fold.end_line -= 1;
        }
        debug_assert!(fold.start_line <= fold.end_line);

        fold.start_character = None;
        fold.end_character = None;
    }
    folds
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path;

    use url::Url;

    use crate::{config, ls, utils};

    #[test]
    fn can_encode_code_folds() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/folds.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let folds = list_code_folds(&doc, &tree);

        assert!(
            folds
                .iter()
                .any(|f| f.start_character.is_some() || f.end_character.is_some())
        );

        let folds = encode_code_folds(
            &CodeFoldingEncoding {
                line_folds_only: true,
                collapsed_text_supported: false,
            },
            folds,
        );

        assert!(
            folds
                .iter()
                .any(|f| f.start_character.is_none() && f.end_character.is_none())
        );
        assert!(folds.iter().any(|f| f.collapsed_text.is_none()));
    }

    #[test]
    fn can_map_folding_range_types() {
        let files = utils::create_file_idx();
        let dirs = config::T32DefaultDirs::default();

        let uri_a = Url::from_file_path(
            path::absolute("tests/samples/folds.cmm").expect("Files must exist."),
        )
        .unwrap();

        let (doc, tree, _) = ls::read_doc(uri_a, &files, &dirs).expect("Must not fail.");

        let folds = list_code_folds(&doc, &tree);

        assert!(folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Comment)
        }));
        assert!(folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Region)
        }));
        assert!(!folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Imports)
        }));

        let folds = strip_unsupported_fold_range_types(&[], folds);

        assert!(!folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Comment)
                || f.kind
                    .as_ref()
                    .is_some_and(|k| *k == FoldingRangeKind::Region)
        }));

        let folds = list_code_folds(&doc, &tree);

        let folds = strip_unsupported_fold_range_types(
            &[FoldingRangeKind::Comment, FoldingRangeKind::Region],
            folds,
        );

        assert!(folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Comment)
                || f.kind
                    .as_ref()
                    .is_some_and(|k| *k == FoldingRangeKind::Region)
        }));

        let folds = list_code_folds(&doc, &tree);

        let folds = strip_unsupported_fold_range_types(&[FoldingRangeKind::Comment], folds);

        assert!(!folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Region)
        }));

        let folds = list_code_folds(&doc, &tree);

        let folds = strip_unsupported_fold_range_types(&[FoldingRangeKind::Region], folds);

        assert!(!folds.iter().any(|f| {
            f.kind
                .as_ref()
                .is_some_and(|k| *k == FoldingRangeKind::Comment)
        }));
    }
}
