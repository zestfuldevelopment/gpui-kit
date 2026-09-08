use std::fmt::Debug;

use crate::{HistoryItem, input::Selection};

#[derive(Debug, PartialEq, Clone)]
pub(super) struct Change {
    pub(crate) old_range: Selection,
    pub(crate) old_text: String,
    pub(crate) new_range: Selection,
    pub(crate) new_text: String,
    pub(crate) selection_before: Selection,
    pub(crate) selection_after: Selection,
    pub(crate) selection_before_reversed: bool,
    pub(crate) selection_after_reversed: bool,
    version: usize,
}

impl Change {
    pub(super) fn with_selection_direction(mut self, reversed: bool) -> Self {
        self.selection_before_reversed = reversed;
        self.selection_after_reversed = reversed;
        self
    }

    pub(super) fn new(
        old_range: impl Into<Selection>,
        old_text: &str,
        new_range: impl Into<Selection>,
        new_text: &str,
        selection_before: Selection,
        selection_after: Selection,
    ) -> Self {
        Self {
            old_range: old_range.into(),
            old_text: old_text.to_string(),
            new_range: new_range.into(),
            new_text: new_text.to_string(),
            selection_before,
            selection_after,
            selection_before_reversed: false,
            selection_after_reversed: false,
            version: 0,
        }
    }
}

impl HistoryItem for Change {
    fn version(&self) -> usize {
        self.version
    }

    fn set_version(&mut self, version: usize) {
        self.version = version;
    }
}
