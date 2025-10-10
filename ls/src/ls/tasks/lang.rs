// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use crate::{
    ls::{
        ReturnCode,
        doc::{TextDocData, TextDocs},
        language::{
            ExtMacroDefOrigin, FileLocation, FindMacroReferencesResult,
            FindReferencesPartialResult, FindReferencesResult, GotoDefinitionResult,
            find_definition, find_external_macro_definition, find_global_macro_definitions,
            find_macro_references, find_references,
        },
        lsp::Message,
        response::{
            FindReferencesResponse, GoToDefinitionResponse, LocationResult, NullResponse, Response,
        },
        tasks::{
            ExtMacroDefLookups, FileCallMap, FileLocationMap, OngoingTask, Task, TaskDone,
            TaskProgress, Tasks, find_ongoing_task_by_id, trace_doc_unknown, try_schedule,
        },
    },
    protocol::{DefinitionParams, LocationLink, NumberOrString, ReferenceParams, TraceValue, Uri},
    t32::{FindMacroRefsLangContext, FindRefsLangContext, GotoDefLangContext, MacroScope},
};

pub fn process_goto_definition_req(
    id: NumberOrString,
    params: DefinitionParams,
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
        Task::GoToDefinition(
            id,
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            GotoDefLangContext::from(t32.clone()),
            params.position,
            find_definition,
        ),
        &mut ts.ongoing,
        &mut ts.blocked,
    )?;
    Ok(())
}

pub fn process_goto_definition_result(
    docs: &TextDocs,
    id: &NumberOrString,
    goto_def: Option<GotoDefinitionResult>,
    ts: &mut Tasks,
) -> Option<GoToDefinitionResponse> {
    let result = match goto_def {
        Some(GotoDefinitionResult::Final(links)) => Some(LocationResult::ExtMeta(links)),
        Some(GotoDefinitionResult::PartialMacro(uri, r#macro, origin, links)) => {
            if let Some(callers) = docs.get_callers(&uri) {
                // Queues the follow-up task for definitions in external files
                goto_external_macro_def(
                    id.clone(),
                    ExtMacroDefOrigin {
                        name: r#macro,
                        span: origin,
                        uri,
                    },
                    links,
                    callers.clone(),
                    &mut ts.ongoing,
                );
                return None;
            }

            if !links.is_empty() {
                Some(LocationResult::ExtMeta(links))
            } else {
                None
            }
        }
        None => None,
    };

    Some(GoToDefinitionResponse {
        id: id.clone(),
        result,
    })
}

pub fn recv_goto_external_macro_def_sync(
    id: &NumberOrString,
    script: &Uri,
    sync: Option<GotoDefinitionResult>,
    mut callers: Vec<Uri>,
    ongoing: &mut Vec<OngoingTask>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);
    let OngoingTask::GoToExternalMacroDef {
        progress,
        results,
        undone,
        ..
    } = &mut ongoing[idx]
    else {
        unreachable!("No other type possible.");
    };

    debug_assert_eq!(undone.files.len(), undone.callees.len());

    if let Some(GotoDefinitionResult::PartialMacro(..)) = sync
        && !callers.is_empty()
    {
        callers
            .iter()
            .for_each(|_| undone.callees.push(script.clone()));
        undone.files.append(&mut callers);

        debug_assert_eq!(undone.files.len(), undone.callees.len());
    }
    debug_assert_eq!(undone.files.len(), undone.callees.len());

    if let Some(res) = sync {
        match res {
            GotoDefinitionResult::Final(mut loc)
            | GotoDefinitionResult::PartialMacro(_, _, _, mut loc) => {
                loc.retain(|l| !results.contains(l));
                results.append(&mut loc);
            }
        }
    }

    progress.advance();
    if progress.ready() && undone.is_empty() {
        progress.abort();
    }
}

pub fn progress_goto_external_macro_def(
    docs: &TextDocs,
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
    done: &mut Vec<Option<TaskDone>>,
) -> Result<(), ReturnCode> {
    let OngoingTask::GoToExternalMacroDef {
        id,
        progress,
        origin,
        undone,
        results,
        ..
    } = task
    else {
        unreachable!("No other type is possible.");
    };

    if progress.finished() {
        let globals = docs.get_all_global_macros();
        if globals.is_some() {
            results.append(&mut find_global_macro_definitions(
                docs,
                globals.unwrap(),
                origin.clone(),
            ));
        }

        done.push(Some(TaskDone::GoToExternalMacroDef(
            id.clone(),
            results.clone(),
        )));
    } else if progress.ready() && !undone.is_empty() {
        next_lookups_goto_external_macro_def(docs, task, outgoing);
    }
    Ok(())
}

