// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Command Long and Short Forms
//! ===================================
//!
//! The same TRACE32 command or function may be executed by several equal
//! command strings. The long name of each command or functions can be
//! converted into a short form by removing all lower case letters from the
//! string.
//!
//! Usually, the short form may not be equal to the empty string. However,
//! there is two notable exception to this rule:
//!   1.  If the first part of a command only consists of lower case letters,
//!       the complete first part may be
//!       dropped.
//!
//!       Example:
//!
//!       ```ignore
//!         optional.CoMmanD → CoMmanD
//!      ```
//!
//!   2.  If the last part of a command only consists of lower case letters,
//!       its last part may be dropped.
//!
//!       Example:
//!
//!       ```ignore
//!         CoMmanD.optional → CoMmanD
//!      ```

use std::cell::RefCell;
use std::rc::Rc;

use t32_language_server_user_guide_data::PracticeFunctionDefinition;
use t32_language_server_user_guide_data::PracticeFunctionName;
use t32_language_server_user_guide_data::PracticeFunctionNamePattern;
use t32_language_server_user_guide_data::PracticeFunctionParameters;
use t32_language_server_user_guide_data::UserGuideData;
use t32_language_server_user_guide_data::UserGuideDocs;
use t32_language_server_user_guide_data::funcname;

#[derive(Clone, Debug)]
enum FunctionTrieNode {
    Internal {
        label: FunctionTrieNodeLabel,
        childs: Vec<Rc<RefCell<FunctionTrieNode>>>,
    },
    Leaf {
        label: FunctionTrieNodeLabel,
        value: FunctionNameSlot,
    },
    #[allow(unused)]
    Wildcard {
        label: FunctionTrieNodeLabel,
        childs: Vec<Rc<RefCell<FunctionTrieNode>>>,
    },
    Root {
        childs: Vec<Rc<RefCell<FunctionTrieNode>>>,
    },
}

pub struct BuiltinFunctions {
    trie: FunctionNameTrie,
    desc: Vec<FunctionDescription>,
    meta: Vec<FunctionMetaData>,
    copyrights: Vec<String>,
    docs: UserGuideDocs,
}

pub struct FunctionDescription {
    name: PracticeFunctionName,
    params: PracticeFunctionParameters,
    operation: String,
}

#[derive(Debug)]
pub struct TrieNodeInsertionPoint {
    node: Rc<RefCell<FunctionTrieNode>>,
    prefix_len: u32,
}

pub struct FunctionMetaData {
    copyright: u32,
    doc: u32,
}

pub struct FunctionMetaDescription<'a> {
    name: &'a PracticeFunctionName,
    params: &'a PracticeFunctionParameters,
    operation: &'a str,
    copyright: &'a str,
    doc: &'a str,
}

#[derive(Clone, Copy, Debug)]
struct FunctionNameSlot(u32);

#[derive(Debug)]
struct FunctionNameTrie {
    root: Rc<RefCell<FunctionTrieNode>>,
    data: String,
}

#[derive(Clone, Copy, Debug)]
struct FunctionTrieNodeLabel {
    border: u32,
    offset: u32,
    len: u32,
}

