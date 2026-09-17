// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fs,
    path::Path,
    rc::Rc,
};

use serde_json::json;

use t32_language_server_user_guide_data::{
    DATFILE_FIELD_WIDTH_BITS_TOTAL, DATFILE_FIELD_WIDTH_NODE_CHAR, DATFILE_FIELD_WIDTH_NODE_TAG,
    DATFILE_FIELD_WIDTH_NODES_TOTAL, DATFILE_FIELD_WIDTH_SUCCESSOR,
};

use crate::{ReturnCode, UserGuideData};

#[derive(Clone, Debug)]
enum Node {
    Leaf {
        idx: u32,
        weight: u32,
        ch: char,
    },
    Internal {
        idx: u32,
        weight: u32,
        left: Rc<RefCell<Node>>,
        right: Rc<RefCell<Node>>,
    },
}

type MaskValues = Vec<Vec<u8>>;

#[derive(Debug)]
struct Decoder {
    root: Node,
    num_leaves: u32,
    num_internal: u32,
}

#[derive(Debug)]
struct Encoder {
    codes: BTreeMap<char, (MaskValues, u32)>,
}

struct PriorityQueue {
    queue: VecDeque<Node>,
}

impl Decoder {
    pub fn from_btree(tree: Node) -> Self {
        let (num_leaves, num_internal) = Self::count_nodes(&tree);

        Self {
            num_leaves,
            num_internal,
            root: tree,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        debug_assert!(self.num_leaves > 0);

        let mut out: Vec<u8> = Vec::with_capacity(
            DATFILE_FIELD_WIDTH_NODES_TOTAL
                + (self.num_leaves as usize)
                    * (DATFILE_FIELD_WIDTH_NODE_TAG + DATFILE_FIELD_WIDTH_NODE_CHAR)
                + (self.num_internal as usize)
                    * (DATFILE_FIELD_WIDTH_NODE_TAG + 2 * DATFILE_FIELD_WIDTH_SUCCESSOR),
        );

        ((self.num_leaves + self.num_internal) as u16)
            .to_le_bytes()
            .iter()
            .for_each(|b| out.push(*b));

        let root = Rc::new(RefCell::new(self.root.clone()));

        let mut node: Rc<RefCell<Node>> = root;
        let mut predecessors: Vec<Rc<RefCell<Node>>> = Vec::new();

        loop {
            let inner: Rc<RefCell<Node>> = node.clone();

            match *inner.borrow() {
                Node::Leaf { ch, .. } => {
                    out.push(0u8);

                    for byte in (ch as u32).to_le_bytes() {
                        out.push(byte);
                    }

                    if predecessors.is_empty() {
                        break;
                    } else {
                        let parent = predecessors.pop().unwrap();
                        let Node::Internal { ref right, .. } = *parent.borrow() else {
                            unreachable!("Leaf nodes cannot be parents.");
                        };
                        node = right.clone();
                    }
                }
                Node::Internal {
                    ref left,
                    ref right,
                    ..
                } => {
                    out.push(1u8);

                    for succ in [&*left.borrow(), &*right.borrow()] {
                        let next = match succ {
                            Node::Leaf { idx: n, .. } => *n,
                            Node::Internal { idx: n, .. } => *n,
                        };

                        for byte in (next as u16).to_le_bytes() {
                            out.push(byte);
                        }
                    }

                    predecessors.push(node);
                    node = left.clone();
                }
            }
        }
        out
    }

    fn count_nodes(tree: &Node) -> (u32, u32) {
        let root = Rc::new(RefCell::new(tree.clone()));

        let mut predecessors: Vec<Rc<RefCell<Node>>> = Vec::new();

        let mut num_leaves: u32 = 0;
        let mut num_internal: u32 = 0;

        let mut node: Rc<RefCell<Node>> = root.clone();

        loop {
            let inner = node.clone();

            match *inner.borrow() {
                Node::Leaf { .. } => {
                    num_leaves += 1;

                    if predecessors.is_empty() {
                        break;
                    } else {
                        let parent = predecessors.pop().unwrap();
                        let Node::Internal { ref right, .. } = *parent.borrow() else {
                            unreachable!("Leaf nodes cannot be parents.");
                        };
                        node = right.clone();
                    }
                }
                Node::Internal { ref left, .. } => {
                    num_internal += 1;

                    predecessors.push(node.clone());
                    node = left.clone();
                }
            }
        }
        (num_leaves, num_internal)
    }
}

impl Encoder {
    pub fn build(codes: BTreeMap<char, Vec<u8>>) -> Self {
        let mut dict: BTreeMap<char, (Vec<Vec<u8>>, u32)> = BTreeMap::new();
        for ch in codes.keys() {
            let shifted: MaskValues = Vec::new();

            dict.insert(*ch, (shifted, 0));
        }

        let masks: [Vec<u8>; 8] = [
            vec![],
            vec![0],
            vec![0, 0],
            vec![0, 0, 0],
            vec![0, 0, 0, 0],
            vec![0, 0, 0, 0, 0],
            vec![0, 0, 0, 0, 0, 0],
            vec![0, 0, 0, 0, 0, 0, 0],
        ];

        for (ch, code) in codes.into_iter() {
            if code.is_empty() {
                continue;
            }
            let Some((offset_mask, len)) = dict.get_mut(&ch) else {
                unreachable!("Dict must hold value encoding.");
            };
            *len = code.len() as u32;

            for (ii, mask) in masks.iter().enumerate() {
                offset_mask.push(Vec::new());

                let val = &mut offset_mask[ii];
                val.push(0);

                let mut bits_total = mask.len();

                for bit in &code {
                    if bits_total % 8 == 0 && bits_total > 7 {
                        val.push(0);
                    }
                    let end = val.len() - 1;
                    val[end] |= *bit << (bits_total % 8);
                    bits_total += 1;
                }
            }
        }
        Self { codes: dict }
    }
}

impl Node {
    pub fn weight(&self) -> u32 {
        match self {
            Node::Leaf { weight, .. } => *weight,
            Node::Internal { weight, .. } => *weight,
        }
    }
}

impl PriorityQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
        }
    }
    pub fn from_freq(frequencies: Vec<(char, u32)>) -> Self {
        let mut queue: VecDeque<Node> = VecDeque::with_capacity(frequencies.len());

        for (ch, val) in frequencies {
            queue.push_back(Node::Leaf {
                idx: 0,
                weight: val,
                ch,
            });
        }
        Self {
            queue: Self::sort(queue),
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn push(&mut self, node: Node) {
        self.queue.push_back(node);
    }

    pub fn pop(&mut self) -> Option<Node> {
        self.queue.pop_front()
    }

    pub fn peek_front(&self) -> Option<u32> {
        if self.queue.is_empty() {
            return None;
        }

        match self.queue[0] {
            Node::Leaf { weight, .. } => Some(weight),
            Node::Internal { weight, .. } => Some(weight),
        }
    }
    fn sort(mut queue: VecDeque<Node>) -> VecDeque<Node> {
        let d = queue.make_contiguous();

        d.sort_by(|a, b| {
            let a = match a {
                Node::Leaf { weight, .. } => *weight,
                Node::Internal { weight, .. } => *weight,
            };

            let b = match b {
                Node::Leaf { weight, .. } => weight,
                Node::Internal { weight, .. } => weight,
            };
            a.cmp(b)
        });

        queue
    }
}

pub fn write_outfile(ugd: UserGuideData, outfile: &Path) -> Result<(), ReturnCode> {
    if let Some(dir) = outfile.parent() {
        if dir.is_file() {
            eprintln!(
                "ERROR: Cannot create output file. \"{}\" is a file.",
                dir.display()
            );
            return Err(ReturnCode::UsageErr);
        }

        if !dir.is_dir() {
            if let Err(err) = fs::create_dir_all(dir) {
                eprintln!("ERROR: Cannot create output directory: {}", err);
                return Err(ReturnCode::CantCreateErr);
            }
        }
    }

    let payload = huffman(&json!(ugd).to_string());

    if let Err(err) = fs::write(outfile, payload) {
        eprintln!("ERROR: Cannot create output file: {}", err);
        return Err(ReturnCode::CantCreateErr);
    }
    Ok(())
}

fn huffman(text: &str) -> Vec<u8> {
    let freq = frequencies(text);

    let root = btree(freq);
    let decoder = Decoder::from_btree(root.clone());

    let dictionary = dictionary(root);

    deflate(&decoder, &dictionary, text)
}

fn frequencies(text: &str) -> Vec<(char, u32)> {
    let mut distrib: BTreeMap<char, u32> = BTreeMap::new();

    for ch in text.chars() {
        if let Some(val) = distrib.get_mut(&ch) {
            *val += 1;
        } else {
            distrib.insert(ch, 1);
        }
    }

    let freq: Vec<(char, u32)> = distrib.into_iter().filter(|(_, val)| *val > 0).collect();
    freq
}

fn btree(distrib: Vec<(char, u32)>) -> Node {
    let mut heap_int = PriorityQueue::new(distrib.len());
    let mut heap_sym = PriorityQueue::from_freq(distrib);

    if heap_sym.len() == 1 {
        return heap_sym.pop().unwrap();
    }

    while (heap_int.len() + heap_sym.len()) > 1 {
        let (left, right): (Rc<RefCell<Node>>, Rc<RefCell<Node>>) = (
            Rc::new(RefCell::new(pop_rarest(&mut heap_sym, &mut heap_int))),
            Rc::new(RefCell::new(pop_rarest(&mut heap_sym, &mut heap_int))),
        );

        let weight = left.borrow().weight() + right.borrow().weight();

        let node = Node::Internal {
            idx: 0,
            weight,
            left,
            right,
        };
        heap_int.push(node);
    }
    debug_assert_eq!(heap_sym.len(), 0);
    debug_assert_ne!(heap_int.len(), 0);

    let root = Rc::new(RefCell::new(heap_int.pop().unwrap()));

    let mut predecessors: Vec<Rc<RefCell<Node>>> = Vec::new();
    let mut node: Rc<RefCell<Node>> = root.clone();

    let mut num: u32 = 0;

    loop {
        let inner = node.clone();

        match *inner.borrow_mut() {
            Node::Leaf { ref mut idx, .. } => {
                *idx = num;
                num += 1;

                if predecessors.is_empty() {
                    break;
                } else {
                    let parent = predecessors.pop().unwrap();
                    let Node::Internal { ref right, .. } = *parent.borrow() else {
                        unreachable!("Leaf nodes cannot be parents.");
                    };
                    node = right.clone();
                }
            }
            Node::Internal {
                ref mut idx,
                ref left,
                ..
            } => {
                *idx = num;
                num += 1;

                predecessors.push(node.clone());
                node = left.clone();
            }
        }
    }
    root.borrow().clone()
}

fn dictionary(root: Node) -> Encoder {
    let mut node: Rc<RefCell<Node>> = Rc::new(RefCell::new(root));
    let mut buf: Vec<u8> = Vec::with_capacity(u8::MAX as usize);
    let mut predecessors: Vec<(Rc<RefCell<Node>>, u32)> = Vec::new();

    let mut codes: BTreeMap<char, Vec<u8>> = BTreeMap::new();

    loop {
        let inner = node.clone();

        match *inner.borrow_mut() {
            Node::Leaf { ch, .. } => {
                if predecessors.is_empty() {
                    // Rightmost leaf node
                    codes.insert(ch, buf.clone());
                    break;
                } else {
                    codes.insert(ch, buf.clone());

                    let (parent, end) = predecessors.pop().unwrap();
                    let Node::Internal { ref right, .. } = *parent.borrow() else {
                        unreachable!("Leaf nodes cannot be parents.");
                    };
                    buf.truncate(end as usize);

                    buf.push(1);

                    node = right.clone();
                }
            }
            Node::Internal { ref left, .. } => {
                predecessors.push((node.clone(), buf.len() as u32));

                node = left.clone();
                buf.push(0);
            }
        }
    }
    Encoder::build(codes)
}

fn deflate(btree: &Decoder, dict: &Encoder, text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = btree.serialize();

    let byte_offset_payload_bits: usize = out.len();

    for _ in 0..DATFILE_FIELD_WIDTH_BITS_TOTAL {
        out.push(0)
    }

    let mut bits_payload: u64 = 0;
    for ch in text.chars() {
        let bit_offset = bits_payload % 8;

        let Some((masks, num_bits)) = dict.codes.get(&ch) else {
            unreachable!("Symbol must be found in dictionary.");
        };

        let mut mask = masks[bit_offset as usize].clone();

        if bit_offset == 0 {
            out.append(&mut mask);
        } else {
            let end = out.len() - 1;
            out[end] |= mask[0];

            out.extend_from_slice(&mask[1..]);
        }
        bits_payload += *num_bits as u64;
    }

    for (ii, byte) in bits_payload.to_le_bytes().iter().enumerate() {
        out[byte_offset_payload_bits + ii] = *byte;
    }
    out
}

fn pop_rarest(a: &mut PriorityQueue, b: &mut PriorityQueue) -> Node {
    debug_assert!(a.len() + b.len() > 0);

    let front_a = a.peek_front();
    let front_b = b.peek_front();

    if let Some(weight_a) = front_a
        && let Some(weight_b) = front_b
    {
        if weight_a <= weight_b {
            a.pop().expect("Must have an element.")
        } else {
            b.pop().expect("Must have an element.")
        }
    } else if a.len() > 0 {
        a.pop().expect("Must have an element.")
    } else if b.len() > 0 {
        b.pop().expect("Must have an element.")
    } else {
        unreachable!("Both queues must not be empty.");
    }
}
