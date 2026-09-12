//! Bounded host-syntax token navigation shared by editor language features.
use gpui_base::input::RopeExt;
use ropey::Rope;
use tree_sitter::Node;

const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_ANCESTORS: usize = 256;
pub(super) const MAX_CURSOR_STEPS: usize = 4096;

/// Retain the path while seeking a token: Node::parent() re-searches from the
/// root, so repeated ancestry queries can otherwise scan a large document.
pub(super) struct Token<'tree> {
    path: Vec<Node<'tree>>,
}

impl<'tree> Token<'tree> {
    pub(super) fn node(&self) -> Node<'tree> {
        *self.path.last().expect("a token includes the root")
    }
    pub(super) fn parent(&self) -> Option<Node<'tree>> {
        self.path.iter().rev().nth(1).copied()
    }
    pub(super) fn parent_of(&self, node: Node<'tree>) -> Option<Node<'tree>> {
        self.path
            .windows(2)
            .find(|pair| pair[1] == node)
            .map(|pair| pair[0])
    }
    pub(super) fn ancestor(&self, predicate: impl Fn(Node<'tree>) -> bool) -> Option<Node<'tree>> {
        self.path
            .iter()
            .rev()
            .copied()
            .find(|node| predicate(*node))
    }
    pub(super) fn is_code(&self) -> bool {
        self.path.iter().all(|node| !protected(node.kind()))
    }
}

pub(super) fn token_before<'tree>(
    root: Node<'tree>,
    text: &Rope,
    end: usize,
) -> Option<Token<'tree>> {
    let token = token_containing(root, text, end)?;
    let node = token.node();
    (node.end_byte() <= end && node.end_byte() - node.start_byte() <= MAX_LINE_BYTES)
        .then_some(token)
}

/// Find the leaf containing a bracket, including compound grammar tokens.
pub(super) fn token_at<'tree>(
    root: Node<'tree>,
    text: &Rope,
    offset: usize,
) -> Option<Token<'tree>> {
    token_containing(root, text, offset.checked_add(1)?)
}

fn token_containing<'tree>(root: Node<'tree>, text: &Rope, end: usize) -> Option<Token<'tree>> {
    let start = text.clip_offset(end.saturating_sub(1), sum_tree::Bias::Left);
    let mut cursor = root.walk();
    let mut path = Vec::new();
    for step in 0..MAX_CURSOR_STEPS {
        if step + 1 == MAX_CURSOR_STEPS {
            return None;
        }
        let node = cursor.node();
        if node.start_byte() <= start && node.end_byte() >= end {
            path.push(node);
            if path.len() > MAX_ANCESTORS {
                return None;
            }
            if cursor.goto_first_child() {
                continue;
            }
            break;
        }
        if node.start_byte() > start || !cursor.goto_next_sibling() {
            break;
        }
    }
    (!path.is_empty()).then_some(Token { path })
}

pub(super) fn protected(kind: &str) -> bool {
    kind.contains("comment")
        || kind.contains("string")
        || matches!(
            kind,
            "char_literal"
                | "character_literal"
                | "regex"
                | "regex_pattern"
                | "heredoc_body"
                | "raw_text"
                | "jsx_text"
                | "plain_scalar"
                | "block_scalar"
        )
}