impl BuiltinFunctions {
    pub fn from_user_guide(ug: &UserGuideData) -> Self {
        let functions: &[PracticeFunctionDefinition] = &ug.functions;

        let mut desc: Vec<FunctionDescription> = Vec::with_capacity(ug.functions.len());
        let mut meta: Vec<FunctionMetaData> = Vec::with_capacity(ug.functions.len());

        let (copyrights, docs): (Vec<String>, UserGuideDocs) = {
            let mut copyrights: Vec<String> = Vec::with_capacity(ug.copyrights.len());

            for PracticeFunctionDefinition { copyright, .. } in functions {
                let cr = &ug.copyrights[*copyright as usize];
                if copyrights.contains(cr) {
                    continue;
                }
                copyrights.push(cr.clone());
            }
            copyrights.shrink_to_fit();

            let mut docs: UserGuideDocs = Vec::with_capacity(ug.docs.len());

            for PracticeFunctionDefinition { source, .. } in functions {
                let doc = &ug.docs[*source as usize];
                if docs.contains(doc) {
                    continue;
                }
                docs.push(doc.clone());
            }
            docs.shrink_to_fit();

            (copyrights, docs)
        };

        for PracticeFunctionDefinition {
            name,
            params,
            operation,
            copyright,
            source,
            ..
        } in functions
        {
            desc.push(FunctionDescription {
                name: name.clone(),
                params: params.to_owned(),
                operation: operation.to_owned(),
            });

            let copyright = copyrights
                .iter()
                .position(|c| *c == ug.copyrights[*copyright as usize])
                .expect("List must contain all copyright references.")
                as u32;

            let doc = docs
                .iter()
                .position(|d| *d == ug.docs[*source as usize])
                .expect("List must contain all source references.") as u32;

            meta.push(FunctionMetaData { copyright, doc })
        }
        debug_assert_eq!(desc.len(), meta.len());

        desc.shrink_to_fit();
        meta.shrink_to_fit();

        let mut names: Vec<PracticeFunctionName> = Vec::new();
        for PracticeFunctionDefinition { name, .. } in functions {
            names.append(&mut permutate_long_and_short_forms(name));
        }
        let trie = FunctionNameTrie::from_user_guide(&names);

        Self {
            trie: trie.expect("Embedded data must contain funtion information."),
            desc,
            meta,
            copyrights,
            docs,
        }
    }

    pub fn get<'a>(&'a self, name: &str) -> Option<FunctionMetaDescription<'a>> {
        if let Some(FunctionNameSlot(slot)) = self.trie.get(name) {
            let slot = slot as usize;

            Some(FunctionMetaDescription {
                name: &self.desc[slot].name,
                params: &self.desc[slot].params,
                operation: &self.desc[slot].operation,
                copyright: &self.copyrights[self.meta[slot].copyright as usize],
                doc: &self.docs[self.meta[slot].doc as usize],
            })
        } else {
            None
        }
    }
}

impl FunctionNameTrie {
    pub fn new(capacity: usize) -> Self {
        Self {
            root: Rc::new(RefCell::new(FunctionTrieNode::Root { childs: Vec::new() })),
            data: String::with_capacity(capacity),
        }
    }
    pub fn from_user_guide(names: &[PracticeFunctionName]) -> Option<Self> {
        if names.is_empty() {
            return None;
        }

        let total: usize = names
            .iter()
            .map(|n| n.strlen())
            .reduce(|sum, len| sum + len)
            .expect("Cannot be empty.");

        let mut trie = Self::new(total);

        for (ii, name) in names.iter().enumerate() {
            if name.has_wildcard() {
                // unimplemented!();
            } else {
                trie.insert_regular_function_name(name, FunctionNameSlot(ii as u32));
            }
        }
        Some(trie)
    }

    pub fn get(&self, name: &str) -> Option<FunctionNameSlot> {
        let mut offset: usize = 0;
        let mut node: Rc<RefCell<FunctionTrieNode>> = self.root.clone();

        loop {
            let mut next: Rc<RefCell<FunctionTrieNode>> = node.clone();
            let mut visit_child: bool = false;

            match &*node.borrow() {
                FunctionTrieNode::Leaf { label, value } => {
                    debug_assert_eq!(
                        Self::common_prefix_len(
                            &self.data
                                [(label.offset as usize)..((label.offset + label.len) as usize)],
                            &name[offset..]
                        ),
                        label.len
                    );
                    return Some(*value);
                }
                FunctionTrieNode::Internal { childs, .. } | FunctionTrieNode::Root { childs } => {
                    for child in childs {
                        let successor = &*child.borrow();

                        let label = successor
                            .get_label()
                            .expect("The root node cannot be the child of an internal node.");
                        let prefix_len = Self::common_prefix_len(
                            &self.data
                                [(label.offset as usize)..((label.offset + label.len) as usize)],
                            &name[offset..],
                        );
                        debug_assert!(prefix_len <= label.len);

                        if prefix_len > 0 {
                            if let FunctionTrieNode::Internal { .. } = successor {
                                offset += prefix_len as usize;
                                next = child.clone();
                                visit_child = true;
                                break;
                            } else if let FunctionTrieNode::Leaf { .. } = successor
                                && prefix_len == label.len
                            {
                                next = child.clone();
                                visit_child = true;
                                break;
                            }
                        }
                    }

                    if !visit_child {
                        return None;
                    }
                }
                FunctionTrieNode::Wildcard { .. } => todo!(),
            }
            node = next;
        }
    }

