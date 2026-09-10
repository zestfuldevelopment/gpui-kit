//! Matching stays within the selected token's syntactic owner. Strings,
//! comments, malformed owners and exhausted lookup budgets show no decoration.
use std::ops::Range;

use gpui_base::input::RopeExt;
use ropey::Rope;

use super::{
    SyntaxHighlighter,
    syntax_tokens::{MAX_CURSOR_STEPS, token_at},
};

pub(super) fn matching_brackets(
    highlighter: &SyntaxHighlighter,
    text: &Rope,
    caret: usize,
) -> Option<[Range<usize>; 2]> {
    if matches!(highlighter.language().as_ref(), "text" | "markdown") {
        return None;
    }
    let root = highlighter.tree()?.root_node();
    if root.has_changes() {
        return None;
    }
    // Choose a side before trying to match: an unmatched preferred bracket
    // must not silently highlight the other adjacent bracket's pair instead.
    let start = if text.char_at(caret).is_some_and(is_bracket) {
        caret
    } else if caret > 0 && text.char_at(caret - 1).is_some_and(is_bracket) {
        caret - 1
    } else {
        return None;
    };
    let token = token_at(root, text, start)?;
    let node = token.node();
    if !token.is_code()
        || !structural_token(node.kind())
        || node.end_byte() - node.start_byte() != node.kind().len()
        || !node
            .kind()
            .as_bytes()
            .get(start - node.start_byte())
            .is_some_and(|byte| is_bracket(*byte as char))
    {
        return None;
    }
    let parent = token.parent()?;
    if parent.has_error() {
        return None;
    }
    let mut cursor = parent.walk();
    if !cursor.goto_first_child() {
        return None;
    }
    let mut stack = Vec::new();
    for _ in 0..MAX_CURSOR_STEPS {
        let sibling = cursor.node();
        if sibling.is_missing() {
            return None;
        }
        // Some host grammars combine delimiters with a prefix or another
        // bracket (Bash `$(`, TOML `[[`). Expand only known structural tokens;
        // never scan arbitrary leaf text or enter a literal/injected subtree.
        if structural_token(sibling.kind())
            && sibling.end_byte() - sibling.start_byte() == sibling.kind().len()
        {
            for (index, byte) in sibling.kind().bytes().enumerate() {
                let offset = sibling.start_byte() + index;
                match byte {
                    b'(' | b'[' | b'{' => stack.push((byte, offset)),
                    b')' | b']' | b'}' => {
                        let (opening, opening_offset) = stack.pop()?;
                        if !matches!((opening, byte), (b'(', b')') | (b'[', b']') | (b'{', b'}')) {
                            return None;
                        }
                        if opening_offset == start || offset == start {
                            return Some([opening_offset..opening_offset + 1, offset..offset + 1]);
                        }
                    }
                    _ => {}
                }
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    None
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}')
}

fn structural_token(kind: &str) -> bool {
    matches!(
        kind,
        "(" | ")"
            | "["
            | "]"
            | "{"
            | "}"
            | "$("
            | "$(("
            | "${"
            | "<("
            | ">("
            | "(("
            | "))"
            | "[["
            | "]]"
    )
}

#[cfg(all(test, feature = "tree-sitter-rust"))]
mod tests {
    use super::*;

    fn parse(source: &str) -> (SyntaxHighlighter, Rope) {
        let text = Rope::from(source);
        let mut highlighter = SyntaxHighlighter::new("rust");
        assert!(highlighter.update(None, &text, None));
        (highlighter, text)
    }

    #[test]
    fn nesting_right_precedence_and_invalid_caret_offsets() {
        let source = "fn f() { call([one, (two)]); }";
        let (highlighter, text) = parse(source);
        let start = source.find("[one").unwrap();
        let end = source.find(']').unwrap();
        assert_eq!(
            matching_brackets(&highlighter, &text, start),
            Some([start..start + 1, end..end + 1])
        );
        // The right bracket owns this caret even when that bracket has no pair.
        let source = "fn f() { call(alpha)[";
        let (highlighter, text) = parse(source);
        assert!(matching_brackets(&highlighter, &text, source.len() - 1).is_none());
        let (highlighter, text) = parse("// 🦀\nfn f() {}");
        for caret in [4, 5, 6, text.len() + 1, usize::MAX] {
            assert!(matching_brackets(&highlighter, &text, caret).is_none());
        }
    }

    #[test]
    fn broad_deep_and_wide_owners_exhaust_budgets_without_matching() {
        for (source, caret) in [
            {
                let prefix = "fn f() {}\n".repeat(5_000);
                (format!("{prefix}fn main() {{value();}}"), prefix.len() + 10)
            },
            {
                let prefix = format!("fn f() {{{}", "if true {".repeat(300));
                (
                    format!("{prefix}call(alpha);{}}}", "}".repeat(300)),
                    prefix.len() + 4,
                )
            },
            (format!("fn f() {{ call({}); }}", "one,".repeat(5_000)), 13),
        ] {
            let (highlighter, text) = parse(&source);
            assert!(matching_brackets(&highlighter, &text, caret).is_none());
        }
    }
}
