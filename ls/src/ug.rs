// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{cell::RefCell, rc::Rc};

use serde_json::{self, error::Category};

#[cfg(debug_assertions)]
use serde_path_to_error;

use t32_language_server_user_guide_data::{
    DATFILE_FIELD_WIDTH_BITS_TOTAL, DATFILE_FIELD_WIDTH_NODE_CHAR, DATFILE_FIELD_WIDTH_NODE_TAG,
    DATFILE_FIELD_WIDTH_NODES_TOTAL, DATFILE_FIELD_WIDTH_SUCCESSOR, UserGuideData,
};

use crate::utils::get_error_offset;

#[derive(Clone, Debug)]
enum HuffmanNode {
    Leaf {
        ch: char,
    },
    Internal {
        left: Rc<RefCell<HuffmanNode>>,
        right: Rc<RefCell<HuffmanNode>>,
    },
}

#[derive(Debug)]
enum HuffmanNodeSerialized {
    Leaf { ch: char },
    Internal { left: u32, right: u32 },
}

const USER_GUIDE_DATA: [u8; include_bytes!("../../data/ugd.dat").len()] =
    *include_bytes!("../../data/ugd.dat");

pub fn convert_user_guide_data() -> Result<(UserGuideData, usize), String> {
    let data = inflate(&USER_GUIDE_DATA)?;

    Ok((import(&data)?, USER_GUIDE_DATA.len()))
}

fn inflate(data: &[u8]) -> Result<String, String> {
    let (payload, total, btree) = match preamble(data) {
        Ok(res) => res,
        Err(off) => return Err(error_preamble(off)),
    };

    debug_assert!(total <= (data.len() as u64) * 8);

    let mut out: String = String::with_capacity(payload.len());

    let masks: [u8; 8] = [
        0b0000_0001,
        0b0000_0010,
        0b0000_0100,
        0b0000_1000,
        0b0001_0000,
        0b0010_0000,
        0b0100_0000,
        0b1000_0000,
    ];

    let mut num_bits: u64 = 0;

    let btree = Rc::new(RefCell::new(btree));
    let mut node: Rc<RefCell<HuffmanNode>> = btree.clone();

    while num_bits < total {
        let byte_offset = num_bits / 8;
        let bit_offset = num_bits % 8;

        let visitor = node.clone();

        let HuffmanNode::Internal { left, right, .. } = &*visitor.borrow() else {
            unreachable!("Pathological case of an alphabet with a single letter is not supported.");
        };

        let visitor =
            if ((payload[byte_offset as usize] & masks[bit_offset as usize]) >> bit_offset) == 1 {
                right.clone()
            } else {
                left.clone()
            };

        if let HuffmanNode::Leaf { ch, .. } = *visitor.borrow() {
            out.push(ch);
            node = btree.clone();
        } else {
            node = visitor;
        }
        num_bits += 1;
    }
    Ok(out)
}