    fn common_prefix_len(label: &str, val: &str) -> u32 {
        let mut chars = label.chars().zip(val.chars());

        let mut prefix: u32 = 0;
        while let Some((a, b)) = chars.next() {
            if a != b {
                break;
            }
            prefix += 1;
        }
        prefix
    }

    fn find_child_with_longest_common_prefix(
        data: &str,
        string: &str,
        root: &Rc<RefCell<FunctionTrieNode>>,
    ) -> TrieNodeInsertionPoint {
        let mut node: Rc<RefCell<FunctionTrieNode>> = root.clone();

        loop {
            let mut next: Rc<RefCell<FunctionTrieNode>> = node.clone();
            let mut visit_child: bool = false;

            match &*node.borrow() {
                FunctionTrieNode::Root { childs, .. }
                | FunctionTrieNode::Internal { childs, .. } => {
                    for child in childs {
                        let successor = &*child.borrow();

                        let label = successor.get_label().expect("Cannot encounter root node");

                        let len = Self::common_prefix_len(
                            &data[(label.offset as usize)..((label.offset + label.len) as usize)],
                            string,
                        );
                        debug_assert!(len as usize <= string.len());
                        debug_assert!(len <= label.len);

                        match successor {
                            FunctionTrieNode::Internal { .. } => {
                                if len == label.len {
                                    next = child.clone();
                                    visit_child = true;
                                    break;
                                }

                                if len > 0 {
                                    return TrieNodeInsertionPoint {
                                        node: child.clone(),
                                        prefix_len: len,
                                    };
                                }
                            }
                            FunctionTrieNode::Leaf { .. } => {
                                if len > 0 {
                                    return TrieNodeInsertionPoint {
                                        node: child.clone(),
                                        prefix_len: len,
                                    };
                                }
                            }
                            FunctionTrieNode::Wildcard { .. } => unimplemented!(),
                            FunctionTrieNode::Root { .. } => {
                                unreachable!("Cannot encounter the root node.")
                            }
                        }
                    }
                }
                FunctionTrieNode::Leaf { .. } => unreachable!("Leaf nodes are not visited."),
                FunctionTrieNode::Wildcard { .. } => todo!(),
            }

            node = next;

            if !visit_child {
                return TrieNodeInsertionPoint {
                    node,
                    prefix_len: 0,
                };
            }
        }
    }

