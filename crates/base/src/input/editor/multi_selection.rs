use std::ops::Range;

use gpui::{Context, Window};

use crate::input::{EditorSelection, EditorState, grapheme, selection_set::SelectionSnapshot};

impl EditorState {
    /// Set ordered, nonoverlapping editor selections. Invalid identity sets are
    /// rejected unchanged. Ranges expand to grapheme boundaries; carets clip left.
    /// Overlaps retain the primary's identity and direction, or the lowest ID.
    /// Empty collections, duplicate IDs, a missing primary, or active IME are
    /// rejected unchanged. Returns whether the request was accepted, including
    /// an unchanged valid set.
    ///
    /// Ordinary movement, pointer selection, and IME entry collapse to the
    /// primary. Indent/outdent also retain their single-selection behavior;
    /// only the explicit selection-edit methods below edit every selection.
    pub fn set_selections(
        &mut self,
        selections: Vec<EditorSelection>,
        primary_id: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.ime_marked_range.is_some() {
            return false;
        }
        let Some(snapshot) = SelectionSnapshot::normalized(selections, primary_id, &self.text)
        else {
            return false;
        };
        self.undo_manager.break_transaction_coalescing();
        self.restore_selection_snapshot(snapshot);
        self.stop_mouse_selection();
        cx.notify();
        true
    }

    /// The complete selection set in document order, including the primary.
    pub fn selections(&self) -> Vec<EditorSelection> {
        self.selection_snapshot().selections
    }

    /// Stable identity of the selection used by existing caret and geometry APIs.
    pub fn primary_selection_id(&self) -> u64 {
        self.selection_set.primary_id()
    }

    /// Replace every selection with the same text as one undo operation.
    /// Uses the same input normalization as ordinary replacement.
    /// Returns false for no text change, read-only/disabled state, or active IME.
    pub fn replace_selections(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.edit_selections(text, None, window, cx)
    }

    /// Delete selected text or the preceding grapheme at each caret atomically.
    pub fn backspace_selections(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.edit_selections("", Some(false), window, cx)
    }

    /// Delete selected text or the following grapheme at each caret atomically.
    pub fn delete_selections(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.edit_selections("", Some(true), window, cx)
    }

    fn edit_selections(
        &mut self,
        replacement: &str,
        deletion: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_editable() || self.ime_marked_range.is_some() {
            return false;
        }
        // Inherited input options (such as the numeric mask) may change UTF-8
        // byte length. Plan and compare against exactly the text the engine inserts.
        let replacement = self.normalize_input(replacement);
        let replacement = replacement.as_ref();
        let before = self.selection_snapshot();
        // Legacy scalar-range callers may have placed the primary inside a
        // cluster. Interactive multi-selection edits use outward grapheme clipping.
        let normalized =
            SelectionSnapshot::normalized(before.selections.clone(), before.primary_id, &self.text)
                .expect("live selection identities are valid");
        let mut ranges: Vec<Range<usize>> = Vec::with_capacity(normalized.selections.len());
        let mut members = Vec::with_capacity(normalized.selections.len());
        for selection in &normalized.selections {
            let mut range = selection.range();
            if range.is_empty() {
                match deletion {
                    Some(true) => range.end = grapheme::next(&self.text, range.end),
                    Some(false) => range.start = grapheme::previous(&self.text, range.start),
                    None => {}
                }
            }
            members.push((selection.id(), range.start));
            if deletion.is_some()
                && let Some(previous) = ranges.last_mut()
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
                continue;
            }
            ranges.push(range);
        }
        let changed: Vec<_> = ranges
            .iter()
            .filter(|range| {
                !self
                    .text
                    .slice((*range).clone())
                    .chars()
                    .eq(replacement.chars())
            })
            .cloned()
            .collect();
        if changed.is_empty() {
            return false;
        }

        // Map all members, including unchanged replacements and boundary no-ops,
        // against the original rope. One monotone scan avoids quadratic mapping.
        let mut after = Vec::with_capacity(members.len());
        let mut range_index = 0;
        let mut delta: isize = 0;
        for (id, start) in members {
            while range_index < changed.len()
                && changed[range_index].end <= start
                && changed[range_index].start < start
            {
                delta += replacement.len() as isize - changed[range_index].len() as isize;
                range_index += 1;
            }
            let offset = if let Some(range) = changed.get(range_index).filter(|r| r.start <= start)
            {
                range.start.saturating_add_signed(delta) + replacement.len()
            } else {
                start.saturating_add_signed(delta) + replacement.len()
            };
            after.push(EditorSelection::new(id, offset, offset));
        }
        let old_text = self.text.clone();
        self.begin_atomic_edit_batch(Some(before));
        for range in changed.iter().rev() {
            let range_utf16 = self.range_to_utf16(range);
            self.replace_text_in_range_silent(Some(range_utf16), replacement, window, cx);
        }
        let after = SelectionSnapshot::normalized(after, normalized.primary_id, &self.text)
            .expect("planned selections retain primary identity");
        self.undo_manager.set_atomic_selection_after(after);
        self.finish_atomic_edit_batch(old_text, window, cx);
        cx.notify();
        true
    }
}
