// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use serde::Serialize;

use serde_json::json;

use crate::{
    ls::{
        ReturnCode,
        doc::{GlobalMacroDefIndex, TextDocData, TextDocs},
        lsp::Message,
        request::Notification,
        response::{GoToDefinitionResponse, LocationResult, NullResponse, Response},
        tasks::{
            ExtMacroDefLookups, FileCallMap, OngoingTask, Task, TaskDone, TaskProgress, Tasks,
            find_ongoing_task_by_id, trace_doc_unknown, try_schedule,
        },
    },
    protocol::{
        DefinitionParams, LocationLink, LogTraceParams, NumberOrString, Position, Range as LRange,
        TraceValue, Uri,
    },
    t32::{self, GotoDefLangContext, MacroDefinitionResult, NodeKind, Subroutine},
    utils::BRange,
};

// TODO: Use dedicated types for GoToExternalMacroDef results. The first two
// elements do not have to be repeated in this case.
#[derive(Debug, PartialEq)]
pub enum GotoDefinitionResult {
    Final(Vec<LocationLink>),
    PartialMacro(Uri, String, LRange, Vec<LocationLink>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MacroReferenceOrigin {
    pub name: String,
    pub span: LRange,
    pub uri: Uri,
}

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
    let idx = find_ongoing_task_by_id(&id, ongoing).expect("Must be a registered task.");

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
        progress.mark_completed();
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

    let idx = find_ongoing_task_by_id(&id, &ongoing).expect("Must be a registered task.");
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

/// Retrieves definitions for `(macro)`, `(subroutine_call_expression)`, and
/// `(command_expression)` nodes. `(command_expression)` nodes capture
/// `DO` and `RUN` commands for subscripts calls.
///   - Macros may have multiple definitions in other files due to the
///     `LOCAL` keyword. `GLOBAL` macro definitions are ignored.
///   - Subscript calls return the start of the script file.
///   - Subroutine definitions are limited to the current file.
///
/// TODO: Add detection for macros that are nested inside HLL expressions. In
///       this case the macro are detected as HLL entities.
fn find_definition(
    textdoc: TextDocData,
    t32: GotoDefLangContext,
    position: Position,
) -> Option<GotoDefinitionResult> {
    let offset = textdoc.doc.to_byte_offset(&position);

    let lang = textdoc.tree.language();
    let allowed_kinds = t32::get_goto_def_ids(&lang);

    let root = textdoc.tree.walk();
    let origin = t32::find_deepest_node(root, offset, &allowed_kinds)?;

    let node = origin.node();
    let origin_span = node.range();

    match t32::id_into_node(&lang, origin.node().kind_id()) {
        NodeKind::Macro => {
            match t32::goto_infile_macro_definition(&textdoc.doc.text, &textdoc.tree, &t32, origin)?
            {
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
            let uri = t32::goto_file(&textdoc.doc.text, &t32.calls.scripts?, origin)?;

            // Point to start of called script file
            Some(GotoDefinitionResult::Final(vec![LocationLink::build(
                &textdoc.doc,
                Some(BRange::from(origin_span.start_byte..origin_span.end_byte)),
                uri,
                BRange::from(0..1),
                BRange::from(0..1),
            )]))
        }
        // TODO: `GOSUB &macro` must look for macro definition instead of subroutine.
        NodeKind::SubroutineCallExpression => {
            let sub: Subroutine =
                t32::goto_subroutine_definition(&textdoc.doc.text, &t32.subroutines, origin)?;
            let (target_range, target_sel) = if let Some(docstring) = sub.docstring {
                (
                    BRange::from(docstring.inner().start..sub.definition.inner().end),
                    sub.name.clone(),
                )
            } else {
                (sub.definition.clone(), sub.name.clone())
            };

            Some(GotoDefinitionResult::Final(vec![LocationLink::build(
                &textdoc.doc,
                Some(BRange::from(origin_span.start_byte..origin_span.end_byte)),
                textdoc.doc.uri.clone(),
                target_range,
                target_sel,
            )]))
        }
        _ => None,
    }
}

fn find_external_macro_definition(
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

    let targets: Vec<BRange> = t32::locate_calls_to_file_target(subscripts, &backtrace);
    if targets.is_empty() {
        return (None, callers);
    }

    // TODO: `RUN` clears the PRACTICE stack, so it cannot propagate
    // `LOCAL` macros.
    // TODO: Add support for `GOTO` → `(label)` transitions.
    match t32::goto_external_macro_definition(
        &textdoc.doc.text,
        &textdoc.tree,
        &t32,
        &r#macro,
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

fn find_global_macro_definitions(
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

    use std::{env, path, time::Instant};

    use tree_sitter::Tree;

    use url::Url;

    use crate::{
        config::T32DefaultDirs,
        ls::doc::{self, TextDoc},
        ls::{Task, TaskCounters, TaskSystem, workspace},
        protocol::{Position, Range as LRange},
        t32::LangExpressions,
        utils,
    };

    fn find_def(file: &str, position: Position) -> Option<GotoDefinitionResult> {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();

        let file_idx = workspace::index_files(utils::files());
        let dirs = T32DefaultDirs::default();

        let (doc, tree, t32) = doc::read_doc(uri, &file_idx, &dirs).unwrap();

        find_definition(
            TextDocData { doc, tree },
            GotoDefLangContext::from(t32),
            position,
        )
    }

    #[test]
    fn can_process_goto_definition_result() {
        let files = utils::files();
        let file_idx = utils::create_file_idx();
        let docs = utils::create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(2);
        let onset = Instant::now();

        let mut ts = Tasks {
            runner: TaskSystem::build(),
            blocked: Vec::new(),
            ongoing: vec![Some(OngoingTask::GoToDefinition(id.clone(), onset.clone()))],
            completed: Vec::new(),
            counters: TaskCounters::new(),
        };

        let uri_a = utils::to_file_uri("tests/samples/a/a.cmm");
        let uri_c = utils::to_file_uri("tests/samples/c.cmm");
        let uri_d = utils::to_file_uri("tests/samples/a/d/d.cmm");

        let goto_def_result = Some(GotoDefinitionResult::PartialMacro(
            uri_a.clone(),
            "&from_c_cmm".to_string(),
            LRange {
                start: Position {
                    line: 139,
                    character: 7,
                },
                end: Position {
                    line: 139,
                    character: 18,
                },
            },
            Vec::new(),
        ));

        let mut outgoing: Vec<Option<Message>> = Vec::new();

        let origin = MacroReferenceOrigin {
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
            uri: uri_a.clone(),
        };

        let result = process_goto_definition_result(
            &docs,
            &id,
            goto_def_result,
            TraceValue::Off,
            &mut ts,
            &mut outgoing,
        );

        assert!(result.is_none());
        assert!(outgoing.is_empty());

        assert!(ts.ongoing[0].as_ref().is_some_and(|t| {
            let OngoingTask::GoToExternalMacroDef { progress, .. } = t else {
                unreachable!()
            };
            progress.ready()
        }));

        assert!(ts.ongoing[0].take().is_some_and(|t| t
            == OngoingTask::GoToExternalMacroDef {
                id,
                onset,
                progress: TaskProgress::new(2),
                origin,
                visited: FileCallMap::new(),
                undone: ExtMacroDefLookups {
                    files: vec![uri_c.clone(), uri_d.clone()],
                    callees: vec![uri_a.clone(), uri_a.clone()],
                },
                results: Vec::new(),
            }));
    }

    #[test]
    fn can_queue_lookups_for_external_macro_definitions() {
        let files = utils::files();
        let file_idx = utils::create_file_idx();
        let docs = utils::create_doc_store(&files, &file_idx);

        let id = NumberOrString::Number(2);
        let onset = Instant::now();

        let uri_c = utils::to_file_uri("tests/samples/c.cmm");
        let uri_d = utils::to_file_uri("tests/samples/a/d/d.cmm");

        let origin = MacroReferenceOrigin {
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
            uri: utils::to_file_uri("tests/samples/a/a.cmm"),
        };

        let mut ongoing = OngoingTask::GoToExternalMacroDef {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(10),
            origin: origin.clone(),
            visited: FileCallMap::new(),
            undone: ExtMacroDefLookups {
                files: vec![uri_c.clone(), uri_d.clone()],
                callees: vec![
                    utils::to_file_uri("tests/samples/a/a.cmm"),
                    utils::to_file_uri("tests/samples/a/a.cmm"),
                ],
            },
            results: Vec::new(),
        };

        let visited: FileCallMap = {
            let mut map = FileCallMap::new();

            for file in [uri_c.clone(), uri_d.clone()] {
                map.insert(file, utils::to_file_uri("tests/samples/a/a.cmm"));
            }
            map
        };

        let mut progress = TaskProgress::new(2);
        progress.ack_ready();

        let mut outgoing: Vec<Task> = Vec::new();

        next_lookups_goto_external_macro_def(&docs, &mut ongoing, &mut outgoing);

        assert!(
            ongoing
                == OngoingTask::GoToExternalMacroDef {
                    id: id,
                    onset,
                    progress,
                    origin,
                    visited,
                    undone: ExtMacroDefLookups::new(),
                    results: Vec::new(),
                }
        );

        assert!(outgoing.len() == 2);
        assert!(outgoing.iter().any(|o| {
            let Task::GoToExternalMacroDef { textdoc, .. } = o else {
                return false;
            };
            textdoc.doc.uri == uri_c
        }));
        assert!(outgoing.iter().any(|o| {
            let Task::GoToExternalMacroDef { textdoc, .. } = o else {
                return false;
            };
            textdoc.doc.uri == uri_d
        }));
    }

    #[test]
    fn can_progress_goto_external_macro_definition_req() {
        let id = NumberOrString::Number(2);
        let onset = Instant::now();

        let uri_c = utils::to_file_uri("tests/samples/c.cmm");
        let uri_d = utils::to_file_uri("tests/samples/a/d/d.cmm");

        let origin = MacroReferenceOrigin {
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
            uri: utils::to_file_uri("tests/samples/a/a.cmm"),
        };

        let sync_goto_ext_def = Some(GotoDefinitionResult::PartialMacro(
            uri_d.clone(),
            "&from_c_cmm".to_string(),
            origin.span.clone(),
            Vec::new(),
        ));

        let visited: FileCallMap = {
            let mut map = FileCallMap::new();

            map.insert(uri_d.clone(), uri_c.clone());
            map
        };

        let mut ongoing = vec![Some(OngoingTask::GoToExternalMacroDef {
            id: id.clone(),
            onset: onset.clone(),
            progress: TaskProgress::new(2),
            origin: origin.clone(),
            visited: visited.clone(),
            undone: ExtMacroDefLookups::new(),
            results: Vec::new(),
        })];

        let mut progress = TaskProgress::new(2);
        progress.advance();

        recv_goto_external_macro_def_sync(
            &id,
            &uri_d,
            sync_goto_ext_def,
            vec![uri_c.clone()],
            &mut ongoing,
        );

        let ongoing = ongoing[0].take();
        assert!(ongoing.as_ref().is_some_and(|t| *t
            == OngoingTask::GoToExternalMacroDef {
                id,
                onset,
                progress,
                origin,
                visited,
                results: Vec::new(),
                undone: ExtMacroDefLookups {
                    files: vec![uri_c],
                    callees: vec![uri_d],
                },
            }));

        assert!(ongoing.is_some_and(|t| {
            let OngoingTask::GoToExternalMacroDef { progress, .. } = t else {
                panic!()
            };
            !progress.ready()
        }));
    }

    #[test]
    fn visits_scripts_for_external_macro_definition_lookup_only_once() {
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
                span: LRange {
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
        let file_idx = utils::create_file_idx();
        let dirs = T32DefaultDirs::default();

        let files = utils::files();
        let mut members: Vec<(TextDoc, Tree, LangExpressions)> = Vec::new();

        for uri in files {
            let (doc, tree, expr) =
                doc::read_doc(uri.clone(), &file_idx, &dirs).expect("Must not fail.");
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

        let _uri = utils::to_file_uri("tests/samples/a/a.cmm");

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

    fn find_external_macro_def(
        file: &str,
        callers: Vec<Uri>,
        origin: MacroReferenceOrigin,
        backtrace: Uri,
    ) -> (Option<GotoDefinitionResult>, Vec<Uri>) {
        let uri = Url::from_file_path(path::absolute(file).expect("File must exist.")).unwrap();

        let file_idx = workspace::index_files(utils::files());
        let dirs = T32DefaultDirs::default();

        let (doc, tree, t32) = doc::read_doc(uri, &file_idx, &dirs).unwrap();

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
}