    fn insert_regular_function_name(
        &mut self,
        name: &PracticeFunctionName,
        slot: FunctionNameSlot,
    ) {
        debug_assert!(!name.has_wildcard());

        // We need to perform a case insensitive lookup, because lower-
        // and uppercase spelling does only indicate which letters are
        // mandatory.
        let key = name.to_string().to_ascii_lowercase();

        let str_repr = key.to_string();
        let str_repr_len: u32 = key.len() as u32;

        let TrieNodeInsertionPoint { node, prefix_len } =
            Self::find_child_with_longest_common_prefix(&self.data, &str_repr, &self.root);

        if let FunctionTrieNode::Root { childs } = &mut *node.borrow_mut() {
            childs.push(Rc::new(RefCell::new(FunctionTrieNode::Leaf {
                label: FunctionTrieNodeLabel {
                    border: self.data.len() as u32,
                    offset: self.data.len() as u32,
                    len: str_repr.len() as u32,
                },
                value: slot,
            })));

            self.data.push_str(&str_repr);
        } else {
            let old = &mut *node.borrow_mut();

            let label = match old {
                FunctionTrieNode::Internal { label, .. } => label,
                FunctionTrieNode::Leaf { label, .. } => label,
                FunctionTrieNode::Wildcard { .. } => todo!(),
                FunctionTrieNode::Root { .. } => {
                    unreachable!("The root node is handled elsewhere.")
                }
            };
            debug_assert!(label.border <= label.offset);

            debug_assert!(prefix_len <= label.len);
            debug_assert!(prefix_len as usize <= str_repr.len());

            let data_len: u32 = self.data.len() as u32;
            self.data.push_str(&str_repr);

            if let FunctionTrieNode::Leaf { label, value } = old {
                // The remaining suffix of the existing leaf
                let a = FunctionTrieNode::Leaf {
                    label: FunctionTrieNodeLabel {
                        border: label.border,
                        offset: label.offset + prefix_len,
                        len: label.len - prefix_len,
                    },
                    value: *value,
                };
                debug_assert!(label.border <= label.offset + prefix_len);
                debug_assert!((label.offset + label.len) as usize <= self.data.len());

                // The remaining suffix of the new data entry
                let b = FunctionTrieNode::Leaf {
                    label: FunctionTrieNodeLabel {
                        border: data_len,
                        offset: data_len + prefix_len,
                        len: str_repr_len - prefix_len,
                    },
                    value: slot,
                };
                debug_assert!((data_len + str_repr_len) as usize <= self.data.len());

                // The new parent node with the common prefix
                let inner = FunctionTrieNode::Internal {
                    label: FunctionTrieNodeLabel {
                        border: data_len,
                        offset: data_len + label.offset - label.border,
                        len: prefix_len,
                    },
                    childs: vec![
                        Rc::new(RefCell::new(a.clone())),
                        Rc::new(RefCell::new(b.clone())),
                    ],
                };
                debug_assert!(
                    (data_len + label.offset - label.border + prefix_len) as usize
                        <= self.data.len()
                );

                *old = inner;
            } else if let FunctionTrieNode::Internal { label, childs } = old {
                if prefix_len == 0 {
                    // No need to modify the label of the parent node
                    let shift = label.len + prefix_len;

                    // The remaining suffix of the new data entry
                    let a = FunctionTrieNode::Leaf {
                        label: FunctionTrieNodeLabel {
                            border: data_len,
                            offset: data_len + shift,
                            len: str_repr_len - shift,
                        },
                        value: slot,
                    };
                    debug_assert!((data_len + str_repr_len) as usize <= self.data.len());

                    childs.push(Rc::new(RefCell::new(a)));
                } else {
                    let childs = childs.clone();

                    // The remaining suffix of the existing internal node
                    let a = FunctionTrieNode::Internal {
                        label: FunctionTrieNodeLabel {
                            border: label.border,
                            offset: label.border + prefix_len,
                            len: label.len - prefix_len,
                        },
                        childs,
                    };
                    debug_assert!((label.border + label.len) as usize <= self.data.len());

                    // The new parent node with the common prefix
                    let inner = FunctionTrieNode::Internal {
                        label: FunctionTrieNodeLabel {
                            border: data_len,
                            offset: data_len + label.offset - label.border,
                            len: prefix_len,
                        },
                        childs: vec![Rc::new(RefCell::new(a))],
                    };
                    debug_assert!(
                        (data_len + label.offset - label.border - prefix_len) as usize
                            <= self.data.len()
                    );

                    *old = inner;
                }
            }
        }
    }
}

impl FunctionTrieNode {
    pub fn get_label(&self) -> Option<&FunctionTrieNodeLabel> {
        match self {
            FunctionTrieNode::Internal { label, .. } | FunctionTrieNode::Leaf { label, .. } => {
                Some(label)
            }
            FunctionTrieNode::Wildcard { .. } => todo!(),
            FunctionTrieNode::Root { .. } => None,
        }
    }
}

