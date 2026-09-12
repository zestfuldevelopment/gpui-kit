//! Conservative Enter rules over the current syntax tree. No parsing on Enter.
use std::ops::Range;

use gpui_base::input::{NewlineIndent, RopeExt};
use ropey::Rope;
use tree_sitter::Node;

use super::{
    SyntaxHighlighter,
    syntax_tokens::{Token, token_before},
};

// Limit copies and ancestry walks for pathological lines / deeply nested syntax.
const MAX_LINE_BYTES: usize = 16 * 1024;

pub(super) fn newline_indent(
    highlighter: &SyntaxHighlighter,
    text: &Rope,
    selection: Range<usize>,
    unit: &str,
) -> Option<NewlineIndent> {
    let root = highlighter.tree()?.root_node();
    if root.has_changes() {
        return None;
    }
    let language = highlighter.language().as_ref();
    if !matches!(
        language,
        "rust"
            | "javascript"
            | "typescript"
            | "tsx"
            | "python"
            | "json"
            | "css"
            | "toml"
            | "yaml"
            | "bash"
            | "html"
    ) {
        return None;
    }
    let line_start = text.line_start_offset(text.offset_to_point(selection.start).row);
    let line_end = text.line_end_offset(text.offset_to_point(selection.end).row);
    if selection.start - line_start > MAX_LINE_BYTES || line_end - selection.end > MAX_LINE_BYTES {
        return None;
    }
    let prefix = text.slice(line_start..selection.start).to_string();
    let mut indent: String = prefix
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect();
    let mut end =
        selection.start - (prefix.len() - prefix.trim_end_matches([' ', '\t', '\r']).len());
    if end == line_start {
        return None;
    }
    let mut token = token_before(root, text, end)?;
    // A trailing line comment does not hide a preceding structural opener.
    if let Some(comment) = token.ancestor(|n| is_comment(n.kind())) {
        if comment.start_byte() < line_start
            || comment.end_byte() > selection.start
            || comment.end_byte() - comment.start_byte() > MAX_LINE_BYTES
        {
            return None;
        }
        let comment_text = text.slice(comment.byte_range()).to_string();
        if !comment_text.starts_with("//") && !comment_text.starts_with('#') {
            return None;
        }
        end = comment.start_byte();
        while end > line_start && matches!(text.char_at(end - 1), Some(' ' | '\t')) {
            end -= 1;
        }
        if end == line_start {
            return None;
        }
        token = token_before(root, text, end)?;
    }
    // If the ancestry budget is exhausted, uncertainty also means inherit.
    if !token.is_code() {
        return None;
    }
    let before = text.slice(line_start..end).to_string();
    let suffix = text.slice(selection.end..line_end).to_string();
    let suffix = suffix.trim_start_matches([' ', '\t']);

    let closer = match token.node().kind() {
        "{" if matches!(
            language,
            "rust"
                | "javascript"
                | "typescript"
                | "tsx"
                | "python"
                | "json"
                | "css"
                | "yaml"
                | "toml"
        ) =>
        {
            Some("}".to_owned())
        }
        "[" if matches!(
            language,
            "rust" | "javascript" | "typescript" | "tsx" | "python" | "json" | "yaml"
        ) =>
        {
            Some("]".to_owned())
        }
        "[" if language == "toml"
            && (token.ancestor(|n| n.kind() == "array").is_some() || before.contains('=')) =>
        {
            Some("]".to_owned())
        }
        "(" if matches!(
            language,
            "rust" | "javascript" | "typescript" | "tsx" | "python" | "css"
        ) =>
        {
            Some(")".to_owned())
        }
        "{" if language == "bash"
            && token
                .ancestor(|n| matches!(n.kind(), "expansion" | "simple_expansion"))
                .is_none() =>
        {
            Some("}".to_owned())
        }
        ":" if language == "python" && python_suite(&token) => Some(String::new()),
        ":" if language == "yaml"
            && token
                .ancestor(|n| n.kind() == "block_mapping_pair")
                .is_some() =>
        {
            Some(String::new())
        }
        "then" | "do" | "else" if language == "bash" => Some(String::new()),
        "in" if language == "bash" => Some(String::new()),
        ">" if matches!(language, "html" | "tsx") => {
            opening_tag(&token, text, language).and_then(|name| {
                let tag =
                    token.ancestor(|n| matches!(n.kind(), "start_tag" | "jsx_opening_element"))?;
                indent = line_indent(text, tag.start_byte())?;
                Some(format!("</{name}>"))
            })
        }
        _ => None,
    };
    if let Some(closer) = closer {
        let unit = if indent.contains('\t') { "\t" } else { unit };
        let plan = NewlineIndent::new(format!("{indent}{unit}"));
        return Some(
            if !closer.is_empty()
                && (suffix.starts_with(&closer)
                    || language == "html"
                        && suffix
                            .get(..closer.len())
                            .is_some_and(|value| value.eq_ignore_ascii_case(&closer)))
                && closer_is_code(root, text, line_end - suffix.len())
            {
                plan.with_closing_indent(indent)
            } else {
                plan
            },
        );
    }

    // Only a closing-only line can change the next line's inherited depth.
    // Its existing whitespace is left untouched. Use the containing syntax
    // node's opener rather than guessing how many columns a tab represents.
    let content = before.trim();
    let close = content.trim_end_matches([';', ',']);
    if matches!(close, "}" | "]" | ")") {
        let close_end = line_start + indent.len() + close.len();
        let closing_token = token_before(root, text, close_end)?;
        if closing_token.node().kind() != close {
            return None;
        }
        let parent = closing_token.parent()?;
        let first = parent.child(0)?;
        let expected = match close {
            "}" => "{",
            "]" => "[",
            _ => "(",
        };
        if first.kind() == expected && first.start_byte() < line_start {
            return line_indent(text, first.start_byte()).map(NewlineIndent::new);
        }
    }
    if matches!(language, "html" | "tsx") && content.starts_with("</") && content.ends_with('>') {
        let tag = token.ancestor(|n| matches!(n.kind(), "end_tag" | "jsx_closing_element"))?;
        if tag.start_byte() == line_start + indent.len() && tag.end_byte() == end {
            let element = token.parent_of(tag)?;
            if element.start_byte() < line_start {
                return line_indent(text, element.start_byte()).map(NewlineIndent::new);
            }
        }
    }
    None
}

