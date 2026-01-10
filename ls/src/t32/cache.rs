// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use crate::{
    protocol::Uri,
    t32::{CallExpression, MacroDefinitions, MacroScope, Subroutine, SubscriptCalls},
    utils::BRange,
};

pub fn find_subroutine_for_call<'a>(
    text: &str,
    call: &CallExpression,
    subroutines: &'a [Subroutine],
) -> Option<&'a Subroutine> {
    if call.target.end >= text.len() {
        return None;
    }

    let name = &text[call.target.clone()];
    for subroutine in subroutines {
        if text[subroutine.name.clone()] == *name {
            return Some(subroutine);
        }
    }
    None
}

pub fn get_macro_scope(macros: &MacroDefinitions, range: &Range<usize>) -> Option<MacroScope> {
    for def in &macros.privates {
        if def.r#macro.contains(&range.start) {
            return Some(MacroScope::Private);
        }
    }

    for def in &macros.locals {
        if def.r#macro.contains(&range.start) {
            return Some(MacroScope::Local);
        }
    }

    for def in &macros.globals {
        if def.r#macro.contains(&range.start) {
            return Some(MacroScope::Global);
        }
    }
    None
}

pub fn locate_calls_to_file_target(calls: &SubscriptCalls, file: &Uri) -> Vec<BRange> {
    let mut targets: Vec<BRange> = Vec::with_capacity(1);

    for (_, loc) in calls
        .targets
        .iter()
        .zip(calls.locations.iter())
        .filter(|&(t, _)| t.is_some() && *t.as_ref().unwrap() == *file)
    {
        targets.push(BRange::from(loc.call.clone()));
    }
    targets
}