fn next_lookups_goto_external_macro_def(
    docs: &TextDocs,
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
) {
    let (id, origin, progress, undone, visited): (
        &NumberOrString,
        &ExtMacroDefOrigin,
        &mut TaskProgress,
        &mut ExtMacroDefLookups,
        &mut FileCallMap,
    ) = match task {
        OngoingTask::GoToExternalMacroDef {
            id,
            progress,
            origin,
            undone,
            visited,
            ..
        } => {
            if undone.is_empty() {
                return;
            }
            (id, origin, progress, undone, visited)
        }
        _ => unreachable!("No other type supported."),
    };

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

        outgoing.push(Task::GoToExternalMacroDef {
            id: id.clone(),
            textdoc: TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            t32: GotoDefLangContext::from(t32.clone()),
            callers: callers,
            origin: ExtMacroDefOrigin {
                uri: callee.clone(),
                ..origin.clone()
            },
            find: find_external_macro_definition,
        });
        visited.insert(script.clone(), callee.clone());
    }
    progress.total = total;
    undone.clear();
}

fn goto_external_macro_def(
    id: NumberOrString,
    origin: ExtMacroDefOrigin,
    defs: Vec<LocationLink>,
    callers: Vec<Uri>,
    ongoing: &mut Vec<OngoingTask>,
) {
    debug_assert!(callers.len() > 0);
    let num = callers.len();

    let (mut scripts, mut callees): (Vec<Uri>, Vec<Uri>) =
        (Vec::with_capacity(num), Vec::with_capacity(num));

    for file in callers {
        scripts.push(file.clone());
        callees.push(origin.uri.clone());
    }

    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let OngoingTask::GoToDefinition(_, onset) = &ongoing[idx] else {
        unreachable!("No other type possible.");
    };

    let task = OngoingTask::GoToExternalMacroDef {
        id,
        onset: onset.clone(),
        progress: TaskProgress::new(num as u32),
        origin,
        visited: FileCallMap::new(),
        results: defs,
        undone: ExtMacroDefLookups {
            files: scripts,
            callees,
        },
    };
    ongoing.push(task);
}

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
                uri: _uri,
                r#macro,
                definitions,
            } => {
                debug_assert!(!definitions.is_empty());

                queue_find_macro_references_req(id.clone(), r#macro, definitions, &mut ts.ongoing);
                return None;
            }
            FindReferencesPartialResult::MacroDefsIncomplete {
                uri,
                r#macro,
                definitions,
            } => {
                if let Some(callers) = docs.get_callers(&uri) {
                    // Queues the follow-up task for definitions in external files.
                    find_external_definitions(
                        id.clone(),
                        uri,
                        r#macro,
                        definitions,
                        callers.clone(),
                        &mut ts.ongoing,
                    );
                    return None;
                }

                if !definitions.is_empty() {
                    // Queues the follow-up task for all references in the file.
                    find_definition_references(id.clone(), r#macro, definitions, &mut ts.ongoing);
                    return None;
                }
                // TODO: Find implicit definition & references in file.
                None
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
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    let OngoingTask::FindMacroDefinitionReferences {
        id,
        onset,
        progress,
        r#macro,
        undone,
        ..
    } = task
    else {
        unreachable!("No other variant is supported.");
    };

    if progress.finished() {
        *task = OngoingTask::FindSubscriptMacroReferences {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(undone.len() as u32),
            r#macro: r#macro.clone(),
        };
    } else if progress.ready() {
        next_lookups_find_macro_def_references(docs, task, outgoing)?;
    }
    Ok(())
}

pub fn recv_find_macro_def_references_sync(
    id: &NumberOrString,
    sync: FindMacroReferencesResult,
    ongoing: &mut Vec<OngoingTask>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);
    let OngoingTask::FindMacroDefinitionReferences {
        progress,
        results,
        undone,
        ..
    } = &mut ongoing[idx]
    else {
        unreachable!("No other type possible.");
    };

    let FindMacroReferencesResult {
        uri,
        references,
        callees,
    } = sync;
    for r#ref in references {
        results.insert(&uri, r#ref);
    }

    for callee in callees {
        if !undone.contains(&callee) {
            undone.push(callee);
        }
    }
    progress.advance();
}

fn find_external_definitions(
    id: NumberOrString,
    uri: Uri,
    _macro: String,
    _definitions: Vec<(FileLocation, Option<MacroScope>)>,
    callers: Vec<Uri>,
    ongoing: &mut Vec<OngoingTask>,
) {
    debug_assert!(callers.len() > 0);
    let num = callers.len();

    let (mut scripts, mut callees): (Vec<Uri>, Vec<Uri>) =
        (Vec::with_capacity(num), Vec::with_capacity(num));

    for file in callers {
        scripts.push(file.clone());
        callees.push(uri.clone());
    }

    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let OngoingTask::FindReferences(_, _onset) = &ongoing[idx] else {
        unreachable!("No other type possible.");
    };

    todo!()
}

fn queue_find_macro_references_req(
    id: NumberOrString,
    r#macro: String,
    definitions: Vec<(FileLocation, Option<MacroScope>)>,
    ongoing: &mut Vec<OngoingTask>,
) {
    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let OngoingTask::FindReferences(_, onset) = &ongoing[idx] else {
        unreachable!("No other type possible.");
    };

    let task = OngoingTask::FindMacroDefinitionReferences {
        id,
        onset: onset.clone(),
        progress: TaskProgress::new(definitions.len() as u32),
        r#macro,
        definitions,
        results: FileLocationMap::new(),
        undone: Vec::new(),
    };
    ongoing.push(task);
}

fn find_definition_references(
    id: NumberOrString,
    _macro: String,
    _definitions: Vec<(FileLocation, Option<MacroScope>)>,
    ongoing: &mut Vec<OngoingTask>,
) {
    let idx = find_ongoing_task_by_id(&id, &ongoing);
    let OngoingTask::FindReferences(_, _onset) = &ongoing[idx] else {
        unreachable!("No other type possible.");
    };

    todo!()
}

fn next_lookups_find_macro_def_references(
    docs: &TextDocs,
    task: &mut OngoingTask,
    outgoing: &mut Vec<Task>,
) -> Result<(), ReturnCode> {
    let OngoingTask::FindMacroDefinitionReferences {
        id,
        onset: _onset,
        progress,
        r#macro,
        definitions,
        ..
    } = task
    else {
        unreachable!("No other type supported.");
    };

    let mut touched: Vec<u8> = vec![0; definitions.len()];
    let mut total: u32 = 0;

    // Group all lookups from the same file into a single request.
    for (ii, (FileLocation { uri, range }, scope)) in definitions.iter().enumerate() {
        if touched[ii] > 0 {
            continue;
        }
        let mut defs: Vec<(Range<usize>, Option<MacroScope>)> =
            vec![(range.clone(), scope.clone())];

        for (jj, (FileLocation { uri: file, range }, scope)) in
            definitions[ii + 1..].iter().enumerate()
        {
            if touched[jj] > 0 || file != uri {
                continue;
            }
            defs.push((range.clone(), scope.clone()));
            touched[jj] = 1;
        }

        let (doc, tree, t32) = docs
            .get_doc_data(uri)
            .expect("File must be known at this point.");

        outgoing.push(Task::FindMacroDefinitionReferences(
            id.clone(),
            TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            FindMacroRefsLangContext::from(t32.clone()),
            r#macro.to_string(),
            defs,
            find_macro_references,
        ));

        touched[ii] = 1;
        total += 1;
    }
    progress.total = total;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Instant;

    use crate::protocol::{Position, Range};

    #[test]
    fn skips_redundant_external_macro_def_checks() {
        let docs = TextDocs::new();

        let mut visited = FileCallMap::new();
        visited.insert(
            "file:///sample/a.cmm".to_string(),
            "file:///sample/b.cmm".to_string(),
        );

        let mut task = OngoingTask::GoToExternalMacroDef {
            id: NumberOrString::Number(1),
            onset: Instant::now(),
            progress: TaskProgress {
                completed: 0,
                total: 3,
                cycles: 0,
                max_cycles: u32::MAX,
            },
            origin: ExtMacroDefOrigin {
                name: "test".to_string(),
                span: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
                uri: "file:///sample/test.cmm".to_string(),
            },
            visited,
            undone: ExtMacroDefLookups {
                files: vec!["file:///sample/a.cmm".to_string()],
                callees: vec!["file:///sample/b.cmm".to_string()],
            },
            results: Vec::new(),
        };

        let mut outgoing: Vec<Task> = Vec::new();
        let mut completed: Vec<Option<TaskDone>> = Vec::new();

        progress_goto_external_macro_def(&docs, &mut task, &mut outgoing, &mut completed).unwrap();

        assert!(outgoing.is_empty());
        assert!(completed.is_empty());

        progress_goto_external_macro_def(&docs, &mut task, &mut outgoing, &mut completed).unwrap();

        assert!(outgoing.is_empty());
        assert!(!completed.is_empty());
    }
}