/// Find an unmatched structural brace in the current host tree. Traverse once,
/// skipping literals and comments; every cursor movement consumes the same
/// finite budget. Missing parser closers do not balance actual opening tokens.
pub(super) fn closing_brace_indent(
    highlighter: &SyntaxHighlighter,
    text: &Rope,
    caret: usize,
) -> Option<String> {
    use super::syntax_tokens::{MAX_CURSOR_STEPS, protected};

    if !matches!(
        highlighter.language().as_ref(),
        "rust" | "javascript" | "typescript" | "tsx" | "python" | "json" | "css" | "toml" | "yaml"
    ) {
        return None;
    }
    let root = highlighter.tree()?.root_node();
    if root.has_changes() || caret > text.len() {
        return None;
    }
    let line_start = text.line_start_offset(text.offset_to_point(caret).row);
    let mut cursor = root.walk();
    let mut stack = Vec::new();
    let mut budget = MAX_CURSOR_STEPS;
    let mut gap_budget = MAX_LINE_BYTES;
    let mut covered = 0;
    'walk: loop {
        budget = budget.checked_sub(1)?;
        let node = cursor.node();
        if node.start_byte() >= caret {
            break;
        }
        let literal = protected(node.kind());
        // Completed named syntax before this line cannot leave a delimiter
        // open. Skip whole earlier functions/statements and valid lifetimes,
        // rather than spending the budget descending unrelated syntax.
        let complete = node.is_named() && !node.has_error() && node.end_byte() <= line_start;
        if literal && node.start_byte() < caret && node.end_byte() >= caret {
            return None;
        }
        if !node.is_missing() && (literal || complete || node.child_count() == 0) {
            whitespace_gap(text, covered, node.start_byte(), &mut gap_budget)?;
            covered = node.end_byte();
            // Incomplete literals can lose their named wrapper in ERROR nodes.
            // Unrepresented text is rejected by whitespace_gap as well (e.g.
            // Rust's incomplete raw-string prefix is absent from the tree).
            if !literal && !complete && matches!(node.kind(), "\"" | "'" | "`" | "/*" | "//") {
                return None;
            }
        }
        if !literal && !complete && !node.is_missing() {
            if node.child_count() > 0 {
                if cursor.goto_first_child() {
                    continue;
                }
            } else if node.end_byte() <= caret && node.end_byte() - node.start_byte() == 1 {
                match node.kind() {
                    "{" | "[" | "(" => stack.push((node.kind(), node.start_byte())),
                    "}" | "]" | ")" => {
                        let (opening, _) = stack.pop()?;
                        if !matches!((opening, node.kind()), ("{", "}") | ("[", "]") | ("(", ")")) {
                            return None;
                        }
                    }
                    _ => {}
                }
            }
        }
        loop {
            budget = budget.checked_sub(1)?;
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    whitespace_gap(text, covered, caret, &mut gap_budget)?;
    let (opening, offset) = stack.last()?;
    (*opening == "{" && *offset < line_start)
        .then(|| line_indent(text, *offset))
        .flatten()
}

fn whitespace_gap(text: &Rope, start: usize, end: usize, budget: &mut usize) -> Option<()> {
    *budget = budget.checked_sub(end.checked_sub(start)?)?;
    text.slice(start..end)
        .chars()
        .all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
        .then_some(())
}

fn closer_is_code(root: Node<'_>, text: &Rope, start: usize) -> bool {
    let end = start
        + if text.char_at(start) == Some('<') {
            2
        } else {
            1
        };
    token_before(root, text, end).is_some_and(|token| token.is_code())
}

fn is_comment(kind: &str) -> bool {
    kind.contains("comment")
}

fn python_suite(token: &Token<'_>) -> bool {
    token.parent().is_some_and(|n| {
        matches!(
            n.kind(),
            "if_statement"
                | "elif_clause"
                | "else_clause"
                | "for_statement"
                | "while_statement"
                | "function_definition"
                | "class_definition"
                | "try_statement"
                | "except_clause"
                | "finally_clause"
                | "with_statement"
                | "match_statement"
                | "case_clause"
        )
    })
}

fn opening_tag(token: &Token<'_>, text: &Rope, language: &str) -> Option<String> {
    let tag = token.ancestor(|n| matches!(n.kind(), "start_tag" | "jsx_opening_element"))?;
    if tag.end_byte() - tag.start_byte() > MAX_LINE_BYTES {
        return None;
    }
    let source = text.slice(tag.byte_range()).to_string();
    if source.ends_with("/>") {
        return None;
    }
    let name: String = source
        .strip_prefix('<')?
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        .collect();
    if name.is_empty() {
        return None;
    }
    let void_name = if language == "html" {
        name.to_ascii_lowercase()
    } else {
        name.clone()
    };
    if matches!(
        void_name.as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    ) {
        return None;
    }
    Some(name)
}

fn line_indent(text: &Rope, offset: usize) -> Option<String> {
    let start = text.line_start_offset(text.offset_to_point(offset).row);
    if offset - start > MAX_LINE_BYTES {
        return None;
    }
    Some(
        text.slice(start..offset)
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect(),
    )
}

#[cfg(all(test, feature = "tree-sitter-rust"))]
mod tests {
    use super::*;

    #[test]
    fn typed_closer_exhausts_syntax_and_gap_budgets_conservatively() {
        for source in [
            format!("{}fn f() {{\n  ", "fn earlier() {}\n".repeat(5_000)),
            format!("fn f() {{\n{}  ", "if true {\n".repeat(5_000)),
            format!("fn f() {{{}  ", "\n".repeat(MAX_LINE_BYTES + 1)),
        ] {
            let text = Rope::from(source.as_str());
            let mut highlighter = SyntaxHighlighter::new("rust");
            assert!(highlighter.update(None, &text, None));
            assert!(closing_brace_indent(&highlighter, &text, text.len()).is_none());
        }
    }

    #[test]
    fn broad_and_deep_syntax_exhausts_lookup_budget_conservatively() {
        for source in [
            format!("{}fn main() {{", "fn f() {}\n".repeat(5_000)),
            format!("fn f() {{\n{}", "if true {\n".repeat(300)),
        ] {
            let text = Rope::from(source.as_str());
            let mut highlighter = SyntaxHighlighter::new("rust");
            assert!(highlighter.update(None, &text, None));
            assert!(newline_indent(&highlighter, &text, text.len()..text.len(), "  ").is_none());
        }
    }
}
