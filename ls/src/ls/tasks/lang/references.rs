// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2
//

//! [Note] Workflow Macro Reference Retrieval
//! =========================================
//!
//! The prerequisite for finding all locations where a macro is referenced is
//! the detection of all corresponding macro definitions. Once the definitions
//! are known, we can determine all references in both the same file and called
//! scripts.
//!

use crate::{
    ls::{
        ReturnCode,
        doc::{TextDocData, TextDocs},
        language::{
            FileLocation, FindDefintionsForMacroRefResult, FindMacroReferencesResult,
            FindReferencesPartialResult, FindReferencesResult, MacroDefinitionLocation,
            MacroPropagation, MacroPropagationCompact, MacroReferenceOrigin,
            find_external_definitions_for_macro_ref, find_infile_macro_references,
            find_macro_references_from_origin, find_references,
        },
        lsp::Message,
        response::{FindReferencesResponse, NullResponse, Response},
        tasks::{
            ExtMacroDefLookups, FileCallMap, FileLocationMap, FindMacroReferencesPhase,
            MacroDefinitionLocationMap, OngoingTask, Task, TaskDone, TaskProgress, Tasks,
            find_ongoing_task_by_id, trace_doc_unknown, try_schedule,
        },
    },
    protocol::{Location, NumberOrString, ReferenceParams, TraceValue, Uri},
    t32::{FindMacroRefsLangContext, FindRefsLangContext, GotoDefLangContext, MacroScope},
};

