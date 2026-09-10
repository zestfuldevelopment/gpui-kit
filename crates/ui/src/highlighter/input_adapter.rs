use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gpui::{HighlightStyle, SharedString, Task};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit as BaseInputEdit, InputHighlighter,
    InputHighlighterFactory, NewlineIndent,
};
use ropey::Rope;
use tree_sitter::{InputEdit, ParseOptions, Parser, Point};

use super::{LanguageRegistry, SyntaxHighlighter};

pub(crate) fn input_highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        let config = LanguageRegistry::singleton().language(language)?;
        config.has_grammar().then(|| {
            Box::new(TreeSitterInputHighlighter::new(language)) as Box<dyn InputHighlighter>
        })
    })
}

struct TreeSitterInputHighlighter {
    inner: Rc<RefCell<SyntaxHighlighter>>,
    parse_task: Rc<RefCell<Option<Task<()>>>>,
    syntax_current: Rc<Cell<bool>>,
}

impl TreeSitterInputHighlighter {
    fn new(language: &str) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SyntaxHighlighter::new(language))),
            parse_task: Rc::new(RefCell::new(None)),
            syntax_current: Rc::new(Cell::new(false)),
        }
    }
}

impl SyntaxHighlighter {
    pub(crate) fn update_input(
        &mut self,
        edit: Option<BaseInputEdit>,
        text: &Rope,
        timeout: Option<Duration>,
    ) -> bool {
        self.update(edit.map(to_tree_sitter_edit), text, timeout)
    }
}

impl InputHighlighter for TreeSitterInputHighlighter {
    fn language(&self) -> SharedString {
        self.inner.borrow().language().clone()
    }

    fn update(
        &mut self,
        edit: Option<BaseInputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        const SYNC_PARSE_TIMEOUT: Duration = Duration::from_millis(2);
        const SYNC_PARSE_MAX_BYTES: usize = 256 * 1024;
        const PARSE_DEBOUNCE: Duration = Duration::from_millis(150);

        let changed = !self.inner.borrow().text().eq(text);
        let was_current = self.syntax_current.get();
        let edit = edit.map(to_tree_sitter_edit);
        let completed = {
            let mut highlighter = self.inner.borrow_mut();
            if text.len() > SYNC_PARSE_MAX_BYTES {
                highlighter.edit_tree(edit, text);
                false
            } else {
                highlighter.update(edit, text, Some(SYNC_PARSE_TIMEOUT))
            }
        };
        self.syntax_current
            .set(completed && (changed || was_current));
        if self.syntax_current.get() {
            self.parse_task.borrow_mut().take();
            return;
        }

        let highlighter = self.inner.clone();
        let syntax_current = self.syntax_current.clone();
        let parse_task = self.parse_task.clone();
        let language = highlighter.borrow().language().clone();
        let old_tree = highlighter.borrow().tree().cloned();
        let injection_data = highlighter.borrow().injection_parse_data();
        let text = text.clone();
        let text_for_apply = text.clone();
        let cancel = Arc::new(AtomicBool::new(false));

        let task = cx.spawn_in(window, async move |entity, cx| {
            struct CancelOnDrop(Arc<AtomicBool>);
            impl Drop for CancelOnDrop {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _cancel_guard = CancelOnDrop(cancel.clone());
            cx.background_executor().timer(PARSE_DEBOUNCE).await;

            let parse_cancel = cancel.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let config = LanguageRegistry::singleton().language(&language)?;
                    let grammar = config.language.as_ref()?;
                    let mut parser = Parser::new();
                    parser.set_language(grammar).ok()?;
                    let mut progress = |_: &tree_sitter::ParseState| {
                        if parse_cancel.load(Ordering::Relaxed) {
                            std::ops::ControlFlow::Break(())
                        } else {
                            std::ops::ControlFlow::Continue(())
                        }
                    };
                    let options = ParseOptions::new().progress_callback(&mut progress);
                    let tree = parser.parse_with_options(
                        &mut |offset, _| {
                            if offset >= text.len() {
                                ""
                            } else {
                                let (chunk, chunk_byte_ix) = text.chunk(offset);
                                &chunk[offset - chunk_byte_ix..]
                            }
                        },
                        old_tree.as_ref(),
                        Some(options),
                    )?;
                    if parse_cancel.load(Ordering::Relaxed) {
                        return None;
                    }
                    let injections = injection_data.map_or_else(Default::default, |data| {
                        SyntaxHighlighter::compute_injection_layers(data, &tree, &text)
                    });
                    let folds = if folding {
                        extract_fold_ranges(&tree)
                    } else {
                        Vec::new()
                    };
                    Some((tree, injections, folds))
                })
                .await;

            if let Some((tree, injections, folds)) = result {
                if highlighter.borrow().text().eq(&text_for_apply) {
                    highlighter.borrow_mut().apply_background_tree(
                        tree,
                        &text_for_apply,
                        injections,
                    );
                    syntax_current.set(true);
                }
                let _ = entity.update(cx, |state, cx| {
                    state.apply_highlighter_fold_candidates(folds, cx);
                });
            }
        });
        parse_task.borrow_mut().replace(task);
    }

    fn newline_indent(
        &self,
        text: &Rope,
        selection: Range<usize>,
        unit: &str,
    ) -> Option<NewlineIndent> {
        if !self.syntax_current.get() {
            return None;
        }
        super::indentation::newline_indent(&self.inner.borrow(), text, selection, unit)
    }

    fn matching_brackets(&self, text: &Rope, caret: usize) -> Option<[Range<usize>; 2]> {
        if !self.syntax_current.get() {
            return None;
        }
        super::brackets::matching_brackets(&self.inner.borrow(), text, caret)
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.inner.borrow().styles(range, resolver)
    }
    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(extract_fold_ranges)
            .unwrap_or_default()
    }

    fn fold_ranges_for_edit(&self, range: Range<usize>, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(|tree| extract_fold_ranges_in_range(tree, range))
            .unwrap_or_default()
    }
}

