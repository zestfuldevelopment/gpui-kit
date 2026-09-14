//! Editor gestures and commands planned against one immutable rope revision.
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    ops::Range,
};

use gpui::{Context, Window};
use unicode_segmentation::UnicodeSegmentation;

use crate::input::{
    EditorSelection, EditorState, RopeExt, SelectAllOccurrences, SelectNextOccurrence, grapheme,
    selection_set::SelectionSnapshot,
};

pub(in crate::input) struct SelectionEdit {
    pub(in crate::input) range: Range<usize>,
    pub(in crate::input) text: String,
}

impl EditorState {
    pub(in crate::input) fn toggle_caret(
        &mut self,
        offset: usize,
        affinity: bool,
        cx: &mut Context<Self>,
    ) {
        if self.disabled || self.ime_marked_range.is_some() {
            return;
        }
        let offset = grapheme::floor(&self.text, offset);
        let mut selections = self.selections();
        let mut primary = self.primary_selection_id();
        if let Some(ix) = selections
            .iter()
            .position(|s| s.range().is_empty() && s.head() == offset)
        {
            if selections.len() == 1 {
                self.stop_mouse_selection();
                return;
            }
            if selections.remove(ix).id() == primary {
                primary = selections[0].id();
            }
        } else {
            let id = next_selection_id(&selections);
            selections
                .push(EditorSelection::new(id, offset, offset).with_line_end_affinity(affinity));
        }
        self.set_selections(selections, primary, cx);
        self.pause_blink_cursor(cx);
    }

    pub(in crate::input) fn select_next_occurrence(
        &mut self,
        _: &SelectNextOccurrence,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_occurrences(false, cx);
    }

    pub(in crate::input) fn select_all_occurrences(
        &mut self,
        _: &SelectAllOccurrences,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_occurrences(true, cx);
    }

    fn select_occurrences(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.disabled || self.ime_marked_range.is_some() {
            return;
        }
        let mut selections = self.selections();
        let primary = self.primary_selection_id();
        let ix = selections.iter().position(|s| s.id() == primary).unwrap();
        let source = self.text.to_string();
        let mut range = selections[ix].range();
        if range.is_empty() {
            let offset = range.start;
            let Some((start, word)) = source
                .unicode_word_indices()
                .find(|(start, word)| *start <= offset && offset < *start + word.len())
                .or_else(|| {
                    source
                        .unicode_word_indices()
                        .find(|(start, word)| *start + word.len() == offset)
                })
            else {
                return;
            };
            range = start..start + word.len();
            selections[ix] = EditorSelection::new(primary, range.start, range.end);
            if !all {
                self.set_selections(selections, primary, cx);
                return;
            }
        }
        let query = &source[range.clone()];
        if query.is_empty() {
            return;
        }
        // Enumerate overlapping literal candidates too: an existing selection
        // can begin between the matches of a globally nonoverlapping search.
        let Ok(matcher) = aho_corasick::AhoCorasick::new([query]) else {
            return;
        };
        let matches: Vec<_> = matcher
            .find_overlapping_iter(source.as_bytes())
            .map(|found| found.start()..found.end())
            .filter(|range| {
                grapheme::floor(&self.text, range.start) == range.start
                    && grapheme::ceil(&self.text, range.end) == range.end
            })
            .collect();
        let after = selections
            .iter()
            .filter(|s| source[s.range()] == *query)
            .map(|s| s.range().end)
            .max()
            .unwrap_or(range.end);
        let candidates = matches
            .iter()
            .filter(|r| r.start >= after)
            .chain(matches.iter().filter(|r| r.start < after));
        let mut occupied: BTreeMap<usize, usize> = selections
            .iter()
            .map(|s| (s.range().start, s.range().end))
            .collect();
        let mut identities: HashSet<u64> = selections.iter().map(EditorSelection::id).collect();
        let mut id = next_selection_id(&selections);
        let mut revealed = Vec::new();
        for candidate in candidates {
            let overlaps =
                occupied
                    .range(..candidate.end)
                    .next_back()
                    .is_some_and(|(&start, &end)| {
                        end > candidate.start || start == end && end == candidate.start
                    })
                    || occupied
                        .get(&candidate.end)
                        .is_some_and(|end| *end == candidate.end);
            if overlaps {
                continue;
            }
            selections.push(EditorSelection::new(id, candidate.start, candidate.end));
            identities.insert(id);
            id = id.wrapping_add(1);
            while identities.contains(&id) {
                id = id.wrapping_add(1);
            }
            occupied.insert(candidate.start, candidate.end);
            revealed.push(candidate.clone());
            if !all {
                break;
            }
        }
        if self.set_selections(selections, primary, cx) {
            let reveal_offset = revealed.last().map(|range| range.start);
            for range in revealed {
                let start = self.text.offset_to_point(range.start).row;
                let end = self.text.offset_to_point(range.end.saturating_sub(1)).row;
                let folds: Vec<_> = self
                    .display_map
                    .folded_ranges()
                    .iter()
                    .filter(|fold| start < fold.end_line && end > fold.start_line)
                    .map(|fold| fold.start_line)
                    .collect();
                for row in folds {
                    self.display_map.set_folded(row, false);
                }
            }
            if !all && let Some(offset) = reveal_offset {
                self.scroll_to(offset, None, cx);
            }
            cx.notify();
        }
    }