fn preamble(data: &[u8]) -> Result<(&[u8], u64, HuffmanNode), u64> {
    let mut offset: usize = 0;

    let num_nodes: u32 = u16::from_le_bytes(
        data[..DATFILE_FIELD_WIDTH_NODES_TOTAL]
            .try_into()
            .map_err(|_| offset as u64)?,
    ) as u32;

    if num_nodes <= 0 {
        return Err(offset as u64);
    }

    offset += DATFILE_FIELD_WIDTH_NODES_TOTAL;

    let mut nodes: Vec<HuffmanNodeSerialized> = Vec::with_capacity(num_nodes as usize);

    for _ in 0..num_nodes {
        let tag: u8 = u8::from_le_bytes(
            data[offset..(offset + DATFILE_FIELD_WIDTH_NODE_TAG)]
                .try_into()
                .map_err(|_| offset as u64)?,
        );

        offset += DATFILE_FIELD_WIDTH_NODE_TAG;

        let node = if tag > 0 {
            let left: u32 = u16::from_le_bytes(
                data[offset..(offset + DATFILE_FIELD_WIDTH_SUCCESSOR)]
                    .try_into()
                    .map_err(|_| offset as u64)?,
            ) as u32;

            offset += DATFILE_FIELD_WIDTH_SUCCESSOR;

            let right: u32 = u16::from_le_bytes(
                data[offset..(offset + DATFILE_FIELD_WIDTH_SUCCESSOR)]
                    .try_into()
                    .map_err(|_| offset as u64)?,
            ) as u32;

            offset += DATFILE_FIELD_WIDTH_SUCCESSOR;

            HuffmanNodeSerialized::Internal { left, right }
        } else {
            let ch: char = char::from_u32(u32::from_le_bytes(
                data[offset..(offset + DATFILE_FIELD_WIDTH_NODE_CHAR)]
                    .try_into()
                    .map_err(|_| offset as u64)?,
            ))
            .ok_or_else(|| offset as u64)?;

            offset += DATFILE_FIELD_WIDTH_NODE_CHAR;

            HuffmanNodeSerialized::Leaf { ch }
        };
        nodes.push(node);
    }

    let num_bits = u64::from_le_bytes(
        data[offset..(offset + DATFILE_FIELD_WIDTH_BITS_TOTAL)]
            .try_into()
            .map_err(|_| offset as u64)?,
    );

    offset += DATFILE_FIELD_WIDTH_BITS_TOTAL;

    Ok((&data[offset..], num_bits, btree(nodes)))
}

fn btree(nodes: Vec<HuffmanNodeSerialized>) -> HuffmanNode {
    let placeholder: HuffmanNode = HuffmanNode::Leaf { ch: '\0' };

    let mut ends: Vec<(&HuffmanNodeSerialized, Rc<RefCell<HuffmanNode>>)> = Vec::new();

    let root = match nodes[0] {
        HuffmanNodeSerialized::Leaf { ch } => HuffmanNode::Leaf { ch },
        HuffmanNodeSerialized::Internal { left, right } => {
            let left_child: Rc<RefCell<HuffmanNode>> = Rc::new(RefCell::new(placeholder.clone()));
            let right_child: Rc<RefCell<HuffmanNode>> = Rc::new(RefCell::new(placeholder.clone()));

            ends.push((&nodes[left as usize], left_child.clone()));
            ends.push((&nodes[right as usize], right_child.clone()));

            HuffmanNode::Internal {
                left: left_child,
                right: right_child,
            }
        }
    };

    loop {
        let Some((linear, tree)) = ends.pop() else {
            break root;
        };

        let node = match linear {
            HuffmanNodeSerialized::Leaf { ch } => HuffmanNode::Leaf { ch: *ch },
            HuffmanNodeSerialized::Internal { left, right } => {
                let left_child: Rc<RefCell<HuffmanNode>> =
                    Rc::new(RefCell::new(placeholder.clone()));
                let right_child: Rc<RefCell<HuffmanNode>> =
                    Rc::new(RefCell::new(placeholder.clone()));

                ends.push((&nodes[*left as usize], left_child.clone()));
                ends.push((&nodes[*right as usize], right_child.clone()));

                HuffmanNode::Internal {
                    left: left_child,
                    right: right_child,
                }
            }
        };

        let inner = &mut *tree.borrow_mut();
        *inner = node;
    }
}
fn import(text: &str) -> Result<UserGuideData, String> {
    cfg_select! {
        debug_assertions => {
            let des = &mut serde_json::Deserializer::from_str(text);
            match serde_path_to_error::deserialize(des) {
                Ok(val) => Ok(val),
                Err(err) => {
                    match err.inner().classify() {
                        Category::Io => unreachable!(), // Byte buffer must be valid.
                        Category::Syntax => return Err(precise_error_syntax(err.path().to_string(), err.into_inner(), Some(text.as_bytes()))),
                        Category::Data => return Err(precise_error_data(err.path().to_string(), err.into_inner(), Some(text.as_bytes()))),
                        Category::Eof => return Err(precise_error_incomplete(err.path().to_string(), err.into_inner(), text.as_bytes().len())),
                    }
                }
            }
        }
        _ => {
            match serde_json::from_str(text) {
                Ok(val) => Ok(val),
                Err(err) => {
                    match err.classify() {
                        Category::Io => unreachable!(), // Byte buffer must be valid.
                        Category::Syntax => return Err(imprecise_error_syntax(err, Some(text.as_bytes()))),
                        Category::Data => return Err(imprecise_error_data(err, Some(text.as_bytes()))),
                        Category::Eof => return Err(imprecise_error_incomplete(err, text.len())),
                    }
                }
            }
        }
    }
}

