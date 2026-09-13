use crate::input::{change::Change, selection_set::SelectionSnapshot};

const MAX_UNDO_TRANSACTIONS: usize = 1000;
const MAX_CHANGES_PER_TRANSACTION: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditIntent {
    Typing,
    Backspace,
    DeleteForward,
    Atomic,
}

#[derive(Debug)]
struct UndoTransaction {
    intent: EditIntent,
    changes: Vec<Change>,
    selections: Option<(SelectionSnapshot, SelectionSnapshot)>,
}

#[derive(Debug)]
struct AtomicBatch {
    changes: Vec<Change>,
    selection_before: Option<SelectionSnapshot>,
    selection_after: Option<SelectionSnapshot>,
}

/// Coordinates undo and redo as explicit editing transactions.
///
/// Each edit first creates a transaction. Compatible adjacent transactions
/// may then coalesce until an explicit boundary is encountered. Callers that
/// perform one logical edit through several callbacks (currently IME
/// composition) bracket those changes with `begin_transaction` and
/// `commit_transaction`.
#[derive(Debug)]
pub(crate) struct UndoManager {
    undo_transactions: Vec<UndoTransaction>,
    redo_transactions: Vec<UndoTransaction>,
    replay_selection: Option<SelectionSnapshot>,
    ignoring: bool,
    transaction_open: bool,
    pending_change: Option<Change>,
    pending_selections: Option<(SelectionSnapshot, SelectionSnapshot)>,
    atomic_batch: Option<AtomicBatch>,
    pub(crate) pending_intent: Option<EditIntent>,
    coalescing_boundary: bool,
}

impl UndoManager {
    pub(super) fn new() -> Self {
        Self {
            undo_transactions: Vec::new(),
            redo_transactions: Vec::new(),
            replay_selection: None,
            ignoring: false,
            transaction_open: false,
            pending_change: None,
            pending_selections: None,
            atomic_batch: None,
            pending_intent: None,
            coalescing_boundary: false,
        }
    }

    pub(super) fn record_transaction(
        &mut self,
        change: Change,
        intent: EditIntent,
        selections: Option<(SelectionSnapshot, SelectionSnapshot)>,
    ) {
        if self.ignoring {
            return;
        }
        if let Some(batch) = self.atomic_batch.as_mut() {
            if change.old_range != change.new_range || change.old_text != change.new_text {
                batch.changes.push(change);
            }
        } else if self.transaction_open {
            merge_selection_snapshots(&mut self.pending_selections, selections);
            // Identical IME callbacks still belong to the open composition.
            // Committing the transaction discards any net-zero change.
            if let Some(pending) = self.pending_change.as_mut() {
                pending.new_range = change.new_range;
                pending.new_text = change.new_text;
                pending.selection_after = change.selection_after;
                pending.selection_after_reversed = change.selection_after_reversed;
            } else {
                self.pending_change = Some(change);
            }
        } else if change.old_range == change.new_range && change.old_text == change.new_text {
            self.break_transaction_coalescing();
        } else {
            self.push_transaction(change, intent, selections);
        }
    }

    /// Group disjoint replacements without the IME transaction's single-range
    /// merging. Every change retains its normal selection and byte boundaries.
    pub(super) fn begin_atomic_batch(&mut self, selection_before: Option<SelectionSnapshot>) {
        self.commit_transaction();
        self.coalescing_boundary = true;
        self.atomic_batch = Some(AtomicBatch {
            changes: Vec::new(),
            selection_before,
            selection_after: None,
        });
    }

    pub(crate) fn is_atomic_batch(&self) -> bool {
        self.atomic_batch.is_some()
    }

    pub(super) fn discard_atomic_batch(&mut self) -> Option<SelectionSnapshot> {
        self.atomic_batch
            .take()
            .and_then(|batch| batch.selection_before)
    }

    pub(super) fn commit_atomic_batch(&mut self, selection_after: Option<SelectionSnapshot>) {
        let Some(batch) = self.atomic_batch.take() else {
            return;
        };
        if batch.changes.is_empty() {
            return;
        }
        self.redo_transactions.clear();
        if self.undo_transactions.len() >= MAX_UNDO_TRANSACTIONS {
            self.undo_transactions.remove(0);
        }
        self.undo_transactions.push(UndoTransaction {
            intent: EditIntent::Atomic,
            changes: batch.changes,
            selections: batch.selection_before.zip(selection_after),
        });
        self.coalescing_boundary = true;
    }