///! See [Note: Command Long and Short Forms] for the conventions for handling
///! multiple command and functions variants.
fn permutate_long_and_short_forms(cmd: &PracticeFunctionName) -> Vec<PracticeFunctionName> {
    let mut parts = cmd.iter();

    let short = match parts.next().expect("Cannot be empty.") {
        PracticeFunctionNamePattern::Literal(c) => {
            let short: String = c.chars().filter(|&c| kept_in_short_form(c)).collect();
            PracticeFunctionNamePattern::Literal(short)
        }
        PracticeFunctionNamePattern::Angled(_) | PracticeFunctionNamePattern::Chained(_) => {
            unreachable!("Commands must not start with a wildcard pattern.")
        }
    };

    if short.is_empty() {
        let mut variants = gen_variants(cmd);
        if cmd.num_parts() > 1 {
            let (_, rest) = cmd.split_first();
            let other = PracticeFunctionName::from_parts(rest).unwrap();

            variants.append(&mut gen_variants(&other));
        }
        return variants;
    }
    gen_variants(cmd)
}

fn gen_variants(cmd: &PracticeFunctionName) -> Vec<PracticeFunctionName> {
    let mut variants: Vec<PracticeFunctionName> =
        Vec::with_capacity(2u32.pow(cmd.num_parts() as u32) as usize);

    let var = match &cmd[0] {
        PracticeFunctionNamePattern::Literal(c) => {
            PracticeFunctionNamePattern::Literal(c.to_string())
        }
        PracticeFunctionNamePattern::Angled(_) | PracticeFunctionNamePattern::Chained(_) => {
            cmd[0].clone()
        }
    };
    variants.push(funcname!(var));

    if let PracticeFunctionNamePattern::Literal(sub) = &cmd[0] {
        let short: String = sub.chars().filter(|&c| kept_in_short_form(c)).collect();

        if !(short.is_empty() || short == *sub) {
            variants.push(funcname!(PracticeFunctionNamePattern::Literal(short)));
        }
    }

    let end = &cmd[cmd.num_parts() - 1];

    let mut parts = cmd.iter();
    let _ = parts.next().expect("Cannot be empty.");

    while let Some(component) = parts.next() {
        if let PracticeFunctionNamePattern::Literal(sub) = component {
            let short: String = sub.chars().filter(|&c| kept_in_short_form(c)).collect();

            // If the last subcommand is empty, then there will be two variants:
            //   1. with all lower case subcommand
            //   2. no subcommand
            //
            // If there is a non-empty short form, then there will be two subcommand
            // variants.
            if (component != end && short.is_empty()) || short.eq_ignore_ascii_case(sub) {
                for var in &mut variants {
                    var.push(match component {
                        PracticeFunctionNamePattern::Literal(c) => {
                            PracticeFunctionNamePattern::Literal(c.to_string())
                        }
                        PracticeFunctionNamePattern::Angled(_)
                        | PracticeFunctionNamePattern::Chained(_) => component.clone(),
                    });
                }
                continue;
            }

            variants.append(&mut variants.clone());

            let pivot = variants.len() / 2;
            for var in &mut variants[..pivot] {
                var.push(match component {
                    PracticeFunctionNamePattern::Literal(c) => {
                        PracticeFunctionNamePattern::Literal(c.to_string())
                    }
                    PracticeFunctionNamePattern::Angled(_)
                    | PracticeFunctionNamePattern::Chained(_) => component.clone(),
                });
            }

            debug_assert!(!short.is_empty() || component == end);
            if !short.is_empty() {
                for var in &mut variants[pivot..] {
                    var.push(PracticeFunctionNamePattern::Literal(short.clone()));
                }
            }
            continue;
        }

        for var in &mut variants {
            var.push(match component {
                PracticeFunctionNamePattern::Angled(_)
                | PracticeFunctionNamePattern::Chained(_) => component.clone(),
                PracticeFunctionNamePattern::Literal(_) => {
                    unreachable!("Literals must not be handled here.")
                }
            });
        }
    }
    variants
}

