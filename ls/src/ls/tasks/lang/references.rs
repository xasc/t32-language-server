// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2
//

//! [Note] Workflow Macro Reference Retrieval
//! =========================================
//!
//! The prerequisite for finding all locations where a macro is referenced is
//! the detection of all corresponding macro definitions. Once the definitions
//! are known, we can determine all references in both the same file and all
//! called scripts.
//! The full workflow for macro reference retrieval looks like this:
//!
//!   1.  Find all macro definitions in the file of the initial macro location.
//!       It initial macro location has been selected by the client.
//!   2.  If the file with the initial macro location is called by other
//!       scripts and any macro definitions may originate from calling scripts,
//!       we look for additional macro definitions in callers. The step is
//!       skipped if there are no calling scripts or all macro definitions were
//!       already found in the script with the initial macro reference.
//!   3.  All macro references in the file with a corresponding macro definition
//!       can be found in a single file iteration over the file. However, we
//!       need to capture all calls to subscripts for the next phase.
//!   4.  Capture all macros references in subscripts that are called from the
//!       files with a corresponding macro definitions.
//!

use serde_json::json;

use crate::{
    ls::{
        ReturnCode,
        doc::{TextDocData, TextDocs},
        language::{
            FileLocation, FindDefintionsForMacroRefResult, FindMacroReferencesResult,
            FindReferencesPartialResult, FindReferencesResult, MacroDefinitionLocation,
            MacroReferenceOrigin, find_external_definitions_for_macro_ref,
            find_infile_macro_references, find_macro_references_from_origin, find_references,
        },
        lsp::Message,
        request::Notification,
        response::{FindReferencesResponse, NullResponse, Response},
        tasks::{
            ExtMacroDefLookups, FileCallMap, FindMacroReferencesPhase, MacroDefinitionLocationMap,
            OngoingTask, Task, TaskDone, TaskProgress, Tasks, find_ongoing_task_by_id,
            trace_doc_unknown, try_schedule,
        },
        workspace::FileIndex,
    },
    protocol::{Location, LogTraceParams, NumberOrString, ReferenceParams, TraceValue, Uri},
    t32::{
        FindMacroRefsLangContext, FindRefsLangContext, GotoDefLangContext, MacroScope,
        resolve_script,
    },
    utils::FileLocationMap,
};

pub fn process_find_references_req(
    id: NumberOrString,
    params: ReferenceParams,
    trace_level: TraceValue,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_find_ref_req(id.clone())));
    }

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
        Task::FindReferences {
            id,
            textdoc: TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            t32: FindRefsLangContext::from(t32.clone()),
            position: params.position,
            declaration_included: params.context.include_declaration,
            find: find_references,
        },
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

