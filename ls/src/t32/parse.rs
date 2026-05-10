// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

//! [Note] Tree Re-Parsing
//! ======================
//!
//! When editing a tree for reparsing, the row-column coordinates can be
//! ignored (see[GitHub discussion]). Only providing the right bytes ranges for
//! `InputEdit` is mandatory.
//! The `old_end_byte` and `new_end_byte` of `InputEdit` should end  end one
//! byte after the last byte of the edited range (0-based). This mirrors the
//! rules of the LSP specification where the right end of the range is not
//! part of the range itself.
//!
//! The expression
//!
//! ~~~~ text
//! &s="&hello"+", world"\n
//! ~~~~
//!
//! creates this abstract syntax tree:
//!
//! ~~~~ text
//! (script (0, 0) - (1, 0) 0..22
//!   (assignment_expression (0, 0) - (1, 0) 0..22
//!     left: (macro) (0, 0) - (0, 2) 0..2
//!     right: (binary_expression (0, 3) - (0, 21) 3..21
//!       left: (string (0, 3) - (0, 11) 3..11
//!         (macro) (0, 4) - (0, 10)) 4..10
//!       right: (string) (0, 12) - (0, 21) 12..21)))
//! ~~~~
//!
//! [GitHub discussion]: https://github.com/tree-sitter/tree-sitter/discussions/1793
//!

use tree_sitter::{InputEdit, Parser, Tree};
use tree_sitter_t32;
pub fn parse_full(text: &[u8]) -> Tree {
    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_t32::LANGUAGE.into())
        .expect("Cannot load t32 grammar.");

    parser
        .parse(text, None)
        .expect("TRACE32 script parser must not fail.")
}

pub fn parse_incremental(text: &[u8], edits: &InputEdit, old_tree: &mut Tree) -> Tree {
    old_tree.edit(&edits);

    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_t32::LANGUAGE.into())
        .expect("Cannot load t32 grammar.");

    parser
        .parse(text, Some(old_tree))
        .expect("TRACE32 script parser must not fail.")
}

#[cfg(test)]
mod test {
    use super::*;

    use tree_sitter::{InputEdit, Point};

