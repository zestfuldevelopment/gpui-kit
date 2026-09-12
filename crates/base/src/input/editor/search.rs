use crate::input::InputModeKind;
use gpui::{Context, Window};
use regex::{Regex, RegexBuilder};
use ropey::Rope;
use std::{ops::Range, rc::Rc, sync::LazyLock};

use super::{InputBaseState, NextSearchMatch, PreviousSearchMatch, Replace, RopeExt as _, Search};

/// Stateful, presentation-independent search engine used by text inputs.
#[derive(Debug, Clone)]
pub struct SearchMatcher {
    text: Rope,
    pub query: Option<Regex>,
    query_text: String,
    case_insensitive: bool,
    whole_word: bool,
    error: Option<String>,
    matched_ranges: Rc<Vec<Range<usize>>>,
    current_match_ix: usize,
    replacing: bool,
}

#[derive(Debug, Clone)]
pub struct SearchSession {
    pub open: bool,
    pub replace_mode: bool,
    pub case_insensitive: bool,
    pub whole_word: bool,
    /// Changes only when an explicit Find/Replace command requests focus.
    pub focus_revision: u64,
    pub query: String,
    pub replacement: String,
    pub anchor_offset: Option<usize>,
    pub matcher: SearchMatcher,
}

impl Default for SearchSession {
    fn default() -> Self {
        Self {
            open: false,
            replace_mode: false,
            case_insensitive: true,
            whole_word: false,
            focus_revision: 0,
            query: String::new(),
            replacement: String::new(),
            anchor_offset: None,
            matcher: SearchMatcher::new(),
        }
    }
}

impl SearchSession {
    pub(crate) fn open(&mut self, replace_mode: bool, replaceable: bool) {
        self.open = true;
        self.focus_revision = self.focus_revision.wrapping_add(1);
        self.replace_mode = replace_mode && replaceable;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    pub(crate) fn update_query(&mut self, query: impl Into<String>, case_insensitive: bool) {
        self.query = query.into();
        self.case_insensitive = case_insensitive;
        self.matcher.update_query(&self.query, case_insensitive);
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub fn open_search(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        if !self.searchable {
            return;
        }
        self.search_session
            .open(replace_mode, self.is_replaceable());
        let selected = self.selected_text().to_string();
        if !selected.is_empty() {
            self.search_session.query = selected;
        }
        self.search_session.anchor_offset = Some(self.selected_range.start);
        self.search_session.matcher.update_query(
            &self.search_session.query,
            self.search_session.case_insensitive,
        );
        self.search_session.matcher.update(&self.text);
        if let Some(anchor) = self.search_session.anchor_offset {
            self.search_session.matcher.update_cursor_by_offset(anchor);
        }
        self.reveal_search_match(cx);
        cx.notify();
    }

    pub fn search_session(&self) -> &SearchSession {
        &self.search_session
    }

    #[doc(hidden)]
    pub fn set_search_replace_mode(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        self.search_session.replace_mode = replace_mode && self.is_replaceable();
        cx.notify();
    }

    /// Returns true if the search panel can replace the matches.
    ///
    /// This is false when the input is not `replaceable`, or when it is
    /// `disabled` or `readonly`.
    pub fn is_replaceable(&self) -> bool {
        self.replaceable && self.is_editable()
    }

    pub fn set_search_query(
        &mut self,
        query: impl Into<String>,
        case_insensitive: bool,
        cx: &mut Context<Self>,
    ) {
        let query = query.into();
        if self.search_session.query == query
            && self.search_session.case_insensitive == case_insensitive
        {
            return;
        }
        self.search_session.update_query(query, case_insensitive);
        self.search_session.matcher.update(&self.text);
        self.anchor_search_match(cx);
        cx.notify();
    }

    pub fn set_search_whole_word(&mut self, whole_word: bool, cx: &mut Context<Self>) {
        if self.search_session.whole_word == whole_word {
            return;
        }
        self.search_session.whole_word = whole_word;
        self.search_session.matcher.set_whole_word(whole_word);
        self.anchor_search_match(cx);
        cx.notify();
    }

    pub fn set_search_replacement(
        &mut self,
        replacement: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let replacement = replacement.into();
        if self.search_session.replacement != replacement {
            self.search_session.replacement = replacement;
            cx.notify();
        }
    }

    fn anchor_search_match(&mut self, cx: &mut Context<Self>) {
        let anchor = self
            .search_session
            .anchor_offset
            .unwrap_or(self.selected_range.start);
        self.search_session.matcher.update_cursor_by_offset(anchor);
        if self.search_session.open {
            self.reveal_search_match(cx);
        }
    }

    fn current_search_match(&self) -> Option<Range<usize>> {
        self.search_session
            .matcher
            .matched_ranges
            .get(self.search_session.matcher.current_match_index())
            .cloned()
    }

    fn reveal_search_match(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self.current_search_match() else {
            return;
        };
        let start = self.text.offset_to_point(range.start).row;
        let end = self.text.offset_to_point(range.end.saturating_sub(1)).row;
        let covering: Vec<_> = self
            .display_map
            .folded_ranges()
            .iter()
            .filter(|fold| start < fold.end_line && end > fold.start_line)
            .map(|fold| fold.start_line)
            .collect();
        for line in covering {
            self.display_map.set_folded(line, false);
        }
        self.scroll_to(range.start, None, cx);
        cx.notify();
    }

    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        if !self.search_session.open {
            return;
        }
        if let Some(range) = self.current_search_match() {
            self.reveal_search_match(cx);
            self.set_selected_range(range, cx);
        }
        self.search_session.close();
        cx.notify();
    }

    pub fn next_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let range = self.search_session.matcher.next()?;
        self.reveal_search_match(cx);
        if !self.search_session.open {
            self.set_selected_range(range.clone(), cx);
        }
        Some(range)
    }

