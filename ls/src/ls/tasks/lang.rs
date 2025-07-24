// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    ls::{
        doc::TextDocs,
        language::{ExtMacroDefOrigin, GotoDefinitionResult},
        response::{GoToDefinitionResponse, LocationResult},
        tasks::{ExtMacroDefOperations, OngoingTask, Tasks, find_ongoing_task_by_id},
    },
    protocol::{LocationLink, NumberOrString, Uri},
};

const ITERATIONS_MACRO_DEF: u32 = 3;

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

            if links.is_empty() {
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

pub fn process_goto_external_macro_def_sync(
    id: NumberOrString,
    script: Uri,
    defs: Option<GotoDefinitionResult>,
    mut callers: Vec<Uri>,
    ongoing: &mut Vec<OngoingTask>,
) {
    let idx = find_ongoing_task_by_id(&id, ongoing);
    let OngoingTask::GoToExternalMacroDefinition {
        completed,
        total,
        depth,
        preliminary,
        ops,
        ..
    } = &mut ongoing[idx]
    else {
        unreachable!("No other type possible.");
    };

    debug_assert!(
        ops.is_none() || ops.as_ref().unwrap().scripts.len() == ops.as_ref().unwrap().callees.len()
    );

    if let Some(GotoDefinitionResult::PartialMacro(..)) = defs
        && !callers.is_empty()
    {
        match ops {
            Some(operations) => {
                callers
                    .iter()
                    .for_each(|_| operations.callees.push(script.clone()));
                operations.scripts.append(&mut callers);

                debug_assert_eq!(operations.scripts.len(), operations.callees.len());
            }
            None => {
                let mut callees: Vec<Uri> = Vec::with_capacity(callers.len());
                callers.iter().for_each(|_| callees.push(script.clone()));

                *ops = Some(ExtMacroDefOperations {
                    callees,
                    scripts: callers,
                })
            }
        }
    }
    debug_assert!(
        ops.is_none() || ops.as_ref().unwrap().scripts.len() == ops.as_ref().unwrap().callees.len()
    );

    if let Some(res) = defs {
        match res {
            GotoDefinitionResult::Final(mut loc)
            | GotoDefinitionResult::PartialMacro(_, _, _, mut loc) => {
                preliminary.append(&mut loc);
            }
        }
    }

    *completed += 1;
    if completed >= total {
        *depth += 1;
        *completed = 0;

        if *depth >= ITERATIONS_MACRO_DEF || ops.is_none() {
            *total = 0;
        }
    }
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
    let OngoingTask::GoToDefinitionExtMeta(_, onset) = &ongoing[idx] else {
        unreachable!("No other type possible.");
    };

    let task = OngoingTask::GoToExternalMacroDefinition {
        id,
        completed: 0,
        total: num as u32,
        depth: 0,
        onset: onset.clone(),
        origin,
        preliminary: defs,
        ops: Some(ExtMacroDefOperations { scripts, callees }),
    };
    ongoing.push(task);
}