fn error_preamble(offset: u64) -> String {
    format!(
        "Data error: Unexpected data in user guide data file preamble at offset {}.",
        offset
    )
}

#[cfg(not(debug_assertions))]
fn imprecise_error_syntax(err: serde_json::Error, buf: Option<&[u8]>) -> String {
    if err.line() == 0 {
        format!(
            "Syntax error: Unexpected data in user guide data due to {}.",
            err.to_string()
        )
    } else {
        let offset = match buf {
            Some(b) => get_error_offset(&err, b),
            None => 0,
        };

        format!(
            "Syntax error: Unexpected data in user guide data at offset \"{}\" due to {}.",
            offset,
            err.to_string(),
        )
    }
}

#[cfg(debug_assertions)]
fn precise_error_syntax(path: String, err: serde_json::Error, buf: Option<&[u8]>) -> String {
    if err.line() == 0 {
        format!(
            "Syntax error: {}: Unexpected data in user guide data due to {}.",
            path,
            err.to_string()
        )
    } else {
        let offset = match buf {
            Some(b) => get_error_offset(&err, b),
            None => 0,
        };

        format!(
            "Syntax error: {}: Unexpected data in user guide data at offset \"{}\" due to {}.",
            path,
            offset,
            err.to_string(),
        )
    }
}

#[cfg(not(debug_assertions))]
fn imprecise_error_data(err: serde_json::Error, buf: Option<&[u8]>) -> String {
    if err.line() == 0 {
        format!(
            "Data error: Semantically incorrect data in user guide data due to {}.",
            err.to_string()
        )
    } else {
        let offset = match buf {
            Some(b) => get_error_offset(&err, b),
            None => 0,
        };

        format!(
            "Data error: Semantically incorrect data in user guide data at offset \"{}\" due to {}.",
            offset,
            err.to_string()
        )
    }
}

#[cfg(debug_assertions)]
fn precise_error_data(path: String, err: serde_json::Error, buf: Option<&[u8]>) -> String {
    if err.line() == 0 {
        format!(
            "Data error: {}: Semantically incorrect data in user guide data due to {}.",
            path,
            err.to_string(),
        )
    } else {
        let offset = match buf {
            Some(b) => get_error_offset(&err, b),
            None => 0,
        };

        format!(
            "Data error: {}: Semantically incorrect data in user guide data at offset \"{}\" due to {}.",
            path,
            offset,
            err.to_string()
        )
    }
}

#[cfg(not(debug_assertions))]
fn imprecise_error_incomplete(err: serde_json::Error, len: usize) -> String {
    format!(
        "Data error: User guide data is incomplete due to {}. Expected a total length of \"{}\" bytes.",
        err.to_string(),
        len
    )
}

#[cfg(debug_assertions)]
fn precise_error_incomplete(path: String, err: serde_json::Error, len: usize) -> String {
    format!(
        "Data error: {}: User guide data is incomplete due to {}. Expected a total length of \"{}\" bytes.",
        path,
        err.to_string(),
        len
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_import_user_guide_data() {
        convert_user_guide_data().expect("Must not fail.");
    }
}
