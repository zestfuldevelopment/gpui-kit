//! Selection values and transaction snapshots. The legacy primary fields remain
//! authoritative; live storage contains only the other selections and primary ID.
use std::collections::HashSet;

use gpui::Pixels;
use ropey::Rope;

use super::{InputBaseState, InputModeKind, Selection, grapheme};

/// An editor selection with a stable identity and UTF-8 anchor/head offsets.
///
/// The head is the moving caret; an anchor greater than the head is reversed.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorSelection {
    id: u64,
    anchor: usize,
    head: usize,
    line_end_affinity: bool,
    preferred_column: Option<(Pixels, usize)>,
}

impl EditorSelection {
    pub fn new(id: u64, anchor: usize, head: usize) -> Self {
        Self {
            id,
            anchor,
            head,
            line_end_affinity: false,
            preferred_column: None,
        }
    }

    pub fn with_line_end_affinity(mut self, affinity: bool) -> Self {
        self.line_end_affinity = affinity;
        self
    }

    pub fn with_preferred_column(mut self, column: Option<(Pixels, usize)>) -> Self {
        self.preferred_column = column;
        self
    }

    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn anchor(&self) -> usize {
        self.anchor
    }
    pub fn head(&self) -> usize {
        self.head
    }
    pub fn line_end_affinity(&self) -> bool {
        self.line_end_affinity
    }
    pub fn preferred_column(&self) -> Option<(Pixels, usize)> {
        self.preferred_column
    }

    pub(super) fn range(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

#[derive(Debug, Default)]
pub(super) struct SelectionSet {
    primary_id: u64,
    secondary: Vec<EditorSelection>,
}

impl SelectionSet {
    pub(super) fn has_secondary(&self) -> bool {
        !self.secondary.is_empty()
    }
    pub(super) fn primary_id(&self) -> u64 {
        self.primary_id
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SelectionSnapshot {
    pub(super) selections: Vec<EditorSelection>,
    pub(super) primary_id: u64,
}

impl SelectionSnapshot {
    /// Normalize once, in document order. Touching nonempty ranges stay distinct;
    /// carets at either edge are contained in that range.
    pub(super) fn normalized(
        mut selections: Vec<EditorSelection>,
        primary_id: u64,
        text: &Rope,
    ) -> Option<Self> {
        let mut ids = HashSet::with_capacity(selections.len());
        if selections.is_empty()
            || selections.iter().any(|s| !ids.insert(s.id))
            || !ids.contains(&primary_id)
        {
            return None;
        }
        for selection in &mut selections {
            let range = selection.range();
            let start = grapheme::floor(text, range.start);
            let end = if range.is_empty() {
                start
            } else {
                grapheme::ceil(text, range.end)
            };
            if selection.anchor > selection.head {
                selection.anchor = end;
                selection.head = start;
            } else {
                selection.anchor = start;
                selection.head = end;
            }
        }
        selections.sort_by_key(|s| (s.range().start, s.range().end, s.id));
        let mut merged: Vec<EditorSelection> = Vec::with_capacity(selections.len());
        for selection in selections {
            if let Some(previous) = merged.last_mut() {
                let a = previous.range();
                let b = selection.range();
                if b.start < a.end || (b.start == a.end && (a.is_empty() || b.is_empty())) {
                    let end = a.end.max(b.end);
                    if selection.id == primary_id
                        || (previous.id != primary_id && selection.id < previous.id)
                    {
                        *previous = selection;
                    }
                    let reversed = previous.anchor > previous.head;
                    previous.anchor = if reversed { end } else { a.start };
                    previous.head = if reversed { a.start } else { end };
                    continue;
                }
            }
            merged.push(selection);
        }
        Some(Self {
            selections: merged,
            primary_id,
        })
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub(super) fn selection_snapshot(&self) -> SelectionSnapshot {
        let (anchor, head) = if self.selection_reversed {
            (self.selected_range.end, self.selected_range.start)
        } else {
            (self.selected_range.start, self.selected_range.end)
        };
        let primary = EditorSelection::new(self.selection_set.primary_id, anchor, head)
            .with_line_end_affinity(self.cursor_line_end_affinity)
            .with_preferred_column(self.preferred_column);
        let mut selections = self.selection_set.secondary.clone();
        let index = selections.partition_point(|s| {
            (s.range().start, s.range().end, s.id)
                < (primary.range().start, primary.range().end, primary.id)
        });
        selections.insert(index, primary);
        SelectionSnapshot {
            selections,
            primary_id: self.selection_set.primary_id,
        }
    }

    pub(super) fn restore_selection_snapshot(&mut self, snapshot: SelectionSnapshot) {
        self.selection_set.primary_id = snapshot.primary_id;
        self.selection_set.secondary.clear();
        for selection in snapshot.selections {
            if selection.id == snapshot.primary_id {
                self.selected_range = Selection::from(selection.range());
                self.selection_reversed = selection.anchor > selection.head;
                self.cursor_line_end_affinity = selection.line_end_affinity;
                self.preferred_column = selection.preferred_column;
            } else {
                self.selection_set.secondary.push(selection);
            }
        }
    }

    /// Ordinary editing, movement, and IME retain their single-selection contract.
    pub(super) fn collapse_secondary_selections(&mut self) {
        if !self.selection_set.secondary.is_empty() {
            self.selection_set.secondary.clear();
            self.undo_manager.break_transaction_coalescing();
        }
    }
}
