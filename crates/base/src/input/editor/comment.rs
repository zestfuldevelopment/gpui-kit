//! Language-configured comment edits, applied through the ordinary editor history.
use std::ops::Range;

use gpui::{Context, SharedString, Window};
use ropey::Rope;

use crate::input::{EditorState, RopeExt, Selection, ToggleComment, undo_manager::EditIntent};

/// Comment delimiters supplied by a language provider; Base owns only the edits.
#[derive(Clone, Debug)]
pub struct CommentSyntax {
    prefix: SharedString,
    suffix: Option<SharedString>,
    forbidden: Option<SharedString>,
}

impl CommentSyntax {
    pub fn line(prefix: impl Into<SharedString>) -> Self {
        Self {
            prefix: prefix.into(),
            suffix: None,
            forbidden: None,
        }
    }

    pub fn block(prefix: impl Into<SharedString>, suffix: impl Into<SharedString>) -> Self {
        Self {
            prefix: prefix.into(),
            suffix: Some(suffix.into()),
            forbidden: None,
        }
    }

    /// Reject an additional sequence that cannot appear inside a block comment.
    pub fn with_forbidden(mut self, sequence: impl Into<SharedString>) -> Self {
        self.forbidden = Some(sequence.into());
        self
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    pub fn suffix(&self) -> Option<&str> {
        self.suffix.as_deref()
    }
    pub fn forbidden(&self) -> Option<&str> {
        self.forbidden.as_deref()
    }
}

struct Edit {
    range: Range<usize>,
    text: String,
    // Prefix insertions move the caret into the comment; suffixes stay after it.
    move_caret: bool,
}

impl EditorState {
    pub(super) fn toggle_comment(
        &mut self,
        _: &ToggleComment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_editable() || self.ime_marked_range.is_some() {
            return;
        }
        if self.selection_set.has_secondary() {
            self.toggle_multiple_comments(window, cx);
            return;
        }
        let selection = self.selected_range;
        let range = affected_lines(&self.text, selection);
        let syntax = self.mode.highlighter().and_then(|highlighter| {
            highlighter
                .borrow()
                .as_ref()?
                .comment_syntax(&self.text, range.clone())
        });
        let Some(syntax) = syntax else {
            return;
        };
        let source = self.text.slice(range.clone()).to_string();
        let Some(edits) = comment_edits(&source, &syntax) else {
            return;
        };
        let mut replacement = String::with_capacity(source.len());
        let mut end = 0;
        for edit in &edits {
            replacement.push_str(&source[end..edit.range.start]);
            replacement.push_str(&edit.text);
            end = edit.range.end;
        }
        replacement.push_str(&source[end..]);
        if replacement == source {
            return;
        }
        let map = |offset: usize, keep_line_start: bool| {
            range.start + transform_offset(offset - range.start, &edits, keep_line_start)
        };
        let after = Selection::new(
            map(
                selection.start,
                !selection.is_empty() && selection.start == range.start,
            ),
            map(selection.end, false),
        );
        let reversed = self.selection_reversed;
        self.undo_manager.pending_intent = Some(EditIntent::Atomic);
        self.replace_text_in_range_silent(
            Some(self.range_to_utf16(&range)),
            &replacement,
            window,
            cx,
        );
        self.selected_range = after;
        self.selection_reversed = reversed;
        self.update_preferred_column();
        self.undo_manager
            .set_last_selection_after(after, reversed, self.selection_snapshot());
        self.scroll_to(self.cursor(), None, cx);
        cx.notify();
    }
    fn toggle_multiple_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows: Vec<_> = self.selected_logical_lines().into_iter().collect();
        let mut groups: Vec<Range<usize>> = Vec::new();
        for row in rows {
            if let Some(group) = groups.last_mut()
                && group.end == row
            {
                group.end = row + 1;
            } else {
                groups.push(row..row + 1);
            }
        }
        let mut planned = Vec::new();
        for group in groups {
            let range =
                self.text.line_start_offset(group.start)..self.text.line_end_offset(group.end - 1);
            let syntax = self.mode.highlighter().and_then(|h| {
                h.borrow()
                    .as_ref()?
                    .comment_syntax(&self.text, range.clone())
            });
            let Some(syntax) = syntax else {
                continue;
            };
            let source = self.text.slice(range.clone()).to_string();
            let Some(edits) = comment_edits(&source, &syntax) else {
                continue;
            };
            planned.extend(edits.into_iter().map(|edit| {
                crate::input::editor::multi_cursor::SelectionEdit {
                    range: range.start + edit.range.start..range.start + edit.range.end,
                    text: edit.text,
                }
            }));
        }
        self.apply_selection_edits(planned, None, window, cx);
    }
}

