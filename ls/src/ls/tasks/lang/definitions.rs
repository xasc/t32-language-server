// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use serde_json::json;

use crate::{
    ls::{
        ReturnCode,
        doc::{TextDocData, TextDocs},
        language::{
            GotoDefinitionResult, MacroReferenceOrigin, find_definition,
            find_external_macro_definition, find_global_macro_definitions,
        },
        lsp::Message,
        request::Notification,
        response::{GoToDefinitionResponse, LocationResult, NullResponse, Response},
        tasks::{
            ExtMacroDefLookups, FileCallMap, OngoingTask, Task, TaskDone, TaskProgress, Tasks,
            find_ongoing_task_by_id, trace_doc_unknown, try_schedule,
        },
    },
    protocol::{DefinitionParams, LocationLink, LogTraceParams, NumberOrString, TraceValue, Uri},
    t32::GotoDefLangContext,
};

pub fn process_goto_definition_req(
    id: NumberOrString,
    params: DefinitionParams,
    trace_level: TraceValue,
    docs: &mut TextDocs,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Result<(), ReturnCode> {
    if trace_level != TraceValue::Off {
        outgoing.push(Some(log_find_defs_req(id.clone())));
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
    trace_level: TraceValue,
    ts: &mut Tasks,
    outgoing: &mut Vec<Option<Message>>,
) -> Option<GoToDefinitionResponse> {
    let result = match goto_def {
        Some(GotoDefinitionResult::Final(links)) => Some(LocationResult::ExtMeta(links)),
        Some(GotoDefinitionResult::PartialMacro(uri, r#macro, origin, links)) => {
            if let Some(callers) = docs.get_callers(&uri) {
                if trace_level != TraceValue::Off {
                    outgoing.push(Some(log_conv_goto_macro_ref_req(
                        id.clone(),
                        r#macro.clone(),
                    )));
                }
                // Queues the follow-up task for definitions in external files
                prepare_goto_external_macro_def_req(
                    id.clone(),
                    MacroReferenceOrigin {
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

            if links.is_empty() {
                None
            } else {
                Some(LocationResult::ExtMeta(links))
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
    ongoing: &mut Vec<Option<OngoingTask>>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);

    let Some(OngoingTask::GoToExternalMacroDef {
        progress,
        results,
        undone,
        ..
    }) = &mut ongoing[idx]
    else {
        unreachable!("Must not retrieve any other variant.");
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
    task: &mut Option<OngoingTask>,
    outgoing: &mut Vec<Task>,
    done: &mut Vec<Option<TaskDone>>,
) -> Result<(), ReturnCode> {
    let Some(OngoingTask::GoToExternalMacroDef {
        id,
        progress,
        origin,
        results,
        ..
    }) = task
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

        let mut links: Vec<LocationLink> = Vec::new();
        links.append(results);

        done.push(Some(TaskDone::GoToExternalMacroDef(id.clone(), links)));
    } else if progress.ready() {
        next_lookups_goto_external_macro_def(docs, task.as_mut().unwrap(), outgoing);
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
        &MacroReferenceOrigin,
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
        _ => unreachable!("Must not be called with any other variant."),
    };
    progress.total =
        queue_goto_external_macro_definitions(id, docs, origin, undone, visited, outgoing);
    progress.ack_ready();
}

fn prepare_goto_external_macro_def_req(
    id: NumberOrString,
    origin: MacroReferenceOrigin,
    defs: Vec<LocationLink>,
    callers: Vec<Uri>,
    ongoing: &mut Vec<Option<OngoingTask>>,
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
    let Some(OngoingTask::GoToDefinition(_, onset)) = ongoing[idx].take() else {
        unreachable!("Must not retrieve any other variant.");
    };

    ongoing[idx] = Some(OngoingTask::GoToExternalMacroDef {
        id,
        onset,
        progress: TaskProgress::new(num as u32),
        origin,
        visited: FileCallMap::new(),
        results: defs,
        undone: ExtMacroDefLookups {
            files: scripts,
            callees,
        },
    });
}

fn queue_goto_external_macro_definitions(
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

        outgoing.push(Task::GoToExternalMacroDef {
            id: id.clone(),
            textdoc: TextDocData {
                doc: doc.clone(),
                tree: tree.clone(),
            },
            t32: GotoDefLangContext::from(t32.clone()),
            callers: callers,
            origin: origin.clone(),
            backtrace: callee.clone(),
            find: find_external_macro_definition,
        });
        visited.insert(script.clone(), callee.clone());
    }
    undone.clear();

    total
}

fn log_find_defs_req(id: NumberOrString) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Received find definitions request with ID \"{:}\".",
                id
            ),
            verbose: None,
        },
    })
}

fn log_conv_goto_macro_ref_req(id: NumberOrString, r#macro: String) -> Message {
    Message::Notification(Notification::LogTraceNotification {
        params: LogTraceParams {
            message: format!(
                "INFO: Request with ID \"{:}\" converted to goto macro definition request.",
                id
            ),
            verbose: Some(json!(r#macro).to_string()),
        },
    })
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

        let mut task = Some(OngoingTask::GoToExternalMacroDef {
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
        });

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