    pub(in crate::input) fn replace_multiple(
        &mut self,
        text: &str,
        typed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !typed || text != "}" {
            self.replace_selections(text, window, cx);
            return;
        }
        let selections = self.selections();
        let mut edits = Vec::with_capacity(selections.len());
        let mut carets = Vec::with_capacity(selections.len());
        for (ix, selection) in selections.iter().enumerate() {
            let mut range = selection.range();
            let mut replacement = self.normalize_input(text).into_owned();
            if range.is_empty() {
                let row = self.text.offset_to_point(range.start).row;
                let start = self.text.line_start_offset(row);
                let end = crate::input::rope_ext::clip_crlf_offset(
                    &self.text,
                    self.text.line_end_offset(row),
                );
                if range.start <= end
                    && range.start - start <= 16 * 1024
                    && end - range.start <= 16 * 1024
                    && self
                        .text
                        .slice(start..end)
                        .chars()
                        .all(|c| matches!(c, ' ' | '\t'))
                    && (ix == 0 || selections[ix - 1].range().end < start)
                {
                    let indent = self.mode.highlighter().and_then(|h| {
                        h.borrow()
                            .as_ref()?
                            .closing_brace_indent(&self.text, range.start)
                    });
                    if let Some(indent) = indent.filter(|indent| {
                        indent.len() < range.start - start
                            && indent.chars().all(|c| matches!(c, ' ' | '\t'))
                            && self
                                .text
                                .slice(start..start + indent.len())
                                .chars()
                                .eq(indent.chars())
                    }) {
                        range.start = start;
                        replacement = self.normalize_input(&format!("{indent}}}")).into_owned();
                    }
                }
            }
            carets.push((selection.id(), range.start, replacement.len()));
            edits.push(SelectionEdit {
                range,
                text: replacement,
            });
        }
        self.apply_selection_edits(edits, Some(carets), window, cx);
    }

    pub(in crate::input) fn newline_multiple(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selections = self.selections();
        let first_break = self.text.line_end_offset(0);
        let line_break = if first_break < self.text.len()
            && self.text.char_at(first_break.saturating_sub(1)) == Some('\r')
        {
            "\r\n"
        } else {
            "\n"
        };
        let mut edits = Vec::with_capacity(selections.len());
        let mut carets = Vec::with_capacity(selections.len());
        for (ix, selection) in selections.iter().enumerate() {
            let mut range = selection.range();
            let plan = self.mode.highlighter().and_then(|h| {
                h.borrow().as_ref()?.newline_indent(
                    &self.text,
                    range.clone(),
                    &self.mode.tab_size().to_string(),
                )
            });
            let row = self.text.offset_to_point(range.start).row;
            let indent: String = self
                .text
                .slice_line(row)
                .chars()
                .take_while(|c| matches!(c, ' ' | '\t'))
                .collect();
            let mut text = format!(
                "{line_break}{}",
                plan.as_ref().map(|p| p.indent()).unwrap_or(&indent)
            );
            let caret = self.normalize_input(&text).len();
            if let Some(closing) = plan.as_ref().and_then(|p| p.closing_indent()) {
                let limit = selections
                    .get(ix + 1)
                    .map(|s| s.range().start)
                    .unwrap_or(self.text.len());
                range.end += self
                    .text
                    .slice(range.end..limit)
                    .chars()
                    .take_while(|c| matches!(c, ' ' | '\t'))
                    .count();
                text.push_str(line_break);
                text.push_str(closing);
            }
            carets.push((selection.id(), range.start, caret));
            edits.push(SelectionEdit {
                range,
                text: self.normalize_input(&text).into_owned(),
            });
        }
        self.apply_selection_edits(edits, Some(carets), window, cx);
    }