fn kept_in_short_form(letter: char) -> bool {
    letter.is_ascii_uppercase() || letter.is_digit(10) || ['_', '<', '>'].contains(&letter)
}

#[cfg(test)]
mod tests {
    use t32_language_server_user_guide_data::format_practice_func_name;

    use super::*;
    use crate::ug::convert_user_guide_data;

    #[test]
    fn can_import_builtin_functions() {
        let (ug, _) = convert_user_guide_data().expect("Must not fail.");

        let functions = BuiltinFunctions::from_user_guide(&ug);

        for meta in functions.meta {
            assert!((meta.copyright as usize) < functions.copyrights.len());
            assert!((meta.doc as usize) < functions.docs.len());
        }
    }

    #[test]
    fn can_permutate_regular_short_and_long_forms() {
        let variants = permutate_long_and_short_forms(&funcname!(
            PracticeFunctionNamePattern::Literal("Abc".to_string()),
            PracticeFunctionNamePattern::Literal("aBc".to_string()),
            PracticeFunctionNamePattern::Literal("abC".to_string()),
        ));
        assert_eq!(variants.len(), 8);

        for l0 in ["A", "Abc"] {
            for l1 in ["B", "aBc"] {
                for l2 in ["C", "abC"] {
                    let cmd = format!("{}.{}.{}", l0, l1, l2);
                    assert!(variants.iter().any(|v| format_practice_func_name(v) == cmd));
                }
            }
        }
    }

    #[test]
    fn can_permutate_short_and_long_forms_with_unique_components() {
        let variants = permutate_long_and_short_forms(&funcname!(
            PracticeFunctionNamePattern::Literal("Abc".to_string()),
            PracticeFunctionNamePattern::Literal("ABC".to_string()),
            PracticeFunctionNamePattern::Literal("abC".to_string()),
        ));
        assert_eq!(variants.len(), 4);

        for l0 in ["A", "Abc"] {
            for l1 in ["ABC"] {
                for l2 in ["C", "abC"] {
                    let cmd = format!("{}.{}.{}", l0, l1, l2);
                    assert!(variants.iter().any(|v| format_practice_func_name(v) == cmd));
                }
            }
        }

        let variants = permutate_long_and_short_forms(&funcname!(
            PracticeFunctionNamePattern::Literal("a".to_string()),
            PracticeFunctionNamePattern::Literal("aBc".to_string()),
            PracticeFunctionNamePattern::Literal("abC".to_string()),
        ));
        assert_eq!(variants.len(), 8);

        for l0 in ["a"] {
            for l1 in ["aBc"] {
                for l2 in ["C", "abC"] {
                    let cmd = format!("{}.{}.{}", l0, l1, l2);
                    assert!(variants.iter().any(|v| v.to_string() == cmd));
                }
            }
        }

        for l0 in ["B", "aBc"] {
            for l1 in ["C", "abC"] {
                let cmd = format!("{}.{}", l0, l1);
                assert!(variants.iter().any(|v| v.to_string() == cmd));
            }
        }

        let variants = permutate_long_and_short_forms(&funcname!(
            PracticeFunctionNamePattern::Literal("Abc".to_string()),
            PracticeFunctionNamePattern::Literal("aBc".to_string()),
            PracticeFunctionNamePattern::Literal("abc".to_string()),
        ));
        assert_eq!(variants.len(), 8);

        for l0 in ["A", "Abc"] {
            for l1 in ["aBc"] {
                for l2 in ["abc"] {
                    let cmd = format!("{}.{}.{}", l0, l1, l2);
                    assert!(variants.iter().any(|v| v.to_string() == cmd));
                }
            }
        }

        for l0 in ["A", "Abc"] {
            for l1 in ["B", "aBc"] {
                let cmd = format!("{}.{}", l0, l1);
                assert!(variants.iter().any(|v| v.to_string() == cmd));
            }
        }
    }