    pub fn previous_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let range = self.search_session.matcher.next_back()?;
        self.reveal_search_match(cx);
        if !self.search_session.open {
            self.set_selected_range(range.clone(), cx);
        }
        Some(range)
    }

    pub fn replace_current_search_match(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_replaceable() {
            return false;
        }
        let Some(range) = self.current_search_match() else {
            return false;
        };
        self.set_search_replacement(replacement, cx);
        // Anchor beyond inserted text so a replacement containing the query
        // cannot repeatedly replace its own newly inserted first match.
        let old_len = self.text.len();
        if self.text.slice(range.clone()).to_string() != replacement {
            self.search_session.matcher.begin_replacement();
            self.undo_manager.pending_intent = Some(crate::input::undo_manager::EditIntent::Atomic);
            let range_utf16 = self.range_to_utf16(&range);
            self.replace_text_in_range_silent(Some(range_utf16), replacement, window, cx);
        }
        let inserted_end = range
            .end
            .saturating_add_signed(self.text.len() as isize - old_len as isize);
        let matcher = &mut self.search_session.matcher;
        let next = matcher
            .matched_ranges
            .iter()
            .position(|candidate| candidate.start >= inserted_end)
            .unwrap_or(0);
        matcher.set_current_match_index(next);
        self.reveal_search_match(cx);
        cx.notify();
        true
    }

    pub fn replace_all_search_matches(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        if !self.is_replaceable() {
            return 0;
        }
        let ranges = self.search_session.matcher.matched_ranges();
        if ranges.is_empty() {
            return 0;
        }
        self.set_search_replacement(replacement, cx);
        let old_text = self.text.clone();
        self.undo_manager.begin_atomic_batch();
        // Reverse order keeps each saved byte range valid. Each edit uses the
        // normal UTF-16 input/history path and adjusts only overlapping folds.
        for range in ranges.iter().rev() {
            if self.text.slice(range.clone()).to_string() == replacement {
                continue;
            }
            self.undo_manager.pending_intent = Some(crate::input::undo_manager::EditIntent::Atomic);
            let range_utf16 = self.range_to_utf16(range);
            self.replace_text_in_range_silent(Some(range_utf16), replacement, window, cx);
        }
        self.finish_atomic_edit_batch(old_text, window, cx);
        self.reveal_search_match(cx);
        cx.notify();
        ranges.len()
    }

    pub(super) fn update_search(&mut self, _cx: &mut gpui::App) {
        if !self.undo_manager.is_atomic_batch() {
            self.search_session.matcher.update(&self.text);
        }
    }

    pub(super) fn on_action_next_search_match(
        &mut self,
        _: &NextSearchMatch,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.searchable {
            self.next_search_match(cx);
        }
    }

    pub(super) fn on_action_previous_search_match(
        &mut self,
        _: &PreviousSearchMatch,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.searchable {
            self.previous_search_match(cx);
        }
    }

    pub(super) fn on_action_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        if !self.searchable {
            return;
        }
        self.open_search(false, cx);
    }

    pub(super) fn on_action_replace(
        &mut self,
        _: &Replace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.searchable {
            return;
        }
        self.open_search(true, cx);
    }
}