    #[allow(dead_code)]
    fn dump_tree(tree: &Tree) {
        let mut cursor = tree.walk();
        'outer: loop {
            eprintln!(">>>>>>>>>>");
            eprintln!("{}", cursor.node());
            eprintln!("{:#?}", cursor.node().byte_range());
            eprintln!("----------");

            if cursor.goto_first_child() {
                continue;
            }

            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    break 'outer;
                }
            }
        }
    }

    #[test]
    fn can_create_ast() {
        let text = "PRINT \"&hello\"\n";

        let tree = parse_full(text.as_bytes());

        let cmd_expr = "(command_expression";
        let arg = "(string (macro))";
        let error = "ERROR";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(cmd_expr.len())
                .filter(|s| *s == cmd_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(arg.len())
                .filter(|s| *s == arg.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );
    }

    #[test]
    fn can_update_ast_after_append_operation() {
        let text = "&a";

        let tree = parse_full(text.as_bytes());

        let text = "&a=1+1\n";

        let edits = InputEdit {
            start_byte: 2,
            old_end_byte: 2,
            new_end_byte: 7,
            start_position: Point { row: 0, column: 1 },
            old_end_position: Point { row: 0, column: 2 },
            new_end_position: Point { row: 1, column: 0 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        let bin_expr = "(binary_expression";
        let integer = "(integer)";
        let error = "ERROR";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(integer.len())
                .filter(|s| *s == integer.as_bytes())
                .count(),
            2
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );

        let text = "&a=1+1\nPRINT \"&hello\"\n";

        let edits = InputEdit {
            start_byte: 2,
            old_end_byte: 7,
            new_end_byte: 22,
            start_position: Point { row: 1, column: 0 },
            old_end_position: Point { row: 1, column: 0 },
            new_end_position: Point { row: 2, column: 0 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(integer.len())
                .filter(|s| *s == integer.as_bytes())
                .count(),
            2
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );

        let cmd_expr = "(command_expression";
        let arg = "(string (macro))";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(cmd_expr.len())
                .filter(|s| *s == cmd_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(arg.len())
                .filter(|s| *s == arg.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );
    }

    #[test]
    fn can_update_ast_after_prepend_operation() {
        let text = "&a\n";

        let tree = parse_full(text.as_bytes());

        let text = "&c=\"&b\"+&a\n";

        let edits = InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 8,
            start_position: Point { row: 0, column: 0 },
            old_end_position: Point { row: 0, column: 0 },
            new_end_position: Point { row: 0, column: 8 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        let bin_expr = "(binary_expression";
        let r#macro = "(macro)";
        let error = "ERROR";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(r#macro.len())
                .filter(|s| *s == r#macro.as_bytes())
                .count(),
            3
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );

        let text = "PRINT \"&hello\"\n&c=\"&b\"+&a\n";
        let edits = InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 15,
            start_position: Point { row: 0, column: 0 },
            old_end_position: Point { row: 0, column: 0 },
            new_end_position: Point { row: 1, column: 0 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(r#macro.len())
                .filter(|s| *s == r#macro.as_bytes())
                .count(),
            4
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );

        let cmd_expr = "(command_expression";
        let arg = "(string (macro))";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(cmd_expr.len())
                .filter(|s| *s == cmd_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(arg.len())
                .filter(|s| *s == arg.as_bytes())
                .count(),
            2
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );
    }

    #[test]
    fn can_update_ast_after_insert_operation() {
        let text = "&a=1\n";

        let tree = parse_full(text.as_bytes());

        let text = "&a=&b+1\n";

        let edits = InputEdit {
            start_byte: 3,
            old_end_byte: 3,
            new_end_byte: 6,
            start_position: Point { row: 0, column: 3 },
            old_end_position: Point { row: 0, column: 3 },
            new_end_position: Point { row: 0, column: 6 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        let bin_expr = "(binary_expression";
        let r#macro = "(macro)";
        let integer = "(integer)";
        let error = "ERROR";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(r#macro.len())
                .filter(|s| *s == r#macro.as_bytes())
                .count(),
            2
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(integer.len())
                .filter(|s| *s == integer.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );

        let text = "&a=SYStem.Up()\n&c=&b+1\n";

        let edits = InputEdit {
            start_byte: 3,
            old_end_byte: 3,
            new_end_byte: 18,
            start_position: Point { row: 0, column: 3 },
            old_end_position: Point { row: 0, column: 3 },
            new_end_position: Point { row: 0, column: 18 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(r#macro.len())
                .filter(|s| *s == r#macro.as_bytes())
                .count(),
            3
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(integer.len())
                .filter(|s| *s == integer.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );
    }

    #[test]
    fn can_update_ast_after_delete_operation() {
        let text = "PRINT STRing.LoWeR(\"&a\")+\"&b\"\n&c=STRing.UPpeR(\"up\")\n";

        let tree = parse_full(text.as_bytes());

        let text = "PRINT STRing.LoWeR(\"&a\")\n&c=STRing.UPpeR(\"up\")\n";

        let edits = InputEdit {
            start_byte: 25,
            old_end_byte: 30,
            new_end_byte: 25,
            start_position: Point { row: 0, column: 25 },
            old_end_position: Point { row: 0, column: 30 },
            new_end_position: Point { row: 0, column: 25 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        let bin_expr = "(binary_expression";
        let call_expr = "(call_expression";
        let r#macro = "(macro)";
        let error = "ERROR";

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            0
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(call_expr.len())
                .filter(|s| *s == call_expr.as_bytes())
                .count(),
            2
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(r#macro.len())
                .filter(|s| *s == r#macro.as_bytes())
                .count(),
            2
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );

        let text = "PRINT STRing.UPpeR(\"up\")\n";

        let edits = InputEdit {
            start_byte: 6,
            old_end_byte: 28,
            new_end_byte: 6,
            start_position: Point { row: 0, column: 6 },
            old_end_position: Point { row: 1, column: 3 },
            new_end_position: Point { row: 0, column: 6 },
        };

        let mut old_tree = tree;
        old_tree.edit(&edits);

        let tree = parse_incremental(text.as_bytes(), &edits, &mut old_tree);

        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(bin_expr.len())
                .filter(|s| *s == bin_expr.as_bytes())
                .count(),
            0
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(call_expr.len())
                .filter(|s| *s == call_expr.as_bytes())
                .count(),
            1
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(r#macro.len())
                .filter(|s| *s == r#macro.as_bytes())
                .count(),
            0
        );
        assert_eq!(
            tree.root_node()
                .to_sexp()
                .as_bytes()
                .windows(error.len())
                .filter(|s| *s == error.as_bytes())
                .count(),
            0
        );
    }
}
