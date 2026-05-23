// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::protocol::{SemanticTokenModifiers, SemanticTokenTypes, SemanticTokensLegend};

impl SemanticTokensLegend {
    pub fn new() -> Self {
        Self {
            token_types: Vec::new(),
            token_modifiers: Vec::new(),
        }
    }

    pub fn from_attrs(
        mut token_types: Vec<SemanticTokenTypes>,
        mut token_modifiers: Vec<SemanticTokenModifiers>,
    ) -> Self {
        token_types
            .retain(|t| *t < SemanticTokenTypes::Namespace || *t == SemanticTokenTypes::Enum);
        token_modifiers.retain(|m| {
            *m == SemanticTokenModifiers::Definition
                || *m == SemanticTokenModifiers::DefaultLibrary
                || *m == SemanticTokenModifiers::Abstract
                || *m == SemanticTokenModifiers::Modification
                || *m == SemanticTokenModifiers::Static
        });

        Self {
            token_types,
            token_modifiers,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.token_types.is_empty()
    }
}
