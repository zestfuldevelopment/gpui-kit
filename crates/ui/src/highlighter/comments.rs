//! Host-language comment configuration. Embedded syntax is deliberately conservative.
use super::{SyntaxHighlighter, syntax_tokens::MAX_CURSOR_STEPS};
use gpui_base::input::CommentSyntax;
use std::ops::Range;

pub(super) fn syntax(
    highlighter: &SyntaxHighlighter,
    current: bool,
    range: Range<usize>,
) -> Option<CommentSyntax> {
    Some(match highlighter.language().as_ref() {
        "rust" | "javascript" | "typescript" => CommentSyntax::line("//"),
        "tsx" => {
            if !current
                || !host_range(highlighter, range, true, |node, _| {
                    node.kind().starts_with("jsx_")
                })
            {
                return None;
            }
            CommentSyntax::line("//")
        }
        "python" | "bash" | "toml" | "yaml" => CommentSyntax::line("#"),
        "css" => CommentSyntax::block("/*", "*/"),
        "html" | "markdown" => {
            if !current
                || !host_range(highlighter, range.clone(), false, |node, remaining| {
                    matches!(
                        node.kind(),
                        "raw_text"
                            | "fenced_code_block"
                            | "indented_code_block"
                            | "minus_metadata"
                            | "plus_metadata"
                    ) || (node.kind() == "html_block"
                        && !complete_html_comment(highlighter, node, &range))
                        || markdown_metadata_intersects(node, &range, remaining)
                })
            {
                return None;
            }
            CommentSyntax::block("<!--", "-->").with_forbidden("--")
        }
        _ => return None,
    })
}

// Markdown parses a standalone HTML comment as an injected html_block. Permit
// a complete wrapper so comment-first toggles can be undone, but never an
// interior line or another raw HTML block (which may contain scripts/styles).
fn complete_html_comment(
    highlighter: &SyntaxHighlighter,
    node: tree_sitter::Node,
    range: &Range<usize>,
) -> bool {
    let start = node.start_byte();
    let end = node.end_byte();
    let text = highlighter.text();
    range.start <= start
        && end.saturating_sub(range.end) <= 2
        && (end <= range.end
            || text
                .slice(range.end..end)
                .chars()
                .all(|c| matches!(c, '\r' | '\n')))
        && end >= start + 4
        && text
            .slice(start..end)
            .bytes()
            .take(4)
            .eq(b"<!--".iter().copied())
}

// The query permits multiple named siblings between the opening/closing
// thematic breaks. Share the host walk's budget rather than starting a second
// unbounded scan for every section.
fn markdown_metadata_intersects(
    node: tree_sitter::Node,
    range: &Range<usize>,
    remaining: &mut usize,
) -> bool {
    if node.kind() != "section" {
        return false;
    }
    let mut cursor = node.walk();
    if !cursor.goto_first_child() || cursor.node().kind() != "thematic_break" {
        return false;
    }
    let mut intersects = false;
    while *remaining > 0 {
        *remaining -= 1;
        if !cursor.goto_next_sibling() {
            return false;
        }
        let child = cursor.node();
        if !child.is_named() {
            continue;
        }
        if child.kind() == "thematic_break" && intersects {
            return true;
        }
        intersects |= child.start_byte() <= range.end && child.end_byte() >= range.start;
    }
    true
}

fn host_range(
    highlighter: &SyntaxHighlighter,
    range: Range<usize>,
    require_valid: bool,
    excluded: impl Fn(tree_sitter::Node, &mut usize) -> bool,
) -> bool {
    let Some(tree) = highlighter.tree() else {
        return false;
    };
    let root = tree.root_node();
    if (require_valid && root.has_error()) || root.has_changes() {
        return false;
    }
    let mut cursor = root.walk();
    let mut ascending = false;
    let mut remaining = MAX_CURSOR_STEPS;
    while remaining > 0 {
        remaining -= 1;
        if ascending {
            if !cursor.goto_parent() {
                return true;
            }
            if cursor.goto_next_sibling() {
                ascending = false;
            }
            continue;
        }
        let node = cursor.node();
        if node.start_byte() > range.end {
            return true;
        }
        if node.end_byte() >= range.start {
            if excluded(node, &mut remaining) || remaining == 0 {
                return false;
            }
            if cursor.goto_first_child() {
                continue;
            }
        }
        if !cursor.goto_next_sibling() {
            ascending = true;
        }
    }
    false
}

#[cfg(all(test, feature = "tree-sitter-tsx"))]
mod tests {
    use super::*;
    use ropey::Rope;

    fn parse(source: &str) -> SyntaxHighlighter {
        let mut highlighter = SyntaxHighlighter::new("tsx");
        assert!(highlighter.update(None, &Rope::from(source), None));
        highlighter
    }

    #[test]
    fn pending_syntax_does_not_guess_a_comment_kind() {
        let source = "const value = 1;";
        let highlighter = parse(source);
        assert!(syntax(&highlighter, true, 0..source.len()).is_some());
        assert!(syntax(&highlighter, false, 0..source.len()).is_none());
        assert!(syntax(&SyntaxHighlighter::new("tsx"), true, 0..0).is_none());
    }

    #[test]
    fn traversal_budget_exhaustion_leaves_the_target_unchanged() {
        let source = "const value = 1;\n".repeat(5_000);
        let highlighter = parse(&source);
        assert!(syntax(&highlighter, true, 0..source.len()).is_none());
        assert!(syntax(&highlighter, true, source.len() - 17..source.len()).is_none());
    }
}
