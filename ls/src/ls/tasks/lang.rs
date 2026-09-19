// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

mod definitions;
mod folds;
mod references;
mod semantic;

pub use definitions::GotoDefinitionResult;
pub use definitions::MacroReferenceOrigin;
pub use definitions::process_goto_definition_req;
pub use definitions::process_goto_definition_result;
pub use definitions::progress_goto_external_macro_def;
pub use definitions::recv_goto_external_macro_def_sync;
pub use folds::process_folding_range_req;
pub use references::FindDefintionsForMacroRefResult;
pub use references::FindMacroReferencesResult;
pub use references::FindReferencesResult;
pub use references::MacroDefinitionLocation;
pub use references::process_find_references_req;
pub use references::process_find_references_result;
pub use references::progress_find_external_macro_definitions;
pub use references::progress_find_macro_def_references;
pub use references::progress_find_subscript_macro_refs;
pub use references::recv_find_external_definitions_for_macro_reference_sync;
pub use references::recv_find_macro_def_references_sync;
pub use references::recv_find_subscript_macro_references_sync;
pub use semantic::process_semantic_tokens_full_req;
pub use semantic::process_semantic_tokens_range_req;