pub fn process_find_references_req(
    id: NumberOrString,
    params: ReferenceParams,
    trace_level: TraceValue,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    let (doc, tree, t32) = match docs.get_doc_data(&params.text_document.uri) {
        Some((doc, tree, t32)) => (doc, tree, t32),
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
        Task::FindReferences(
            id,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            FindRefsLangContext::from(t32.clone()),
            params.position,
            params.context.include_declaration,
            find_references,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

pub fn process_find_references_result(
    docs: &TextDocs,
    id: &NumberOrString,
    references: Option<FindReferencesResult>,
    ts: &mut Tasks,
) -> Option<FindReferencesResponse> {
    let result = match references {
        Some(FindReferencesResult::Final(refs)) => Some(refs),
        Some(FindReferencesResult::Partial(partial)) => match partial {
            FindReferencesPartialResult::MacroDefsComplete {
                origin,
                definitions,
                ..
            } => {
                debug_assert!(!definitions.is_empty());

                prepare_find_macro_references_req(
                    docs,
                    id.clone(),
                    origin,
                    definitions,
                    &mut ts.ongoing,
                );
                return None;
            }
            FindReferencesPartialResult::MacroDefsIncomplete {
                origin,
                definitions,
            } => {
                if let Some(callers) = docs.get_callers(&origin.uri) {
                    // Queues the follow-up task for definitions in external files.
                    prepare_find_external_macro_definitions_req(
                        docs,
                        id.clone(),
                        origin,
                        definitions,
                        callers.clone(),
                        &mut ts.ongoing,
                    );
                    return None;
                }

                if !definitions.is_empty() {
                    // Queues the follow-up task for all references in the file
                    // that the request originated from.
                    prepare_find_macro_references_req(
                        docs,
                        id.clone(),
                        origin,
                        definitions,
                        &mut ts.ongoing,
                    );
                    return None;
                }

                // We are dealing with an implicitly defined macro. There are
                // neither any definitions in the file where the request
                // originated from or not any callers of this file where
                // external might be defined. Implicit definitions have already
                // been checked, too, so we only have the request origin.
                Some(vec![Location {
                    uri: origin.uri,
                    range: origin.span,
                }])
            }
            FindReferencesPartialResult::FileTarget => todo!(),
        },
        None => None,
    };

    Some(FindReferencesResponse {
        id: id.clone(),
        result,
    })
}

pub fn progress_find_macro_def_references(
    docs: &TextDocs,
    task: &mut Option<OngoingTask>,
    outgoing: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    let Some(OngoingTask::FindMacroReferences { progress, .. }) = task else {
        unreachable!("Must not called with any other variant.");
    };

    if progress.finished() {
        let Some(OngoingTask::FindMacroReferences {
            id,
            onset,
            origin,
            phase:
                FindMacroReferencesPhase::ReferencesFromDefinitions {
                    results, undone, ..
                },
            ..
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        *task = Some(OngoingTask::FindMacroReferences {
            id,
            onset,
            progress: TaskProgress::new(undone.len() as u32),
            origin,
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited: Vec::new(),
                results,
                undone,
            },
        });
    } else if progress.ready() {
        next_lookups_find_macro_def_references(docs, task.as_mut().unwrap(), outgoing)?;
    }
    Ok(())
}

pub fn progress_find_subscript_macro_refs(
    docs: &TextDocs,
    task: &mut Option<OngoingTask>,
    outgoing: &mut Vec<Task>,
    done: &mut Vec<Option<TaskDone>>,
) -> Result<(), ReturnCode> {
    let Some(OngoingTask::FindMacroReferences { progress, .. }) = task else {
        unreachable!("Must not be called with any other variant.");
    };

    if progress.finished() {
        let Some(OngoingTask::FindMacroReferences {
            id,
            onset,
            progress,
            origin,
            phase:
                FindMacroReferencesPhase::ReferencesInSubscripts {
                    visited,
                    results,
                    undone,
                },
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        // Move results out of ongoing task
        *task = Some(OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset,
            progress,
            origin,
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited,
                results: FileLocationMap::new(),
                undone,
            },
        });

        done.push(Some(TaskDone::FindMacroReferences(
            id,
            Some(results.to_locations()),
        )));
    } else if progress.ready() {
        next_lookups_find_subscript_macro_refs(docs, task.as_mut().unwrap(), outgoing)?;
    }
    Ok(())
}

pub fn progress_find_external_macro_definitions(
    docs: &TextDocs,
    task: &mut Option<OngoingTask>,
    outgoing: &mut Vec<Task>,
    done: &mut Vec<Option<TaskDone>>,
) -> Result<(), ReturnCode> {
    let Some(OngoingTask::FindMacroReferences { progress, .. }) = task else {
        unreachable!("Must not be called with any other variant.");
    };

    if progress.finished() {
        let Some(OngoingTask::FindMacroReferences {
            id,
            onset,
            origin,
            phase: FindMacroReferencesPhase::ExternalDefinitions { definitions, .. },
            ..
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        if definitions.is_empty() {
            done.push(Some(TaskDone::FindMacroReferences(
                id,
                Some(vec![Location {
                    uri: origin.uri,
                    range: origin.span,
                }]),
            )));
        } else {
            *task = Some(OngoingTask::FindMacroReferences {
                id,
                onset,
                progress: TaskProgress::new(definitions.len() as u32),
                origin,
                phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                    definitions,
                    results: FileLocationMap::new(),
                    undone: Vec::new(),
                },
            });
        }
    } else if progress.ready() {
        next_lookups_find_external_macro_defs(docs, task.as_mut().unwrap(), outgoing);
    }
    Ok(())
}

pub fn recv_find_macro_def_references_sync(
    id: &NumberOrString,
    sync: FindMacroReferencesResult,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);
    let Some(OngoingTask::FindMacroReferences {
        progress,
        phase:
            FindMacroReferencesPhase::ReferencesFromDefinitions {
                results, undone, ..
            },
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
    };

    let FindMacroReferencesResult {
        uri,
        references,
        subscripts,
    } = sync;

    for r#ref in references {
        results.insert(&uri, r#ref);
    }

    for callee in subscripts {
        if !undone.contains(&callee) {
            undone.push(callee);
        }
    }

    progress.advance();
    if progress.ready() && undone.is_empty() {
        progress.abort();
    }
}

pub fn recv_find_subscript_macro_references_sync(
    id: &NumberOrString,
    sync: FindMacroReferencesResult,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);
    let Some(OngoingTask::FindMacroReferences {
        progress,
        phase:
            FindMacroReferencesPhase::ReferencesInSubscripts {
                results, undone, ..
            },
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
    };

    let FindMacroReferencesResult {
        uri,
        references,
        subscripts,
    } = sync;
    for r#ref in references {
        results.insert(&uri, r#ref);
    }

    for callee in subscripts {
        if !undone.contains(&callee) {
            undone.push(callee);
        }
    }

    progress.advance();
    if progress.ready() && undone.is_empty() {
        progress.abort();
    }
}

pub fn recv_find_external_definitions_for_macro_reference_sync(
    id: &NumberOrString,
    sync: FindDefintionsForMacroRefResult,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);
    let Some(OngoingTask::FindMacroReferences {
        progress,
        phase:
            FindMacroReferencesPhase::ExternalDefinitions {
                results, undone, ..
            },
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
    };

    let (uri, definitions, callers): (Uri, Vec<MacroDefinitionLocation>, Vec<Uri>) = match sync {
        FindDefintionsForMacroRefResult::Final(definitions, uri) => (uri, definitions, Vec::new()),
        FindDefintionsForMacroRefResult::Partial(definitions, uri, callers) => {
            (uri, definitions, callers)
        }
    };

    for def in definitions {
        results.insert(&uri, def);
    }

    for script in callers {
        undone.add(script, uri.clone());
    }

    progress.advance();
    if progress.ready() && undone.is_empty() {
        progress.abort();
    }
}

fn prepare_find_external_macro_definitions_req(
    docs: &TextDocs,
    id: NumberOrString,
    origin: MacroReferenceOrigin,
    definitions: Vec<(FileLocation, Option<MacroScope>)>,
    callers: Vec<Uri>,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    debug_assert!(callers.len() > 0);
    let num = callers.len();

    let lookups: ExtMacroDefLookups = {
        let (mut files, mut callees): (Vec<Uri>, Vec<Uri>) =
            (Vec::with_capacity(num), Vec::with_capacity(num));

        for file in callers {
            files.push(file.clone());
            callees.push(origin.uri.clone());
        }
        ExtMacroDefLookups { files, callees }
    };

    let definitions: Vec<MacroPropagation> = {
        let mut defs: Vec<MacroPropagation> = Vec::with_capacity(definitions.len());
        for (loc, scope) in definitions {
            defs.push(MacroPropagation::new(docs, &origin.name, loc, scope));
        }
        defs
    };

    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let Some(OngoingTask::FindReferences(_, onset)) = ongoing[idx].take() else {
        unreachable!("No other type possible.");
    };

    ongoing[idx] = Some(OngoingTask::FindMacroReferences {
        id,
        onset: onset,
        progress: TaskProgress::new(lookups.files.len() as u32),
        origin,
        phase: FindMacroReferencesPhase::ExternalDefinitions {
            definitions,
            visited: FileCallMap::new(),
            results: MacroDefinitionLocationMap::new(),
            undone: lookups,
        },
    });
}

fn prepare_find_macro_references_req(
    docs: &TextDocs,
    id: NumberOrString,
    origin: MacroReferenceOrigin,
    definitions: Vec<(FileLocation, Option<MacroScope>)>,
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let Some(OngoingTask::FindReferences(_, onset)) = &ongoing[idx].take() else {
        unreachable!("Must not retrieve any other type.");
    };

    let definitions: Vec<MacroPropagation> = {
        let mut defs: Vec<MacroPropagation> = Vec::with_capacity(definitions.len());
        for (loc, scope) in definitions {
            defs.push(MacroPropagation::new(docs, &origin.name, loc, scope));
        }
        defs
    };

    ongoing[idx] = Some(OngoingTask::FindMacroReferences {
        id,
        onset: onset.clone(),
        progress: TaskProgress::new(definitions.len() as u32),
        origin,
        phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
            definitions,
            results: FileLocationMap::new(),
            undone: Vec::new(),
        },
    });
}

fn next_lookups_find_macro_def_references(
    docs: &TextDocs,
    task: &mut OngoingTask,
    next: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    let OngoingTask::FindMacroReferences {
        id,
        progress,
        origin,
        phase: FindMacroReferencesPhase::ReferencesFromDefinitions { definitions, .. },
        ..
    } = task
    else {
        unreachable!("Must not be called with any other variant.");
    };
    debug_assert!(progress.ready());

    let mut touched: Vec<u8> = vec![0; definitions.len()];
    let mut total: u32 = 0;

    // Group all lookups originating from the same file into a single request.
    for (ii, first) in definitions.iter().enumerate() {
        if touched[ii] > 0 {
            continue;
        }
        let uri = first.get_uri();

        let mut defs: Vec<MacroPropagationCompact> =
            vec![MacroPropagationCompact::from(first.clone())];

        for (jj, next) in definitions[ii + 1..].iter().enumerate() {
            if touched[jj] > 0 || *next.get_uri() != *uri {
                continue;
            }
            defs.push(MacroPropagationCompact::from(next.clone()));
            touched[jj] = 1;
        }

        let (doc, tree, t32) = docs
            .get_doc_data(uri)
            .expect("File must be known at this point.");

        next.push(Task::FindMacroReferencesFromDefinitions(
            id.clone(),
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            FindMacroRefsLangContext::from(t32.clone()),
            origin.name.clone(),
            defs,
            find_macro_references_from_origin,
        ));

        touched[ii] = 1;
        total += 1;
    }
    progress.total = total;

    Ok(())
}

fn next_lookups_find_subscript_macro_refs(
    docs: &TextDocs,
    task: &mut OngoingTask,
    next: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    let OngoingTask::FindMacroReferences {
        id,
        progress,
        origin,
        phase:
            FindMacroReferencesPhase::ReferencesInSubscripts {
                visited, undone, ..
            },
        ..
    } = task
    else {
        unreachable!("Must not be called with any other variant.");
    };
    debug_assert!(progress.ready());

    // It is fine to visit files containing the macro defintion a second time.
    // The second iteration will stop as soon as it encounters the macro
    // definition.
    let mut total: u32 = 0;
    for uri in undone.iter().filter(|&s| !visited.contains(s)) {
        let (doc, tree, t32) = docs
            .get_doc_data(uri)
            .expect("File must be known at this point.");

        next.push(Task::FindMacroReferencesInSubscripts(
            id.clone(),
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            FindMacroRefsLangContext::from(t32.clone()),
            origin.name.clone(),
            find_infile_macro_references,
        ));
        total += 1;
    }
    progress.total = total;
    undone.clear();

    Ok(())
}

fn queue_find_external_macro_definitions_req(
    id: &NumberOrString,
    docs: &TextDocs,
    origin: &MacroReferenceOrigin,
    undone: &mut ExtMacroDefLookups,
    visited: &mut FileCallMap,
    outgoing: &mut Vec<Task>,
) -> u32 {
    let mut total: u32 = 0;
    for (script, callee) in undone.files.iter().zip(undone.callees.iter()) {
        if let Some(seen) = visited.get(script)
            && seen.contains(callee)
        {
            continue;
        }
        total += 1;

        let (doc, tree, t32) = docs.get_doc_data(script).unwrap();
        let callers = match docs.get_callers(script) {
            Some(files) => files.clone(),
            None => Vec::new(),
        };

        outgoing.push(Task::FindExternalDefinitionsForMacroRef {
            id: id.clone(),
            textdoc: TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            t32: GotoDefLangContext::from(t32.clone()),
            callers: callers,
            origin: origin.clone(),
            callee: callee.clone(),
            find: find_external_definitions_for_macro_ref,
        });
        visited.insert(script.clone(), callee.clone());
    }
    undone.clear();

    total
}

fn next_lookups_find_external_macro_defs(
    docs: &TextDocs,
    task: &mut OngoingTask,
    next: &mut Vec<Task>,
) {
    let (id, origin, progress, undone, visited): (
        &NumberOrString,
        &MacroReferenceOrigin,
        &mut TaskProgress,
        &mut ExtMacroDefLookups,
        &mut FileCallMap,
    ) = match task {
        OngoingTask::FindMacroReferences {
            id,
            progress,
            origin,
            phase:
                FindMacroReferencesPhase::ExternalDefinitions {
                    visited, undone, ..
                },
            ..
        } => {
            if undone.is_empty() {
                return;
            }
            (id, origin, progress, undone, visited)
        }
        _ => unreachable!("Must not be called with any other variant."),
    };
    progress.total =
        queue_find_external_macro_definitions_req(id, docs, origin, undone, visited, next);
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{time::Instant, u32};

    use crate::protocol::{Position, Range};

    #[test]
    fn skips_redundant_subscript_macro_ref_checks() {
        let docs = TextDocs::new();

        let mut task = Some(OngoingTask::FindMacroReferences {
            id: NumberOrString::Number(1),
            onset: Instant::now(),
            progress: TaskProgress {
                completed: 0,
                total: 3,
                cycles: 0,
                max_cycles: u32::MAX,
            },
            origin: MacroReferenceOrigin {
                name: "test".to_string(),
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 5,
                    },
                },
                uri: "test.cmm".to_string(),
            },
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited: vec![
                    "file:///sample/a.cmm".to_string(),
                    "file:///sample/b.cmm".to_string(),
                ],
                undone: vec![
                    "file:///sample/a.cmm".to_string(),
                    "file:///sample/b.cmm".to_string(),
                ],
                results: FileLocationMap::new(),
            },
        });

        let mut outgoing: Vec<Task> = Vec::new();
        let mut completed: Vec<Option<TaskDone>> = Vec::new();

        progress_find_subscript_macro_refs(&docs, &mut task, &mut outgoing, &mut completed)
            .unwrap();

        assert!(outgoing.is_empty());
        assert!(completed.is_empty());

        progress_find_subscript_macro_refs(&docs, &mut task, &mut outgoing, &mut completed)
            .unwrap();

        assert!(outgoing.is_empty());
        assert!(!completed.is_empty());
    }
}
