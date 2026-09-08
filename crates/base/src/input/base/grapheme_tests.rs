#[gpui::test]
fn grapheme_arrows_cross_complete_clusters(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for cluster in ["e\u{301}", "👨‍👩‍👧‍👦", "👍🏽", "🇺🇸", "中", "a", "\r\n", "\r", "\n"]
            {
                let text = format!("{cluster}X");
                state.set_value(text, window, cx);
                state.set_selected_range(0..0, cx);
                state.left(&MoveLeft, window, cx);
                assert_eq!(state.selected_range(), 0..0);
                state.right(&MoveRight, window, cx);
                assert_eq!(
                    state.selected_range(),
                    cluster.len()..cluster.len(),
                    "{cluster:?}"
                );
                state.left(&MoveLeft, window, cx);
                assert_eq!(state.selected_range(), 0..0, "{cluster:?}");
                state.set_selected_range(state.text.len()..state.text.len(), cx);
                state.right(&MoveRight, window, cx);
                assert_eq!(state.cursor(), state.text.len());
            }
        })
    });
}

#[gpui::test]
fn grapheme_interior_and_partial_selection_arrows(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("e\u{301}Xe\u{301}", window, cx);
            for reversed in [false, true] {
                for (range, left, right) in [(1..1, 0, 3), (1..5, 0, 7), (3..4, 3, 4)] {
                    state.set_selected_range(range.clone(), cx);
                    state.selection_reversed = reversed;
                    state.left(&MoveLeft, window, cx);
                    assert_eq!(state.cursor(), left);
                    state.set_selected_range(range, cx);
                    state.selection_reversed = reversed;
                    state.right(&MoveRight, window, cx);
                    assert_eq!(state.cursor(), right);
                }
            }
        })
    });
}

#[gpui::test]
fn grapheme_shift_expands_shrinks_and_reverses(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("e\u{301}Xe\u{301}", window, cx);
            state.set_selected_range(1..1, cx);
            state.select_right(&SelectRight, window, cx);
            assert_eq!(state.selected_range(), 0..3);
            assert!(!state.selection_reversed);
            state.select_right(&SelectRight, window, cx);
            assert_eq!(state.selected_range(), 0..4);
            state.select_left(&SelectLeft, window, cx);
            assert_eq!(state.selected_range(), 0..3);
            state.select_left(&SelectLeft, window, cx);
            assert_eq!(state.selected_range(), 0..0);
            state.set_selected_range(5..5, cx);
            state.select_left(&SelectLeft, window, cx);
            assert_eq!(state.selected_range(), 4..7);
            assert!(state.selection_reversed);
            state.select_right(&SelectRight, window, cx);
            assert_eq!(state.selected_range(), 7..7);
            state.set_selected_range(3..3, cx);
            state.select_left(&SelectLeft, window, cx);
            state.select_right(&SelectRight, window, cx);
            state.select_right(&SelectRight, window, cx);
            assert_eq!(state.selected_range(), 3..4);
            assert!(!state.selection_reversed);
        })
    });
}

#[gpui::test]
fn grapheme_deletion_restores_original_caret(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for cluster in ["e\u{301}", "👨‍👩‍👧‍👦", "👍🏽", "🇺🇸", "中", "a", "\r\n", "\r", "\n"]
            {
                for backwards in [false, true] {
                    let mut offsets: Vec<_> = cluster
                        .char_indices()
                        .map(|(i, _)| i)
                        .filter(|i| *i > 0)
                        .collect();
                    offsets.push(if backwards { cluster.len() } else { 0 });
                    for offset in offsets {
                        let text = format!("{cluster}X");
                        state.set_value(text.clone(), window, cx);
                        state.set_selected_range(offset..offset, cx);
                        if backwards {
                            state.backspace(&Backspace, window, cx);
                        } else {
                            state.delete(&Delete, window, cx);
                        }
                        assert_eq!(
                            state.value(),
                            "X",
                            "{cluster:?}, {offset}, backwards={backwards}"
                        );
                        state.undo(&Undo, window, cx);
                        assert_eq!(state.value(), text);
                        assert_eq!(state.selected_range(), offset..offset);
                        state.redo(&Redo, window, cx);
                        assert_eq!(state.value(), "X");
                        assert_eq!(state.selected_range(), 0..0);
                    }
                }
            }
        })
    });
}

