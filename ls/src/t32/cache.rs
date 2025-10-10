// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use crate::t32::{CallExpression, MacroDefinitions, MacroScope, Subroutine};

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
    if let Some(macros) = &macros.privates {
        for def in macros {
            if def.r#macro.contains(&range.start) {
                return Some(MacroScope::Private);
            }
        }
    }

    if let Some(macros) = &macros.locals {
        for def in macros {
            if def.r#macro.contains(&range.start) {
                return Some(MacroScope::Local);
            }
        }
    }

    if let Some(macros) = &macros.globals {
        for def in macros {
            if def.r#macro.contains(&range.start) {
                return Some(MacroScope::Global);
            }
        }
    }
    None
}