    /// Record the final selection after a completed, nonempty command adjusts
    /// its caret. Full transaction snapshots and legacy history stay in sync.
    pub(super) fn set_last_selection_after(
        &mut self,
        selection: crate::input::Selection,
        reversed: bool,
        after: SelectionSnapshot,
    ) {
        if self.ignoring {
            return;
        }
        if let Some(transaction) = self.undo_transactions.last_mut() {
            if let Some(change) = transaction.changes.last_mut() {
                change.selection_after = selection;
                change.selection_after_reversed = reversed;
            }
            if let Some((_, selection_after)) = transaction.selections.as_mut() {
                *selection_after = after;
            }
        }
    }

    /// Supply the planned final editor selections while the batch is still open.
    pub(super) fn set_atomic_selection_after(&mut self, after: SelectionSnapshot) {
        if let Some(batch) = self.atomic_batch.as_mut() {
            batch.selection_after = Some(after);
        }
    }

    pub(super) fn take_atomic_selection_after(&mut self) -> Option<SelectionSnapshot> {
        self.atomic_batch.as_mut()?.selection_after.take()
    }

    pub(super) fn take_replay_selection(&mut self) -> Option<SelectionSnapshot> {
        self.replay_selection.take()
    }

    pub(super) fn begin_transaction(&mut self) {
        if self.transaction_open {
            return;
        }
        self.transaction_open = true;
        self.pending_change = None;
        self.pending_selections = None;
    }

    pub(super) fn commit_transaction(&mut self) {
        if !self.transaction_open {
            return;
        }
        self.transaction_open = false;
        let selections = self.pending_selections.take();
        if let Some(change) = self.pending_change.take()
            && (change.old_range != change.new_range || change.old_text != change.new_text)
        {
            self.push_transaction(change, EditIntent::Atomic, selections);
        }
    }

    fn push_transaction(
        &mut self,
        change: Change,
        intent: EditIntent,
        selections: Option<(SelectionSnapshot, SelectionSnapshot)>,
    ) {
        self.redo_transactions.clear();
        let can_coalesce = !self.coalescing_boundary
            && intent != EditIntent::Atomic
            && self.undo_transactions.last().is_some_and(|previous| {
                previous.intent == intent
                    && previous.changes.len() < MAX_CHANGES_PER_TRANSACTION
                    && previous
                        .changes
                        .last()
                        .is_some_and(|last| is_adjacent(intent, last, &change))
            });

        if can_coalesce {
            let transaction = self
                .undo_transactions
                .last_mut()
                .expect("coalescing requires a previous transaction");
            transaction.changes.push(change);
            merge_selection_snapshots(&mut transaction.selections, selections);
            return;
        }

        if self.undo_transactions.len() >= MAX_UNDO_TRANSACTIONS {
            self.undo_transactions.remove(0);
        }
        self.undo_transactions.push(UndoTransaction {
            intent,
            changes: vec![change],
            selections,
        });
        self.coalescing_boundary = intent == EditIntent::Atomic;
    }

    pub(super) fn break_transaction_coalescing(&mut self) {
        self.commit_transaction();
        self.coalescing_boundary = true;
    }

    pub(super) fn is_ignoring(&self) -> bool {
        self.ignoring
    }