#[gpui::test]
fn grapheme_selected_deletion_restores_direction(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for backwards in [false, true] {
                for reversed in [false, true] {
                    state.set_value("e\u{301}Xe\u{301}Y", window, cx);
                    state.set_selected_range(1..5, cx);
                    state.selection_reversed = reversed;
                    if backwards {
                        state.backspace(&Backspace, window, cx);
                    } else {
                        state.delete(&Delete, window, cx);
                    }
                    assert_eq!(state.value(), "Y");
                    state.selection_reversed = !reversed;
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), "e\u{301}Xe\u{301}Y");
                    assert_eq!(state.selected_range(), 1..5);
                    assert_eq!(state.selection_reversed, reversed);
                }
            }
        })
    });
}

#[gpui::test]
fn grapheme_rejected_deletion_preserves_selection(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("e\u{301}X", window, cx);
            for mode in 0..3 {
                state.readonly = mode == 0;
                state.disabled = mode == 1;
                state.validate = (mode == 2)
                    .then(|| Box::new(|text: &str, _: &mut App| text == "e\u{301}X") as _);
                for backwards in [false, true] {
                    state.set_selected_range(1..1, cx);
                    if backwards {
                        state.backspace(&Backspace, window, cx);
                    } else {
                        state.delete(&Delete, window, cx);
                    }
                    assert_eq!(state.value(), "e\u{301}X");
                    assert_eq!(state.selected_range(), 1..1);
                }
            }
        })
    });
}

#[gpui::test]
fn grapheme_programmatic_replacement_remains_scalar_exact(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("e\u{301}X", window, cx);
            state.set_selected_range(1..1, cx);
            assert_eq!(state.selected_range(), 1..1);
            state.replace_text_in_range(Some(1..2), "", window, cx);
            assert_eq!(state.value(), "eX");
        })
    });
}

#[gpui::test]
fn grapheme_deletion_preserves_ime_marked_range_precedence(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for backwards in [false, true] {
                state.set_value("X", window, cx);
                state.set_selected_range(0..0, cx);
                state.replace_and_mark_text_in_range(None, "e\u{301}ab", Some(2..2), window, cx);
                if backwards {
                    state.backspace(&Backspace, window, cx);
                } else {
                    state.delete(&Delete, window, cx);
                }
                assert_eq!(state.value(), "X");
                assert!(state.ime_marked_range.is_none());
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "X");
            }
        })
    });
}

#[gpui::test]
fn grapheme_navigation_keeps_fold_clamps(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for newline in ["\n", "\r\n"] {
                state.set_value(format!("e\u{301}{newline}hidden{newline}X"), window, cx);
                state.apply_highlighter_fold_candidates(
                    vec![crate::input::FoldRange::new(0, 2)],
                    cx,
                );
                state.display_map.set_folded(0, true);
                let end = state.text.line_start_offset(2);
                state.set_selected_range(3..3, cx);
                state.right(&MoveRight, window, cx);
                assert_eq!(state.cursor(), end);
                state.left(&MoveLeft, window, cx);
                assert_eq!(state.cursor(), 3);
                state.select_right(&SelectRight, window, cx);
                assert_eq!(state.selected_range(), 3..end);
            }
        })
    });
}

#[gpui::test]
fn grapheme_deletions_coalesce_and_document_edges_are_noops(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for backwards in [false, true] {
                let text = "e\u{301}👍🏽";
                state.set_value(text, window, cx);
                let offset = if backwards { text.len() } else { 0 };
                state.set_selected_range(offset..offset, cx);
                for _ in 0..3 {
                    if backwards {
                        state.backspace(&Backspace, window, cx);
                    } else {
                        state.delete(&Delete, window, cx);
                    }
                }
                assert_eq!(state.value(), "");
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), text);
                assert_eq!(state.selected_range(), offset..offset);
            }
        })
    });
}

#[gpui::test]
fn grapheme_deletion_clears_collapsed_caret_affinity(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            for backwards in [false, true] {
                state.set_value("e\u{301}X", window, cx);
                state.set_selected_range(1..1, cx);
                state.cursor_line_end_affinity = true;
                if backwards {
                    state.backspace(&Backspace, window, cx);
                } else {
                    state.delete(&Delete, window, cx);
                }
                assert!(!state.cursor_line_end_affinity);
            }
        })
    });
}
