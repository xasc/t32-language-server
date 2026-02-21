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
        token_types.retain(|t| *t < SemanticTokenTypes::Namespace);
        token_modifiers.retain(|m| *m < SemanticTokenModifiers::Definition);

        Self {
            token_types,
            token_modifiers,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.token_types.is_empty()
    }
}

impl SemanticTokenTypes {
    pub const fn num() -> usize {
        [
            Self::Namespace,
            Self::Type,
            Self::Class,
            Self::Enum,
            Self::Interface,
            Self::Struct,
            Self::TypeParameter,
            Self::Parameter,
            Self::Variable,
            Self::Property,
            Self::EnumMember,
            Self::Event,
            Self::Function,
            Self::Method,
            Self::Macro,
            Self::Keyword,
            Self::Modifier,
            Self::Comment,
            Self::String,
            Self::Number,
            Self::Regexp,
            Self::Operator,
            Self::Decorator,
            Self::Label,
        ]
        .len()
    }
}

impl SemanticTokenModifiers {
    pub const fn num() -> usize {
        [
            Self::Declaration,
            Self::Definition,
            Self::Readonly,
            Self::Static,
            Self::Deprecated,
            Self::Abstract,
            Self::Async,
            Self::Modification,
            Self::Documentation,
            Self::DefaultLibrary,
        ]
        .len()
    }
}