fn affected_lines(text: &Rope, selection: Selection) -> Range<usize> {
    let first = text.offset_to_point(selection.start).row;
    let end = text.offset_to_point(selection.end);
    let last = if !selection.is_empty() && end.column == 0 {
        end.row.saturating_sub(1)
    } else {
        end.row
    };
    text.line_start_offset(first)..text.line_end_offset(last)
}

fn transform_offset(offset: usize, edits: &[Edit], keep_line_start: bool) -> usize {
    let mut removed = 0;
    let mut added = 0;
    for edit in edits {
        if offset < edit.range.start {
            break;
        }
        if offset <= edit.range.end {
            let move_right = edit.move_caret && !(keep_line_start && offset == 0);
            return edit.range.start - removed
                + added
                + if move_right { edit.text.len() } else { 0 };
        }
        removed += edit.range.len();
        added += edit.text.len();
    }
    offset - removed + added
}

fn comment_edits(source: &str, syntax: &CommentSyntax) -> Option<Vec<Edit>> {
    let prefix = syntax.prefix();
    if prefix.is_empty()
        || prefix.contains(['\r', '\n'])
        || syntax
            .suffix()
            .is_some_and(|s| s.is_empty() || s.contains(['\r', '\n']))
    {
        return None;
    }
    // Byte offsets into the original span. CR is part of the line ending, never content.
    let mut offset = 0;
    let lines: Vec<_> = source
        .split('\n')
        .map(|line| {
            let content = line.strip_suffix('\r').unwrap_or(line);
            let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
            let end = content.trim_end_matches([' ', '\t']).len().max(indent);
            let result = (offset + indent, offset + end, indent == content.len());
            offset += line.len() + 1;
            result
        })
        .collect();
    let all_blank = lines.iter().all(|(_, _, blank)| *blank);
    let active: Vec<_> = lines
        .into_iter()
        .filter(|(_, _, blank)| all_blank || !blank)
        .collect();
    if let Some(suffix) = syntax.suffix() {
        let start = active.first()?.0;
        let end = active.last()?.1;
        let content = &source[start..end];
        let valid_body = |body: &str| {
            !body.contains(prefix)
                && !body.contains(suffix)
                && !syntax.forbidden().is_some_and(|s| body.contains(s))
        };
        if let Some(body) = content
            .strip_prefix(prefix)
            .and_then(|s| s.strip_suffix(suffix))
        {
            if !valid_body(body) {
                return None;
            }
            let leading = usize::from(body.starts_with(' '));
            let trailing = usize::from(body.len() > leading && body.ends_with(' '));
            return Some(vec![
                Edit {
                    range: start..start + prefix.len() + leading,
                    text: String::new(),
                    move_caret: true,
                },
                Edit {
                    range: end - suffix.len() - trailing..end,
                    text: String::new(),
                    move_caret: false,
                },
            ]);
        }
        if !valid_body(content) {
            return None;
        }
        Some(vec![
            Edit {
                range: start..start,
                text: format!("{prefix} "),
                move_caret: true,
            },
            Edit {
                range: end..end,
                text: format!(" {suffix}"),
                move_caret: false,
            },
        ])
    } else {
        let uncomment = active
            .iter()
            .all(|(start, _, _)| source[*start..].starts_with(prefix));
        Some(
            active
                .into_iter()
                .map(|(start, _, _)| {
                    if uncomment {
                        let end = start + prefix.len();
                        let end = end + usize::from(source[end..].starts_with(' '));
                        Edit {
                            range: start..end,
                            text: String::new(),
                            move_caret: true,
                        }
                    } else {
                        Edit {
                            range: start..start,
                            text: format!("{prefix} "),
                            move_caret: true,
                        }
                    }
                })
                .collect(),
        )
    }
}