    pub(in crate::input) fn selected_logical_lines(&self) -> BTreeSet<usize> {
        let mut rows = BTreeSet::new();
        for selection in self.selections() {
            let range = selection.range();
            let first = self.text.offset_to_point(range.start).row;
            let end = self.text.offset_to_point(range.end);
            let last = if !range.is_empty() && end.column == 0 {
                end.row.saturating_sub(1)
            } else {
                end.row
            };
            rows.extend(first..=last);
        }
        rows
    }

    pub(in crate::input) fn indent_multiple(
        &mut self,
        outdent: bool,
        block: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let indent = self.mode.tab_size().to_string();
        if !outdent && !block && self.selections().iter().all(|s| s.range().is_empty()) {
            self.replace_selections(&indent, window, cx);
            return;
        }
        let edits = self
            .selected_logical_lines()
            .into_iter()
            .filter_map(|row| {
                let start = self.text.line_start_offset(row);
                if outdent {
                    self.text
                        .slice_line(row)
                        .chars()
                        .take(indent.chars().count())
                        .eq(indent.chars())
                        .then(|| SelectionEdit {
                            range: start..start + indent.len(),
                            text: String::new(),
                        })
                } else {
                    Some(SelectionEdit {
                        range: start..start,
                        text: indent.to_string(),
                    })
                }
            })
            .collect();
        self.apply_selection_edits(edits, None, window, cx);
    }

    /// Apply localized, nonoverlapping edits descending; map every member from the original revision.
    pub(in crate::input) fn apply_selection_edits(
        &mut self,
        edits: Vec<SelectionEdit>,
        carets: Option<Vec<(u64, usize, usize)>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_editable() || self.ime_marked_range.is_some() {
            return false;
        }
        let before = self.selection_snapshot();
        let mut edits: Vec<_> = edits
            .into_iter()
            .map(|edit| SelectionEdit {
                text: self.normalize_input(&edit.text).into_owned(),
                ..edit
            })
            .filter(|edit| {
                !self
                    .text
                    .slice(edit.range.clone())
                    .chars()
                    .eq(edit.text.chars())
            })
            .collect();
        edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
        if edits.is_empty() {
            return false;
        }
        debug_assert!(
            edits
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
        );
        let mut deltas = Vec::with_capacity(edits.len() + 1);
        deltas.push(0isize);
        for edit in &edits {
            deltas.push(
                deltas.last().unwrap() + edit.text.len() as isize - edit.range.len() as isize,
            );
        }
        let map = |offset: usize, right: bool| {
            // A prior edit ending here contributes its full delta even for a
            // left-affinity endpoint belonging to the next adjacent edit.
            let ix =
                edits.partition_point(|edit| edit.range.end <= offset && edit.range.start < offset);
            let delta = deltas[ix];
            if let Some(edit) = edits.get(ix).filter(|edit| edit.range.start <= offset) {
                edit.range.start.saturating_add_signed(delta)
                    + if right { edit.text.len() } else { 0 }
            } else {
                offset.saturating_add_signed(delta)
            }
        };
        let after = if let Some(carets) = carets {
            carets
                .into_iter()
                .map(|(id, start, local)| {
                    let end = map(start, false) + local;
                    EditorSelection::new(id, end, end)
                })
                .collect()
        } else {
            before
                .selections
                .iter()
                .map(|s| {
                    EditorSelection::new(s.id(), map(s.anchor(), true), map(s.head(), true))
                        .with_line_end_affinity(s.line_end_affinity())
                        .with_preferred_column(s.preferred_column())
                })
                .collect()
        };
        let old_text = self.text.clone();
        self.begin_atomic_edit_batch(Some(before.clone()));
        for edit in edits.iter().rev() {
            self.replace_text_in_range_silent(
                Some(self.range_to_utf16(&edit.range)),
                &edit.text,
                window,
                cx,
            );
        }
        let after = SelectionSnapshot::normalized(after, before.primary_id, &self.text)
            .expect("planned identities remain valid");
        self.undo_manager.set_atomic_selection_after(after);
        self.finish_atomic_edit_batch(old_text, window, cx);
        self.pause_blink_cursor(cx);
        cx.notify();
        true
    }
}

fn next_selection_id(selections: &[EditorSelection]) -> u64 {
    let mut id = selections
        .iter()
        .map(EditorSelection::id)
        .max()
        .unwrap_or(0)
        .wrapping_add(1);
    while selections.iter().any(|s| s.id() == id) {
        id = id.wrapping_add(1);
    }
    id
}