impl Default for SearchMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchMatcher {
    pub fn new() -> Self {
        Self {
            text: "".into(),
            query: None,
            query_text: String::new(),
            case_insensitive: false,
            whole_word: false,
            error: None,
            matched_ranges: Rc::new(Vec::new()),
            current_match_ix: 0,
            replacing: false,
        }
    }

    /// Update the source text and recompute matches.
    pub fn update(&mut self, text: &Rope) {
        if self.text.eq(text) {
            self.replacing = false;
            return;
        }
        self.text = text.clone();
        self.update_matches();
    }

    pub fn update_query(&mut self, query: &str, case_insensitive: bool) {
        if self.query_text == query && self.case_insensitive == case_insensitive {
            return;
        }
        self.query_text = query.to_owned();
        self.case_insensitive = case_insensitive;
        self.error = None;
        self.query = if query.is_empty() {
            None
        } else {
            match RegexBuilder::new(&regex::escape(query))
                .case_insensitive(case_insensitive)
                .unicode(true)
                .build()
            {
                Ok(query) => Some(query),
                Err(error) => {
                    self.error = Some(format!("Search query is too large: {error}"));
                    None
                }
            }
        };
        self.update_matches();
    }

    /// Require no Unicode word character immediately outside either end of
    /// the literal, including punctuation-containing literals. Underscores,
    /// letters, numbers, combining marks and join controls are word characters.
    pub fn set_whole_word(&mut self, whole_word: bool) {
        if self.whole_word == whole_word {
            return;
        }
        self.whole_word = whole_word;
        self.update_matches();
    }

    /// A failed query compilation clears matches and exposes a displayable error.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn matched_ranges(&self) -> Rc<Vec<Range<usize>>> {
        self.matched_ranges.clone()
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match_ix
    }

    pub fn len(&self) -> usize {
        self.matched_ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matched_ranges.is_empty()
    }

    pub fn label(&self) -> String {
        if self.is_empty() {
            "0/0".into()
        } else {
            format!("{}/{}", self.current_match_ix + 1, self.len())
        }
    }

    fn has_next_without_wrap(&self) -> bool {
        self.current_match_ix < self.matched_ranges.len().saturating_sub(1)
    }

    pub fn update_cursor_by_offset(&mut self, offset: usize) {
        self.current_match_ix = self
            .matched_ranges
            .iter()
            .position(|range| range.contains(&offset) || range.start >= offset)
            .unwrap_or(0);
    }

    /// Preserve the current logical match while a replacement mutates text.
    fn begin_replacement(&mut self) {
        self.replacing = true;
    }

    fn set_current_match_index(&mut self, index: usize) {
        self.current_match_ix = index.min(self.matched_ranges.len().saturating_sub(1));
    }

    fn next_index(&self) -> Option<usize> {
        if self.is_empty() {
            None
        } else if self.has_next_without_wrap() {
            Some(self.current_match_ix + 1)
        } else {
            Some(0)
        }
    }

    fn update_matches(&mut self) {
        let mut ranges = Vec::new();
        if let Some(query) = &self.query {
            let text = self.text.to_string();
            ranges.extend(
                query
                    .find_iter(&text)
                    .map(|result| result.range())
                    .filter(|range| !self.whole_word || whole_word_range(&text, range)),
            );
        }
        self.matched_ranges = Rc::new(ranges);
        if !self.replacing || self.is_empty() {
            self.current_match_ix = 0;
        } else {
            self.current_match_ix = self.current_match_ix.min(self.len() - 1);
        }
        self.replacing = false;
    }
}