    pub(super) fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
        if ignoring {
            self.commit_transaction();
        }
    }

    pub(super) fn clear(&mut self) {
        self.replay_selection = None;
        self.undo_transactions.clear();
        self.redo_transactions.clear();
        self.transaction_open = false;
        self.pending_change = None;
        self.pending_selections = None;
        self.atomic_batch = None;
        self.pending_intent = None;
        self.coalescing_boundary = false;
    }

    pub(super) fn undo(&mut self) -> Option<Vec<Change>> {
        self.commit_transaction();
        self.replay_selection = None;
        let transaction = self.undo_transactions.pop()?;
        self.replay_selection = transaction
            .selections
            .as_ref()
            .map(|(before, _)| before.clone());
        let changes = transaction.changes.iter().rev().cloned().collect();
        self.redo_transactions.push(transaction);
        self.coalescing_boundary = true;
        Some(changes)
    }

    pub(super) fn redo(&mut self) -> Option<Vec<Change>> {
        self.commit_transaction();
        self.replay_selection = None;
        let transaction = self.redo_transactions.pop()?;
        self.replay_selection = transaction
            .selections
            .as_ref()
            .map(|(_, after)| after.clone());
        let changes = transaction.changes.clone();
        self.undo_transactions.push(transaction);
        self.coalescing_boundary = true;
        Some(changes)
    }

    #[cfg(test)]
    pub(super) fn has_undos(&self) -> bool {
        !self.undo_transactions.is_empty()
    }
}

/// Coalescing and IME retain the original selection and the latest result.
fn merge_selection_snapshots(
    target: &mut Option<(SelectionSnapshot, SelectionSnapshot)>,
    next: Option<(SelectionSnapshot, SelectionSnapshot)>,
) {
    if let Some((before, after)) = next {
        if let Some((_, previous_after)) = target.as_mut() {
            *previous_after = after;
        } else {
            *target = Some((before, after));
        }
    }
}

fn is_adjacent(intent: EditIntent, previous: &Change, current: &Change) -> bool {
    match intent {
        EditIntent::Typing => {
            previous.old_range.is_empty()
                && current.old_range.is_empty()
                && !previous.new_text.contains(['\n', '\r'])
                && !current.new_text.contains(['\n', '\r'])
                && previous.new_range.end == current.old_range.start
        }
        EditIntent::Backspace => {
            previous.new_text.is_empty()
                && current.new_text.is_empty()
                && current.old_range.end == previous.old_range.start
        }
        EditIntent::DeleteForward => {
            previous.new_text.is_empty()
                && current.new_text.is_empty()
                && current.old_range.start == previous.old_range.start
        }
        EditIntent::Atomic => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Selection;

    fn typing_change(offset: usize, text: &str) -> Change {
        let end = offset + text.len();
        Change::new(
            offset..offset,
            "",
            offset..end,
            text,
            Selection::new(offset, offset),
            Selection::new(end, end),
        )
    }

    #[test]
    fn adjacent_typing_transactions_coalesce() {
        let mut manager = UndoManager::new();
        manager.record_transaction(typing_change(0, "a"), EditIntent::Typing, None);
        manager.record_transaction(typing_change(1, "b"), EditIntent::Typing, None);

        assert_eq!(manager.undo().unwrap().len(), 2);
        assert!(manager.undo().is_none());
    }

    #[test]
    fn explicit_transaction_collects_multiple_changes() {
        let mut manager = UndoManager::new();
        manager.begin_transaction();
        manager.record_transaction(typing_change(0, "a"), EditIntent::Typing, None);
        manager.record_transaction(typing_change(0, "ab"), EditIntent::Typing, None);
        manager.commit_transaction();

        let transaction = manager.undo().unwrap();
        assert_eq!(transaction.len(), 1);
        assert_eq!(transaction[0].new_text, "ab");
    }

    #[test]
    fn limits_the_number_of_retained_transactions() {
        let mut manager = UndoManager::new();

        for offset in 0..1_100 {
            manager.record_transaction(typing_change(offset, "a"), EditIntent::Atomic, None);
        }

        for _ in 0..MAX_UNDO_TRANSACTIONS {
            assert!(manager.undo().is_some());
        }
        assert!(manager.undo().is_none());
    }

    #[test]
    fn splits_a_coalesced_transaction_before_its_change_list_grows_too_large() {
        let mut manager = UndoManager::new();

        for offset in 0..1_100 {
            manager.record_transaction(typing_change(offset, "a"), EditIntent::Typing, None);
        }

        assert_eq!(manager.undo().unwrap().len(), 100);
        assert_eq!(manager.undo().unwrap().len(), MAX_CHANGES_PER_TRANSACTION);
        assert!(manager.undo().is_none());
    }
}