fn to_tree_sitter_edit(edit: BaseInputEdit) -> InputEdit {
    InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: Point::new(edit.start_position.row, edit.start_position.column),
        old_end_position: Point::new(edit.old_end_position.row, edit.old_end_position.column),
        new_end_position: Point::new(edit.new_end_position.row, edit.new_end_position.column),
    }
}

fn extract_fold_ranges(tree: &tree_sitter::Tree) -> Vec<FoldRange> {
    extract_fold_ranges_in_range(tree, 0..usize::MAX)
}

fn extract_fold_ranges_in_range(
    tree: &tree_sitter::Tree,
    byte_range: Range<usize>,
) -> Vec<FoldRange> {
    fn collect(node: tree_sitter::Node, bytes: &Range<usize>, ranges: &mut Vec<FoldRange>) {
        if node.end_byte() <= bytes.start || node.start_byte() >= bytes.end {
            return;
        }
        let start = node.start_position().row;
        let end = node.end_position().row;
        if end.saturating_sub(start) < 2 {
            return;
        }
        ranges.push(FoldRange::new(start, end));
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect(child, bytes, ranges);
        }
    }

    let root = tree.root_node();
    let mut ranges = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        collect(child, &byte_range, &mut ranges);
    }
    ranges.sort_by_key(|range| range.start_line);
    ranges.dedup_by_key(|range| range.start_line);
    ranges
}

#[cfg(all(test, feature = "tree-sitter-rust"))]
mod tests {
    use super::*;
    use gpui::{AppContext, Context, Entity, IntoElement, Render, TestAppContext, Window, div};

    struct Harness(Entity<EditorState>);
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let _ = self.0.read(cx);
            div()
        }
    }

    #[gpui::test]
    fn bracket_matching_waits_for_current_background_syntax(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let adapter = Rc::new(RefCell::new(TreeSitterInputHighlighter::new("rust")));
        let text = Rope::from("fn f() { call(alpha); }");
        assert!(
            adapter
                .borrow_mut()
                .inner
                .borrow_mut()
                .update(None, &text, None)
        );
        // A stored tree alone is insufficient while the adapter awaits freshness.
        assert!(adapter.borrow().matching_brackets(&text, 13).is_none());
        let (_, cx) = cx.add_window_view(|window, cx| {
            let state = cx.new(|cx| EditorState::new(window, cx));
            state.update(cx, |_, cx| {
                adapter.borrow_mut().update(None, &text, false, window, cx)
            });
            Harness(state)
        });
        assert!(adapter.borrow().matching_brackets(&text, 13).is_none());
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(
            adapter.borrow().matching_brackets(&text, 13),
            Some([13..14, 19..20])
        );
    }

    #[gpui::test]
    fn unchanged_text_keeps_parsing_until_syntax_is_current(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let adapter = Rc::new(RefCell::new(TreeSitterInputHighlighter::new("rust")));
        let old = Rope::from("fn f() {}");
        let text = Rope::from("fn f() {\n  if true {");
        // Deterministically model a timed-out parse: the stored text advanced,
        // but the tree and syntax-current flag still describe the previous text.
        assert!(
            adapter
                .borrow_mut()
                .inner
                .borrow_mut()
                .update(None, &old, None)
        );
        adapter.borrow_mut().inner.borrow_mut().edit_tree(
            Some(InputEdit {
                start_byte: old.len() - 1,
                old_end_byte: old.len(),
                new_end_byte: text.len(),
                start_position: Point::new(0, old.len() - 1),
                old_end_position: Point::new(0, old.len()),
                new_end_position: Point::new(1, "  if true {".len()),
            }),
            &text,
        );
        let (_, cx) = cx.add_window_view(|window, cx| {
            let state = cx.new(|cx| EditorState::new(window, cx));
            state.update(cx, |_, cx| {
                adapter.borrow_mut().update(None, &text, false, window, cx)
            });
            Harness(state)
        });
        assert!(adapter.borrow().parse_task.borrow().is_some());
        assert!(
            adapter
                .borrow()
                .newline_indent(&text, text.len()..text.len(), "  ")
                .is_none()
        );
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(
            adapter
                .borrow()
                .newline_indent(&text, text.len()..text.len(), "  ")
                .unwrap()
                .indent(),
            "    "
        );
    }
}
