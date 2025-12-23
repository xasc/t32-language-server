// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

mod definitions;
mod references;

pub use definitions::{
    process_goto_definition_req, process_goto_definition_result, progress_goto_external_macro_def,
    recv_goto_external_macro_def_sync,
};
pub use references::{
    process_find_references_req, process_find_references_result,
    progress_find_external_macro_definitions, progress_find_macro_def_references,
    progress_find_subscript_macro_refs, recv_find_external_definitions_for_macro_reference_sync,
    recv_find_macro_def_references_sync, recv_find_subscript_macro_references_sync,
};