pub fn process_find_references_result(
    docs: &TextDocs,
    files: &FileIndex,
    id: &NumberOrString,
    references: Option<FindReferencesResult>,
    trace_level: TraceValue,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
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

                if trace_level != TraceValue::Off {
                    outgoing.push(Some(log_conv_find_macro_ref_req(
                        id.clone(),
                        origin.clone(),
                    )));
                }
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
                    if trace_level != TraceValue::Off {
                        outgoing.push(Some(log_conv_find_macro_ref_req(
                            id.clone(),
                            origin.clone(),
                        )));
                    }
                    // Queues the follow-up task for definitions in external files.
                    prepare_find_external_macro_definitions_req(
                        id.clone(),
                        origin,
                        definitions,
                        callers.clone(),
                        &mut ts.ongoing,
                    );
                    return None;
                }

                if !definitions.is_empty() {
                    if trace_level != TraceValue::Off {
                        outgoing.push(Some(log_conv_find_macro_ref_req(
                            id.clone(),
                            origin.clone(),
                        )));
                    }
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
                // originated from nor any callers of this file where
                // external might be defined. Implicit definitions have already
                // been checked, so the only result is the request origin.
                Some(vec![Location {
                    uri: origin.uri,
                    range: origin.span,
                }])
            }
            // TODO: Include the script itself, if request asked to include declarations?
            FindReferencesPartialResult::FileTarget(target) => {
                if let Some(scripts) = resolve_script(&target, files) {
                    let mut locations: Vec<Location> = Vec::new();
                    for script in scripts {
                        if let Some(locs) = docs.get_all_target_file_refs(&script) {
                            for (file, spans) in locs.iter() {
                                for span in spans {
                                    locations.push(Location {
                                        uri: file.clone(),
                                        range: span.clone(),
                                    });
                                }
                            }
                        }
                    }

                    if locations.is_empty() {
                        None
                    } else {
                        Some(locations)
                    }
                } else {
                    None
                }
            }
            FindReferencesPartialResult::Command(cmd) => {
                if let Some(commands) = docs.get_all_command_refs(&cmd) {
                    let mut locations: Vec<Location> = Vec::new();
                    for (file, spans) in commands.iter() {
                        for span in spans {
                            locations.push(Location {
                                uri: file.clone(),
                                range: span.clone(),
                            });
                        }
                    }

                    if locations.is_empty() {
                        None
                    } else {
                        Some(locations)
                    }
                } else {
                    None
                }
            }
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
                    subscripts,
                    results,
                    ..
                },
            ..
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        *task = Some(OngoingTask::FindMacroReferences {
            id,
            onset,
            progress: TaskProgress::new(subscripts.len() as u32),
            origin,
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited: Vec::new(),
                results,
                undone: subscripts,
            },
        });
    } else if progress.ready() {
        next_lookups_find_macro_def_references(docs, task.as_mut().unwrap(), outgoing);
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
        next_lookups_find_subscript_macro_refs(docs, task.as_mut().unwrap(), outgoing);
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
            phase:
                FindMacroReferencesPhase::ExternalDefinitions {
                    results: definitions,
                    ..
                },
            ..
        }) = task.take()
        else {
            unreachable!("Must not be called with any other variant.");
        };

        if definitions.is_empty() {
            // We have found no macro definition. The only
            // valid macro reference is the origin.
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
                progress: TaskProgress::new(definitions.num_files() as u32),
                origin,
                phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                    subscripts: Vec::new(),
                    results: FileLocationMap::new(),
                    undone: definitions,
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
                subscripts,
                results,
                undone,
                ..
            },
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
    };

    let FindMacroReferencesResult {
        uri,
        references,
        subscripts: new_subscripts,
    } = sync;

    for r#ref in references {
        results.insert(&uri, r#ref);
    }

    for file in new_subscripts {
        if !subscripts.contains(&file) {
            subscripts.push(file);
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

    let definitions: MacroDefinitionLocationMap = {
        let mut defs: MacroDefinitionLocationMap = MacroDefinitionLocationMap::new();
        for (FileLocation { uri, range }, scope) in definitions {
            defs.insert(
                &uri,
                MacroDefinitionLocation::from_span(scope.as_ref(), range),
            );
        }
        defs
    };

    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let Some(OngoingTask::FindReferences(_, onset)) = ongoing[idx].take() else {
        unreachable!("Must not retrieve any other variant.");
    };

    ongoing[idx] = Some(OngoingTask::FindMacroReferences {
        id,
        onset: onset,
        progress: TaskProgress::new(lookups.num_files() as u32),
        origin,
        phase: FindMacroReferencesPhase::ExternalDefinitions {
            visited: FileCallMap::new(),
            results: definitions,
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
    let Some(OngoingTask::FindReferences(_, onset)) = ongoing[idx].take() else {
        unreachable!("Must not retrieve any other type.");
    };

    let (definitions, subscripts): (MacroDefinitionLocationMap, Vec<Uri>) = {
        let mut defs: MacroDefinitionLocationMap = MacroDefinitionLocationMap::new();
        let mut scripts: Vec<Uri> = Vec::new();

        for (FileLocation { uri, range }, scope) in definitions {
            // Add all files containing macro references with the same name as
            // the global macro to the search list. Any of these files might
            // contain a reference the global macro. The check is performed
            // when subscripts are checked for macro references.
            if let Some(MacroScope::Global) = scope {
                let refs = match docs.get_all_scripts_with_macro(&origin.name) {
                    Some(files) => files.clone(),
                    None => Vec::new(),
                };

                for file in refs {
                    if !scripts.contains(&file) {
                        scripts.push(file);
                    }
                }
            }

            defs.insert(
                &uri,
                MacroDefinitionLocation::from_span(scope.as_ref(), range),
            );
        }
        (defs, scripts)
    };

    ongoing[idx] = Some(OngoingTask::FindMacroReferences {
        id,
        onset,
        progress: TaskProgress::new(definitions.num_files() as u32),
        origin,
        phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
            subscripts,
            results: FileLocationMap::new(),
            undone: definitions,
        },
    });
}

fn next_lookups_find_macro_def_references(
    docs: &TextDocs,
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
) {
    let OngoingTask::FindMacroReferences {
        id,
        progress,
        origin,
        phase: FindMacroReferencesPhase::ReferencesFromDefinitions { undone, .. },
        ..
    } = task
    else {
        unreachable!("Must not be called with any other variant.");
    };
    debug_assert!(progress.ready());

    progress.total = undone.num_files() as u32;

    for (uri, definitions) in undone.iter() {
        let (doc, tree, t32) = docs
            .get_doc_data(uri)
            .expect("File must be known at this point.");

        outgoing.push(Task::FindMacroReferencesFromDefinitions {
            id: id.clone(),
            textdoc: TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            t32: FindMacroRefsLangContext::from(t32.clone()),
            r#macro: origin.name.clone(),
            definitions: definitions.clone(),
            find: find_macro_references_from_origin,
        });
    }
    undone.clear();

    progress.ack_ready();
}

fn next_lookups_find_subscript_macro_refs(
    docs: &TextDocs,
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
) {
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
    undone.retain(|s| !visited.contains(s));

    for uri in undone.into_iter() {
        let (doc, tree, t32) = docs
            .get_doc_data(uri)
            .expect("File must be known at this point.");

        outgoing.push(Task::FindMacroReferencesInSubscripts {
            id: id.clone(),
            textdoc: TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            t32: FindMacroRefsLangContext::from(t32.clone()),
            r#macro: origin.name.clone(),
            find: find_infile_macro_references,
        });
        total += 1;
    }
    visited.append(undone);

    progress.total = total;
    progress.ack_ready();
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
    outgoing: &mut Vec<Task>,
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
        queue_find_external_macro_definitions_req(id, docs, origin, undone, visited, outgoing);
    progress.ack_ready();
}

fn log_find_ref_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received find references request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}

fn log_conv_find_macro_ref_req(id: NumberOrString, r#macro: MacroReferenceOrigin) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Request with ID \"{:}\" converted to find macro references request.",
                id
            ),
            verbose: Some(json!(r#macro).to_string()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{time::Instant, u32};

    use crate::{
        ls::{TaskCounterInternal, TaskSystem, tasks::TaskProgress},
        protocol::{Position, Range},
        utils::{BRange, create_doc_store, create_file_idx, files, to_file_uri},
    };

    #[test]
    fn can_queue_lookups_for_macro_definition_references() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let origin = MacroReferenceOrigin {
            name: "&local_macro".to_string(),
            span: Range {
                start: Position {
                    line: 38,
                    character: 4,
                },
                end: Position {
                    line: 38,
                    character: 16,
                },
            },
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let definitions: MacroDefinitionLocationMap = {
            let mut defs = MacroDefinitionLocationMap::new();

            defs.insert(
                &origin.uri,
                MacroDefinitionLocation::Local(BRange::from(509..521)),
            );
            defs
        };

        let mut ongoing = OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(10),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                subscripts: Vec::new(),
                results: FileLocationMap::new(),
                undone: definitions.clone(),
            },
        };

        let mut progress = TaskProgress::new(1);
        progress.ack_ready();

        let mut outgoing: Vec<Task> = Vec::new();

        next_lookups_find_macro_def_references(&docs, &mut ongoing, &mut outgoing);

        assert!(
            ongoing
                == OngoingTask::FindMacroReferences {
                    id,
                    onset,
                    progress,
                    origin,
                    phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                        subscripts: Vec::new(),
                        results: FileLocationMap::new(),
                        undone: MacroDefinitionLocationMap::new(),
                    },
                }
        );
        assert!(!outgoing.is_empty());
    }

    #[test]
    fn can_queue_global_macro_reference_lookups() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(3);
        let onset = Instant::now();

        let origin = MacroReferenceOrigin {
            name: "&global_macro".to_string(),
            span: Range {
                start: Position {
                    line: 41,
                    character: 7,
                },
                end: Position {
                    line: 41,
                    character: 20,
                },
            },
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let definitions: Vec<(FileLocation, Option<MacroScope>)> = {
            let mut defs: Vec<(FileLocation, Option<MacroScope>)> = Vec::new();

            defs.push((
                FileLocation {
                    uri: to_file_uri("tests/samples/a/a.cmm"),
                    range: BRange::from(489..502),
                },
                Some(MacroScope::Global),
            ));
            defs
        };

        let mut ongoing = vec![Some(OngoingTask::FindReferences(id.clone(), onset.clone()))];

        let undone: MacroDefinitionLocationMap = {
            let mut macros = MacroDefinitionLocationMap::new();

            for (FileLocation { uri, range }, scope) in definitions.iter() {
                macros.insert(
                    uri,
                    MacroDefinitionLocation::from_span(scope.as_ref(), range.clone()),
                );
            }
            macros
        };

        prepare_find_macro_references_req(
            &docs,
            id.clone(),
            origin.clone(),
            definitions,
            &mut ongoing,
        );

        let ongoing = ongoing[0].take();
        assert!(ongoing.as_ref().is_some_and(|t| *t
            == OngoingTask::FindMacroReferences {
                id,
                onset,
                progress: TaskProgress::new(undone.num_files() as u32),
                origin,
                phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                    subscripts: vec![
                        to_file_uri("tests/samples/c.cmm"),
                        to_file_uri("tests/samples/a/a.cmm"),
                    ],
                    results: FileLocationMap::new(),
                    undone,
                },
            }));

        assert!(ongoing.is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                panic!()
            };

            progress.ready()
        }));
    }

    #[test]
    fn skips_redundant_subscript_macro_ref_checks() {
        let docs = TextDocs::new();

        let mut task = Some(OngoingTask::FindMacroReferences {
            id: NumberOrString::Number(1),
            onset: Instant::now(),
            progress: TaskProgress::new(3),
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

    #[test]
    fn can_process_find_refs_result_for_macro_only_defined_in_file() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut ts = Tasks {
            runner: TaskSystem::build(),
            blocked: Vec::new(),
            ongoing: vec![Some(OngoingTask::FindReferences(id.clone(), onset.clone()))],
            completed: Vec::new(),
            counter: TaskCounterInternal::new(),
        };

        let origin = MacroReferenceOrigin {
            name: "&a".to_string(),
            span: Range {
                start: Position {
                    line: 10,
                    character: 0,
                },
                end: Position {
                    line: 10,
                    character: 2,
                },
            },
            uri: to_file_uri("tests/samples/a/d/d.cmm"),
        };

        let find_refs_res = Some(FindReferencesResult::Partial(
            FindReferencesPartialResult::MacroDefsComplete {
                origin: origin.clone(),
                definitions: vec![(
                    FileLocation {
                        uri: to_file_uri("tests/samples/a/d/d.cmm"),
                        range: BRange::from(149..151),
                    },
                    Some(MacroScope::Local),
                )],
            },
        ));

        let definitions: MacroDefinitionLocationMap = {
            let mut defs = MacroDefinitionLocationMap::new();

            defs.insert(
                &origin.uri,
                MacroDefinitionLocation::Local(BRange::from(149..151)),
            );
            defs
        };

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let result = process_find_references_result(
            &docs,
            &file_idx,
            &id,
            find_refs_res,
            TraceValue::Off,
            &mut ts,
            &mut outgoing,
        );

        assert!(result.is_none());
        assert!(ts.completed.is_empty());
        assert!(ts.ongoing[0].as_ref().is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                unreachable!()
            };
            progress.ready()
        }));
        assert!(ts.ongoing[0].take().is_some_and(|t| t
            == OngoingTask::FindMacroReferences {
                id: id,
                onset,
                progress: TaskProgress::new(1),
                origin,
                phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                    subscripts: Vec::new(),
                    results: FileLocationMap::new(),
                    undone: definitions,
                },
            }));
    }

    #[test]
    fn can_process_find_refs_result_for_externally_defined_macro() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut ts = Tasks {
            runner: TaskSystem::build(),
            blocked: Vec::new(),
            ongoing: vec![Some(OngoingTask::FindReferences(id.clone(), onset.clone()))],
            completed: Vec::new(),
            counter: TaskCounterInternal::new(),
        };

        let origin = MacroReferenceOrigin {
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
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let lookups: ExtMacroDefLookups = {
            let mut def_lookups = ExtMacroDefLookups::new();

            def_lookups.add(
                to_file_uri("tests/samples/c.cmm"),
                to_file_uri("tests/samples/a/a.cmm"),
            );

            def_lookups.add(
                to_file_uri("tests/samples/a/d/d.cmm"),
                to_file_uri("tests/samples/a/a.cmm"),
            );
            def_lookups
        };

        let find_refs_res = Some(FindReferencesResult::Partial(
            FindReferencesPartialResult::MacroDefsIncomplete {
                origin: origin.clone(),
                definitions: Vec::new(),
            },
        ));

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let result = process_find_references_result(
            &docs,
            &file_idx,
            &id,
            find_refs_res,
            TraceValue::Off,
            &mut ts,
            &mut outgoing,
        );

        assert!(result.is_none());

        assert!(ts.ongoing[0].as_ref().is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                unreachable!()
            };
            progress.ready()
        }));

        assert!(ts.ongoing[0].take().is_some_and(|t| t
            == OngoingTask::FindMacroReferences {
                id,
                onset,
                progress: TaskProgress::new(2),
                origin,
                phase: FindMacroReferencesPhase::ExternalDefinitions {
                    visited: FileCallMap::new(),
                    results: MacroDefinitionLocationMap::new(),
                    undone: lookups,
                }
            }));
    }

    #[test]
    fn can_process_find_refs_result_for_macro_without_definition() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut ts = Tasks {
            runner: TaskSystem::build(),
            blocked: Vec::new(),
            ongoing: vec![Some(OngoingTask::FindReferences(id.clone(), onset.clone()))],
            completed: Vec::new(),
            counter: TaskCounterInternal::new(),
        };

        let origin = MacroReferenceOrigin {
            name: "&undefined".to_string(),
            span: Range {
                start: Position {
                    line: 6,
                    character: 7,
                },
                end: Position {
                    line: 6,
                    character: 17,
                },
            },
            uri: to_file_uri("tests/samples/orphan.cmm"),
        };

        let find_refs_res = Some(FindReferencesResult::Partial(
            FindReferencesPartialResult::MacroDefsIncomplete {
                origin: origin.clone(),
                definitions: Vec::new(),
            },
        ));

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let result = process_find_references_result(
            &docs,
            &file_idx,
            &id,
            find_refs_res,
            TraceValue::Off,
            &mut ts,
            &mut outgoing,
        );

        assert!(result.is_some_and(|r| r
            == FindReferencesResponse {
                id,
                result: Some(vec![Location {
                    uri: origin.uri,
                    range: origin.span
                }])
            }));
    }

    #[test]
    fn can_progress_macro_ref_lookup_from_definition() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut progress = TaskProgress::new(1);

        let origin = MacroReferenceOrigin {
            name: "&local_macro".to_string(),
            span: Range {
                start: Position {
                    line: 38,
                    character: 4,
                },
                end: Position {
                    line: 38,
                    character: 16,
                },
            },
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let references: Vec<Range> = vec![
            Range {
                start: Position {
                    line: 38,
                    character: 4,
                },
                end: Position {
                    line: 38,
                    character: 16,
                },
            },
            Range {
                start: Position {
                    line: 139,
                    character: 7,
                },
                end: Position {
                    line: 139,
                    character: 18,
                },
            },
            Range {
                start: Position {
                    line: 162,
                    character: 7,
                },
                end: Position {
                    line: 162,
                    character: 18,
                },
            },
        ];

        let undone: MacroDefinitionLocationMap = {
            let mut defs = MacroDefinitionLocationMap::new();

            defs.insert(
                &origin.uri,
                MacroDefinitionLocation::Local(BRange::from(509..521)),
            );
            defs
        };

        let sync = FindMacroReferencesResult {
            uri: to_file_uri("tests/samples/a/a.cmm"),
            references: references.clone(),
            subscripts: vec![to_file_uri("test/samples/b/b.cmm")],
        };

        let mut ongoing = vec![Some(OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                subscripts: Vec::new(),
                results: FileLocationMap::new(),
                undone: undone.clone(),
            },
        })];

        let results: FileLocationMap = {
            let mut locations = FileLocationMap::new();
            for loc in references {
                locations.insert(&sync.uri, loc);
            }
            locations
        };
        progress.set_cycles(1);

        recv_find_macro_def_references_sync(&id, sync, &mut ongoing);

        let ongoing = ongoing[0].take();
        assert!(ongoing.as_ref().is_some_and(|t| *t
            == OngoingTask::FindMacroReferences {
                id,
                onset,
                progress,
                origin,
                phase: FindMacroReferencesPhase::ReferencesFromDefinitions {
                    subscripts: vec![to_file_uri("test/samples/b/b.cmm")],
                    results,
                    undone,
                },
            }));

        assert!(ongoing.as_ref().is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                panic!()
            };
            progress.ready()
        }));

        assert!(ongoing.is_some_and(|t| {
            let OngoingTask::FindMacroReferences {
                phase: FindMacroReferencesPhase::ReferencesFromDefinitions { subscripts, .. },
                ..
            } = t
            else {
                panic!()
            };
            subscripts == vec![to_file_uri("test/samples/b/b.cmm")]
        }));
    }

    #[test]
    fn can_queue_lookups_for_externally_defined_macros() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut progress = TaskProgress::new(2);

        let origin = MacroReferenceOrigin {
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
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let undone: ExtMacroDefLookups = {
            let mut def_lookups = ExtMacroDefLookups::new();

            def_lookups.add(
                to_file_uri("tests/samples/c.cmm"),
                to_file_uri("tests/samples/a/a.cmm"),
            );

            def_lookups.add(
                to_file_uri("tests/samples/a/d/d.cmm"),
                to_file_uri("tests/samples/a/a.cmm"),
            );
            def_lookups
        };

        let mut ongoing = OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ExternalDefinitions {
                visited: FileCallMap::new(),
                results: MacroDefinitionLocationMap::new(),
                undone: undone.clone(),
            },
        };

        let visited: FileCallMap = {
            let mut calls = FileCallMap::new();

            for (file, callee) in undone.files.into_iter().zip(undone.callees.into_iter()) {
                calls.insert(file, callee);
            }
            calls
        };
        progress.ack_ready();

        let mut outgoing: Vec<Task> = Vec::new();

        next_lookups_find_external_macro_defs(&docs, &mut ongoing, &mut outgoing);

        assert!(
            ongoing
                == OngoingTask::FindMacroReferences {
                    id,
                    onset,
                    progress,
                    origin,
                    phase: FindMacroReferencesPhase::ExternalDefinitions {
                        visited,
                        results: MacroDefinitionLocationMap::new(),
                        undone: ExtMacroDefLookups::new(),
                    }
                }
        );
        assert!(!outgoing.is_empty());
    }

    #[test]
    fn can_skip_redundant_lookups_for_externally_defined_macros() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let origin = MacroReferenceOrigin {
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
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let undone: ExtMacroDefLookups = {
            let mut def_lookups = ExtMacroDefLookups::new();

            def_lookups.add(
                to_file_uri("tests/samples/c.cmm"),
                to_file_uri("tests/samples/a/a.cmm"),
            );

            def_lookups.add(
                to_file_uri("tests/samples/a/d/d.cmm"),
                to_file_uri("tests/samples/a/a.cmm"),
            );
            def_lookups
        };

        let visited: FileCallMap = {
            let mut calls = FileCallMap::new();

            for (file, callee) in undone
                .clone()
                .files
                .into_iter()
                .zip(undone.clone().callees.into_iter())
            {
                calls.insert(file, callee);
            }
            calls
        };

        let mut ongoing = OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(2),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ExternalDefinitions {
                visited: visited.clone(),
                results: MacroDefinitionLocationMap::new(),
                undone: undone.clone(),
            },
        };

        let mut progress = TaskProgress::new(0);
        progress.ack_ready();

        let mut outgoing: Vec<Task> = Vec::new();

        next_lookups_find_external_macro_defs(&docs, &mut ongoing, &mut outgoing);

        assert!(
            ongoing
                == OngoingTask::FindMacroReferences {
                    id,
                    onset,
                    progress,
                    origin,
                    phase: FindMacroReferencesPhase::ExternalDefinitions {
                        visited,
                        results: MacroDefinitionLocationMap::new(),
                        undone: ExtMacroDefLookups::new(),
                    }
                }
        );
        assert!(outgoing.is_empty());
    }

    #[test]
    fn can_progress_external_definition_lookup_for_partial_definitions() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let progress = TaskProgress::new(1);

        let origin = MacroReferenceOrigin {
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
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let mut ongoing = vec![Some(OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ExternalDefinitions {
                visited: FileCallMap::new(),
                results: MacroDefinitionLocationMap::new(),
                undone: ExtMacroDefLookups::new(),
            },
        })];

        let sync = FindDefintionsForMacroRefResult::Partial(
            Vec::new(),
            to_file_uri("tests/samples/a/d/d.cmm"),
            vec![to_file_uri("tests/samples/c.cmm")],
        );

        let lookups: ExtMacroDefLookups = {
            let mut lu: ExtMacroDefLookups = ExtMacroDefLookups::new();

            lu.add(
                to_file_uri("tests/samples/c.cmm"),
                to_file_uri("tests/samples/a/d/d.cmm"),
            );
            lu
        };

        let mut progress = TaskProgress::new(1);
        progress.set_cycles(1);

        recv_find_external_definitions_for_macro_reference_sync(&id, sync, &mut ongoing);

        let ongoing = ongoing[0].take();
        assert!(ongoing.as_ref().is_some_and(|t| *t
            == OngoingTask::FindMacroReferences {
                id,
                onset,
                progress,
                origin,
                phase: FindMacroReferencesPhase::ExternalDefinitions {
                    visited: FileCallMap::new(),
                    results: MacroDefinitionLocationMap::new(),
                    undone: lookups,
                },
            }));

        assert!(ongoing.is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                panic!()
            };
            progress.ready()
        }));
    }

    #[test]
    fn can_progress_external_definition_lookup_for_final_definitions() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let progress = TaskProgress::new(1);

        let origin = MacroReferenceOrigin {
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
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let definitions: Vec<MacroDefinitionLocation> = vec![
            MacroDefinitionLocation::Local(BRange::from(411..422)),
            MacroDefinitionLocation::Local(BRange::from(497..508)),
        ];

        let mut ongoing = vec![Some(OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ExternalDefinitions {
                visited: FileCallMap::new(),
                results: MacroDefinitionLocationMap::new(),
                undone: ExtMacroDefLookups::new(),
            },
        })];

        let sync = FindDefintionsForMacroRefResult::Final(
            definitions.clone(),
            to_file_uri("tests/samples/c.cmm"),
        );

        let results: MacroDefinitionLocationMap = {
            let uri = to_file_uri("tests/samples/c.cmm");

            let mut locations = MacroDefinitionLocationMap::new();
            for loc in definitions {
                locations.insert(&uri, loc);
            }
            locations
        };

        let mut progress = TaskProgress::new(0);
        progress.set_cycles(1);

        recv_find_external_definitions_for_macro_reference_sync(&id, sync, &mut ongoing);

        let ongoing = ongoing[0].take();
        assert!(ongoing.as_ref().is_some_and(|t| *t
            == OngoingTask::FindMacroReferences {
                id,
                onset,
                progress,
                origin,
                phase: FindMacroReferencesPhase::ExternalDefinitions {
                    visited: FileCallMap::new(),
                    results,
                    undone: ExtMacroDefLookups::new(),
                },
            }));

        assert!(ongoing.is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                panic!()
            };
            progress.finished()
        }));
    }

    #[test]
    fn can_queue_lookups_for_subscript_macro_references() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let origin = MacroReferenceOrigin {
            name: "&local_macro".to_string(),
            span: Range {
                start: Position {
                    line: 38,
                    character: 4,
                },
                end: Position {
                    line: 38,
                    character: 16,
                },
            },
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let mut progress = TaskProgress::new(1);

        let mut ongoing = OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited: Vec::new(),
                results: FileLocationMap::new(),
                undone: vec![to_file_uri("tests/samples/b/b.cmm")],
            },
        };
        progress.ack_ready();

        let mut outgoing: Vec<Task> = Vec::new();

        next_lookups_find_subscript_macro_refs(&docs, &mut ongoing, &mut outgoing);

        assert!(
            ongoing
                == OngoingTask::FindMacroReferences {
                    id,
                    onset,
                    progress,
                    origin,
                    phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                        visited: vec![to_file_uri("tests/samples/b/b.cmm")],
                        results: FileLocationMap::new(),
                        undone: Vec::new(),
                    },
                }
        );
        assert!(!outgoing.is_empty());
    }

    #[test]
    fn visits_subscripts_for_macro_reference_lookup_only_once() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let origin = MacroReferenceOrigin {
            name: "&local_macro".to_string(),
            span: Range {
                start: Position {
                    line: 38,
                    character: 4,
                },
                end: Position {
                    line: 38,
                    character: 16,
                },
            },
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let mut ongoing = OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(1),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited: vec![to_file_uri("tests/samples/b/b.cmm")],
                results: FileLocationMap::new(),
                undone: vec![to_file_uri("tests/samples/b/b.cmm")],
            },
        };

        let mut progress = TaskProgress::new(0);
        progress.ack_ready();

        let mut outgoing: Vec<Task> = Vec::new();

        next_lookups_find_subscript_macro_refs(&docs, &mut ongoing, &mut outgoing);

        assert!(
            ongoing
                == OngoingTask::FindMacroReferences {
                    id,
                    onset,
                    progress,
                    origin,
                    phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                        visited: vec![to_file_uri("tests/samples/b/b.cmm")],
                        results: FileLocationMap::new(),
                        undone: Vec::new(),
                    },
                }
        );
        assert!(outgoing.is_empty());
    }

    #[test]
    fn can_progress_macro_ref_lookup_in_subscripts() {
        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut progress = TaskProgress::new(1);

        let origin = MacroReferenceOrigin {
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
            uri: to_file_uri("tests/samples/a/a.cmm"),
        };

        let references: Vec<Range> = vec![
            Range {
                start: Position {
                    line: 139,
                    character: 7,
                },
                end: Position {
                    line: 139,
                    character: 18,
                },
            },
            Range {
                start: Position {
                    line: 162,
                    character: 7,
                },
                end: Position {
                    line: 162,
                    character: 18,
                },
            },
        ];

        let sync = FindMacroReferencesResult {
            uri: to_file_uri("tests/samples/a/a.cmm"),
            references: references.clone(),
            subscripts: vec![to_file_uri("test/samples/b/b.cmm")],
        };

        let mut ongoing = vec![Some(OngoingTask::FindMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: progress.clone(),
            origin: origin.clone(),
            phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                visited: Vec::new(),
                results: FileLocationMap::new(),
                undone: Vec::new(),
            },
        })];

        let results: FileLocationMap = {
            let mut locations = FileLocationMap::new();
            for loc in references {
                locations.insert(&sync.uri, loc);
            }
            locations
        };
        progress.set_cycles(1);

        recv_find_subscript_macro_references_sync(&id, sync, &mut ongoing);

        let ongoing = ongoing[0].take();
        assert!(ongoing.as_ref().is_some_and(|t| *t
            == OngoingTask::FindMacroReferences {
                id,
                onset,
                progress,
                origin,
                phase: FindMacroReferencesPhase::ReferencesInSubscripts {
                    visited: Vec::new(),
                    results,
                    undone: vec![to_file_uri("test/samples/b/b.cmm")]
                },
            }));

        assert!(ongoing.is_some_and(|t| {
            let OngoingTask::FindMacroReferences { progress, .. } = t else {
                panic!()
            };
            progress.ready()
        }));
    }

    #[test]
    fn can_process_find_refs_result_for_file_target() {
        let files = files();
        let file_idx = create_file_idx();
        let docs = create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(1);
        let onset = Instant::now();

        let mut ts = Tasks {
            runner: TaskSystem::build(),
            blocked: Vec::new(),
            ongoing: vec![Some(OngoingTask::FindReferences(id.clone(), onset.clone()))],
            completed: Vec::new(),
            counter: TaskCounterInternal::new(),
        };

        let find_refs_res = Some(FindReferencesResult::Partial(
            FindReferencesPartialResult::FileTarget(to_file_uri("tests/samples/b/b.cmm")),
        ));

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let result = process_find_references_result(
            &docs,
            &file_idx,
            &id,
            find_refs_res,
            TraceValue::Off,
            &mut ts,
            &mut outgoing,
        );

        assert!(result.is_some_and(|r| r
            == FindReferencesResponse {
                id,
                result: Some(vec![Location {
                    uri: to_file_uri("tests/samples/a/a.cmm"),
                    range: Range {
                        start: Position {
                            line: 49,
                            character: 3,
                        },
                        end: Position {
                            line: 49,
                            character: 13,
                        },
                    },
                },]),
            }));
    }
}
