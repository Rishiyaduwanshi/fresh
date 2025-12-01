use std::ops::Range;
use std::sync::Arc;

use crate::model::piece_tree::{LeafData, PieceTreeNode};

/// Summary of differences between two piece tree roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceTreeDiff {
    /// Whether the two trees represent identical piece sequences.
    pub equal: bool,
    /// Changed byte range in the "after" tree (exclusive end). Empty when `equal` is true.
    pub byte_range: Range<usize>,
    /// Changed line range in the "after" tree (exclusive end). `None` when line counts are unknown.
    pub line_range: Option<Range<usize>>,
}

/// Compute a shallow diff between two piece tree roots.
///
/// This relies on piece sharing: if a piece (location+offset+bytes) appears
/// in both versions at the same relative order, it is considered unchanged.
/// The result identifies the minimal contiguous range in the "after" tree that
/// differs from "before". If the trees are identical, `equal` is true and
/// the ranges are empty.
pub fn diff_piece_trees(
    before: &Arc<PieceTreeNode>,
    after: &Arc<PieceTreeNode>,
) -> PieceTreeDiff {
    let mut before_leaves = Vec::new();
    collect_leaves(before, &mut before_leaves);

    let mut after_leaves = Vec::new();
    collect_leaves(after, &mut after_leaves);

    // Fast-path: identical leaf sequences.
    if leaf_slices_equal(&before_leaves, &after_leaves) {
        return PieceTreeDiff {
            equal: true,
            byte_range: 0..0,
            line_range: Some(0..0),
        };
    }

    // Longest common prefix.
    let mut prefix = 0;
    while prefix < before_leaves.len()
        && prefix < after_leaves.len()
        && leaves_equal(&before_leaves[prefix], &after_leaves[prefix])
    {
        prefix += 1;
    }

    // Longest common suffix (without overlapping prefix).
    let mut suffix = 0;
    while suffix + prefix < before_leaves.len()
        && suffix + prefix < after_leaves.len()
        && leaves_equal(
            &before_leaves[before_leaves.len() - 1 - suffix],
            &after_leaves[after_leaves.len() - 1 - suffix],
        ) {
        suffix += 1;
    }

    let after_changed = &after_leaves[prefix..after_leaves.len() - suffix];

    // Byte offsets are measured in the "after" tree.
    let start_byte = sum_bytes(&after_leaves[..prefix]);
    let end_byte = start_byte + sum_bytes(after_changed);

    // Line offsets are also relative to the "after" tree.
    let line_range = sum_line_feeds(&after_leaves[..prefix]).and_then(|lines_before| {
        // If we have no bytes in the changed span (pure deletion),
        // still mark a single line so callers have a location to attach to.
        let lines_in_changed = if after_changed.is_empty() {
            Some(1_usize)
        } else {
            lines_in_slice(after_changed)
        }?;
        Some(lines_before..lines_before + lines_in_changed)
    });

    PieceTreeDiff {
        equal: false,
        byte_range: start_byte..end_byte,
        line_range,
    }
}

fn collect_leaves(node: &Arc<PieceTreeNode>, out: &mut Vec<LeafData>) {
    match node.as_ref() {
        PieceTreeNode::Internal { left, right, .. } => {
            collect_leaves(left, out);
            collect_leaves(right, out);
        }
        PieceTreeNode::Leaf {
            location,
            offset,
            bytes,
            line_feed_cnt,
        } => out.push(LeafData::new(*location, *offset, *bytes, *line_feed_cnt)),
    }
}

fn leaves_equal(a: &LeafData, b: &LeafData) -> bool {
    a.location == b.location && a.offset == b.offset && a.bytes == b.bytes
}

fn leaf_slices_equal(a: &[LeafData], b: &[LeafData]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| leaves_equal(x, y))
}

fn sum_bytes(leaves: &[LeafData]) -> usize {
    leaves.iter().map(|leaf| leaf.bytes).sum()
}

fn sum_line_feeds(leaves: &[LeafData]) -> Option<usize> {
    let mut total = 0;
    for leaf in leaves {
        total += leaf.line_feed_cnt?;
    }
    Some(total)
}

fn lines_in_slice(leaves: &[LeafData]) -> Option<usize> {
    if leaves.is_empty() {
        return Some(0);
    }
    let line_feeds = sum_line_feeds(leaves)?;
    Some(line_feeds + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::piece_tree::BufferLocation;

    fn leaf(loc: BufferLocation, offset: usize, bytes: usize, lfs: Option<usize>) -> LeafData {
        LeafData::new(loc, offset, bytes, lfs)
    }

    // Minimal balanced builder for tests.
    fn build(leaves: &[LeafData]) -> Arc<PieceTreeNode> {
        if leaves.is_empty() {
            return Arc::new(PieceTreeNode::Leaf {
                location: BufferLocation::Stored(0),
                offset: 0,
                bytes: 0,
                line_feed_cnt: Some(0),
            });
        }
        if leaves.len() == 1 {
            let l = leaves[0];
            return Arc::new(PieceTreeNode::Leaf {
                location: l.location,
                offset: l.offset,
                bytes: l.bytes,
                line_feed_cnt: l.line_feed_cnt,
            });
        }

        let mid = leaves.len() / 2;
        let left = build(&leaves[..mid]);
        let right = build(&leaves[mid..]);

        Arc::new(PieceTreeNode::Internal {
            left_bytes: sum_bytes(&leaves[..mid]),
            lf_left: sum_line_feeds(&leaves[..mid]),
            left,
            right,
        })
    }

    #[test]
    fn detects_identical_trees() {
        let leaves = vec![leaf(BufferLocation::Stored(0), 0, 10, Some(0))];
        let before = build(&leaves);
        let after = build(&leaves);

        let diff = diff_piece_trees(&before, &after);
        assert!(diff.equal);
        assert_eq!(diff.byte_range, 0..0);
        assert_eq!(diff.line_range, Some(0..0));
    }

    #[test]
    fn detects_single_line_change() {
        let before = build(&[leaf(BufferLocation::Stored(0), 0, 5, Some(0))]);
        let after = build(&[leaf(BufferLocation::Added(1), 0, 5, Some(0))]);

        let diff = diff_piece_trees(&before, &after);
        assert!(!diff.equal);
        assert_eq!(diff.byte_range, 0..5);
        assert_eq!(diff.line_range, Some(0..1)); // same line, different content
    }

    #[test]
    fn tracks_newlines_in_changed_span() {
        let before = build(&[leaf(BufferLocation::Stored(0), 0, 6, Some(0))]);
        let after = build(&[leaf(BufferLocation::Added(1), 0, 6, Some(1))]); // introduces a newline

        let diff = diff_piece_trees(&before, &after);
        assert!(!diff.equal);
        assert_eq!(diff.byte_range, 0..6);
        assert_eq!(diff.line_range, Some(0..2)); // spans two lines after change
    }

    #[test]
    fn handles_deletion_by_marking_anchor_line() {
        let before = build(&[
            leaf(BufferLocation::Stored(0), 0, 6, Some(1)), // two lines
            leaf(BufferLocation::Stored(0), 6, 4, Some(0)), // trailing text
        ]);
        let after = build(&[leaf(BufferLocation::Stored(0), 0, 6, Some(1))]);

        let diff = diff_piece_trees(&before, &after);
        assert!(!diff.equal);
        assert_eq!(diff.byte_range, 6..6); // no bytes remain at the change site
        assert_eq!(diff.line_range, Some(1..2)); // anchor on the line after the removed span
    }
}