    #[test]
    fn can_permutate_regular_short_and_long_forms_with_wildcard_patterns() {
        let variants = permutate_long_and_short_forms(&funcname!(
            PracticeFunctionNamePattern::Literal("Abc".to_string()),
            PracticeFunctionNamePattern::Angled("placeholder".to_string()),
            PracticeFunctionNamePattern::Literal("abC".to_string()),
        ));
        assert_eq!(variants.len(), 4);

        for l0 in ["A", "Abc"] {
            for l1 in ["<placeholder>"] {
                for l2 in ["C", "abC"] {
                    let cmd = format!("{}.{}.{}", l0, l1, l2);
                    assert!(variants.iter().any(|v| v.to_string() == cmd));
                }
            }
        }

        let variants = permutate_long_and_short_forms(&funcname!(
            PracticeFunctionNamePattern::Literal("Abc".to_string()),
            PracticeFunctionNamePattern::Chained(vec![
                PracticeFunctionNamePattern::Literal("1".to_string()),
                PracticeFunctionNamePattern::Angled("placeholder".to_string()),
            ]),
            PracticeFunctionNamePattern::Literal("abC".to_string()),
        ));
        assert_eq!(variants.len(), 4);

        for l0 in ["A", "Abc"] {
            for l1 in ["1<placeholder>"] {
                for l2 in ["C", "abC"] {
                    let cmd = format!("{}.{}.{}", l0, l1, l2);
                    assert!(variants.iter().any(|v| v.to_string() == cmd));
                }
            }
        }
    }

    #[test]
    fn can_lookup_function_names() {
        let mut trie = FunctionNameTrie::new(0);

        let names = [
            funcname!(
                PracticeFunctionNamePattern::Literal("a".to_string()),
                PracticeFunctionNamePattern::Literal("b".to_string()),
                PracticeFunctionNamePattern::Literal("c".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("1".to_string()),
                PracticeFunctionNamePattern::Literal("2".to_string()),
                PracticeFunctionNamePattern::Literal("3".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("0".to_string()),
                PracticeFunctionNamePattern::Literal("b".to_string()),
                PracticeFunctionNamePattern::Literal("c".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("a".to_string()),
                PracticeFunctionNamePattern::Literal("2".to_string()),
                PracticeFunctionNamePattern::Literal("c".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("a".to_string()),
                PracticeFunctionNamePattern::Literal("2".to_string()),
                PracticeFunctionNamePattern::Literal("3".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("a".to_string()),
                PracticeFunctionNamePattern::Literal("2".to_string()),
            ),
        ];

        for (ii, name) in names.iter().enumerate() {
            trie.insert_regular_function_name(name, FunctionNameSlot(ii as u32));
        }

        for (_ii, name) in names.iter().enumerate() {
            let Some(FunctionNameSlot(_ii)) = trie.get(&name.to_string()) else {
                panic!();
            };
        }
    }

    #[test]
    fn can_lookup_invalid_function_name() {
        let mut trie = FunctionNameTrie::new(0);

        let names = [
            funcname!(
                PracticeFunctionNamePattern::Literal("a".to_string()),
                PracticeFunctionNamePattern::Literal("b".to_string()),
                PracticeFunctionNamePattern::Literal("c".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("1".to_string()),
                PracticeFunctionNamePattern::Literal("2".to_string()),
                PracticeFunctionNamePattern::Literal("3".to_string()),
            ),
            funcname!(
                PracticeFunctionNamePattern::Literal("0".to_string()),
                PracticeFunctionNamePattern::Literal("b".to_string()),
                PracticeFunctionNamePattern::Literal("c".to_string()),
            ),
        ];

        for (ii, name) in names.iter().enumerate() {
            trie.insert_regular_function_name(name, FunctionNameSlot(ii as u32));
        }

        assert!(trie.get(&"UNKNOWN".to_string()).is_none());
    }
}