fn whole_word_range(text: &str, range: &Range<usize>) -> bool {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w").expect("valid word class"));
    let is_word = |ch: char| {
        if ch.is_ascii() {
            ch.is_ascii_alphanumeric() || ch == '_'
        } else {
            WORD.is_match(ch.encode_utf8(&mut [0; 4]))
        }
    };
    !text[..range.start].chars().next_back().is_some_and(is_word)
        && !text[range.end..].chars().next().is_some_and(is_word)
}

impl Iterator for SearchMatcher {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let ix = self.next_index()?;
        self.current_match_ix = ix;
        self.matched_ranges.get(ix).cloned()
    }
}

impl DoubleEndedIterator for SearchMatcher {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        if self.current_match_ix == 0 {
            self.current_match_ix = self.len();
        }
        self.current_match_ix -= 1;
        self.matched_ranges.get(self.current_match_ix).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_word_excludes_unicode_word_neighbors() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("café CAFÉ caféine _café café_"));
        matcher.update_query("café", true);
        matcher.set_whole_word(true);
        assert_eq!(&*matcher.matched_ranges(), &[0..5, 6..11]);
        matcher.update(&Rope::from("a.b aXb xa.b a.b_ a.b\u{301} a.b"));
        matcher.update_query("a.b", true);
        assert_eq!(&*matcher.matched_ranges(), &[0..3, 24..27]);
        matcher.update(&Rope::from("(foo) x(foo) (foo)_ (foo)"));
        matcher.update_query("(foo)", false);
        assert_eq!(&*matcher.matched_ranges(), &[0..5, 20..25]);
        matcher.update(&Rope::from("é e\u{301}"));
        matcher.update_query("é", true);
        assert_eq!(&*matcher.matched_ranges(), &[0..2]);
    }

    #[test]
    fn unchanged_query_options_and_text_keep_matches() {
        let mut matcher = SearchMatcher::new();
        let text = Rope::from("foo foo");
        matcher.update(&text);
        matcher.update_query("foo", true);
        matcher.next();
        let ranges = matcher.matched_ranges();
        matcher.update_query("foo", true);
        matcher.set_whole_word(false);
        matcher.update(&text);
        assert!(Rc::ptr_eq(&ranges, &matcher.matched_ranges()));
        assert_eq!(matcher.current_match_index(), 1);
    }

    #[test]
    fn oversized_query_reports_error_and_recovers() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo"));
        matcher.update_query(&"Σ".repeat(400_000), true);
        assert!(matcher.error().is_some());
        assert!(matcher.is_empty());
        matcher.update_query("foo", true);
        assert!(matcher.error().is_none());
        assert_eq!(&*matcher.matched_ranges(), &[0..3]);
    }

    #[test]
    #[ignore = "manual representative matcher timing"]
    fn search_matcher_representative_timing() {
        use std::time::Instant;
        for lines in [10_000, 50_000] {
            let text = Rope::from("let café = CAFÉ + caféine; // Σσς a.b\r\n".repeat(lines));
            let mut matcher = SearchMatcher::new();
            matcher.update(&text);
            for query in ["café", "σ", "missing"] {
                let started = Instant::now();
                matcher.update_query(query, true);
                let query_time = started.elapsed();
                let started = Instant::now();
                matcher.set_whole_word(true);
                let word_time = started.elapsed();
                let ranges = matcher.matched_ranges();
                let started = Instant::now();
                for _ in 0..100 {
                    matcher.update_query(query, true);
                    matcher.update(&text);
                    matcher.set_whole_word(true);
                }
                eprintln!(
                    "{lines} lines, {query}: compile+scan {query_time:?}, whole-word {word_time:?}, 100 unchanged {:?}, {} matches",
                    started.elapsed(),
                    matcher.len()
                );
                assert!(Rc::ptr_eq(&ranges, &matcher.matched_ranges()));
                matcher.set_whole_word(false);
            }
        }
    }

    #[test]
    fn unicode_literal_search() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("café CAFÉ caféine _café café_"));
        matcher.update_query("café", true);
        assert_eq!(
            &*matcher.matched_ranges(),
            &[0..5, 6..11, 12..17, 22..27, 28..33]
        );
        matcher.update(&Rope::from("Σσς"));
        matcher.update_query("σ", true);
        assert_eq!(&*matcher.matched_ranges(), &[0..2, 2..4, 4..6]);
        matcher.update(&Rope::from("a.b aXb"));
        matcher.update_query("a.b", true);
        assert_eq!(&*matcher.matched_ranges(), &[0..3]);
        matcher.update_query("", true);
        assert!(matcher.is_empty());
    }

    #[test]
    fn finds_navigates_and_preserves_replacement_position() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo FOO foo"));
        matcher.update_query("foo", true);
        assert_eq!(&*matcher.matched_ranges(), &[0..3, 4..7, 8..11]);
        assert_eq!(matcher.next(), Some(4..7));
        assert_eq!(matcher.next_back(), Some(0..3));

        matcher.set_current_match_index(2);
        matcher.begin_replacement();
        matcher.update(&Rope::from("foo FOO bar"));
        assert_eq!(matcher.current_match_index(), 1);
    }

    #[test]
    fn next_wraps_to_start() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(".....aaaaa.....aaaaa.....aaaaa"));
        matcher.update_query("aaaaa", false);
        matcher.set_current_match_index(2);
        assert_eq!(matcher.next(), Some(5..10));
    }

    #[test]
    fn replacement_keeps_current_match_index_on_next_match() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo foo foo"));
        matcher.update_query("foo", true);
        assert_eq!(matcher.label(), "1/3");

        assert!(matcher.has_next_without_wrap());
        matcher.begin_replacement();
        matcher.update(&Rope::from("bar foo foo"));
        assert_eq!(matcher.current_match_index(), 0);
        assert_eq!(matcher.matched_ranges()[0], 4..7);
        assert_eq!(matcher.label(), "1/2");

        matcher.set_current_match_index(1);
        assert!(!matcher.has_next_without_wrap());
        matcher.set_current_match_index(0);
        matcher.begin_replacement();
        matcher.update(&Rope::from("bar foo bar"));
        assert_eq!(matcher.current_match_index(), 0);
        assert_eq!(matcher.matched_ranges()[0], 4..7);
        assert_eq!(matcher.label(), "1/1");
    }

    #[test]
    fn update_matches_clamps_current_match_index_while_replacing() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo foo foo"));
        matcher.update_query("foo", true);
        matcher.set_current_match_index(2);
        matcher.begin_replacement();

        matcher.update(&Rope::from("foo xoo foo"));

        assert_eq!(matcher.len(), 2);
        assert_eq!(matcher.current_match_index(), 1);
        assert_eq!(matcher.label(), "2/2");
    }
}
