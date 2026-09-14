use crate::input::EditorSelection;

#[gpui::test]
fn multi_selection_normalization_and_validation(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one two three", window, cx);
                let input = vec![
                    EditorSelection::new(9, 8, 3)
                        .with_line_end_affinity(true)
                        .with_preferred_column(Some((px(7.), 2))),
                    EditorSelection::new(2, 1, 5),
                    EditorSelection::new(4, 12, 12),
                    EditorSelection::new(5, 12, 12),
                ];
                assert!(state.set_selections(input.clone(), 9, cx));
                let expected = vec![
                    EditorSelection::new(9, 8, 1)
                        .with_line_end_affinity(true)
                        .with_preferred_column(Some((px(7.), 2))),
                    input[2].clone(),
                ];
                assert_eq!(state.selections(), expected);
                assert_eq!(state.primary_selection_id(), 9);
                let mut reordered = input.clone();
                reordered.reverse();
                assert!(state.set_selections(reordered, 9, cx));
                assert_eq!(state.selections(), expected);
                assert!(state.set_selections(expected.clone(), 9, cx));
                assert!(!state.set_selections(vec![], 9, cx));
                assert!(!state.set_selections(vec![input[0].clone(), input[0].clone()], 9, cx));
                assert!(!state.set_selections(input.clone(), 100, cx));
                assert_eq!(state.selections(), expected);
                assert!(state.set_selections(input, 5, cx));
                assert_eq!(state.selections()[1].id(), 5);
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_atomic_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one two three", window, cx);
                let before = vec![
                    EditorSelection::new(10, 0, 3),
                    EditorSelection::new(20, 7, 4)
                        .with_line_end_affinity(true)
                        .with_preferred_column(Some((px(17.), 4))),
                ];
                assert!(state.set_selections(before.clone(), 20, cx));
                assert!(state.replace_selections("X", window, cx));
                assert_eq!(state.value(), "X X three");
                let after = state.selections();
                assert_eq!(
                    after
                        .iter()
                        .map(|s| (s.id(), s.anchor(), s.head()))
                        .collect::<Vec<_>>(),
                    vec![(10, 1, 1), (20, 3, 3)]
                );
                state.set_selected_range(0..0, cx);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "one two three");
                assert_eq!(state.selections(), before);
                assert_eq!(state.primary_selection_id(), 20);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "X X three");
                assert_eq!(state.selections(), after);
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_graphemes_and_legacy_collapse(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("a\u{301}🙂\r\nb", window, cx);
                for (offset, expected) in [(1, 0), (2, 0), (4, 3), (6, 3), (8, 7), (100, 10)] {
                    assert!(state.set_selections(
                        vec![EditorSelection::new(1, offset, offset)],
                        1,
                        cx
                    ));
                    assert_eq!(state.selected_range(), expected..expected);
                }
                assert!(state.set_selections(vec![EditorSelection::new(1, 8, 1)], 1, cx));
                assert_eq!(state.selections(), vec![EditorSelection::new(1, 9, 0)]);
                assert!(state.set_selections(
                    vec![EditorSelection::new(1, 0, 0), EditorSelection::new(2, 9, 9)],
                    2,
                    cx
                ));
                state.set_selected_range(1..1, cx);
                assert_eq!(state.selected_range(), 1..1);
                assert_eq!(state.selections().len(), 1);
                state.set_value("", window, cx);
                assert_eq!(state.selections().len(), 1);
                assert!(!state.backspace_selections(window, cx));
                assert!(!state.delete_selections(window, cx));
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_deletion_union_and_redo_noops(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for forward in [false, true] {
                    state.set_value("a\u{301}🙂\r\nb", window, cx);
                    let offsets = if forward {
                        vec![0, 3, 7]
                    } else {
                        vec![3, 7, 9]
                    };
                    let before = offsets
                        .iter()
                        .enumerate()
                        .map(|(id, &offset)| EditorSelection::new(id as u64, offset, offset))
                        .collect::<Vec<_>>();
                    assert!(state.set_selections(before.clone(), 1, cx));
                    assert!(if forward {
                        state.delete_selections(window, cx)
                    } else {
                        state.backspace_selections(window, cx)
                    });
                    assert_eq!(state.value(), "b");
                    assert_eq!(state.selections(), vec![EditorSelection::new(1, 0, 0)]);
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), "a\u{301}🙂\r\nb");
                    assert_eq!(state.selections(), before);
                    state.set_selected_range(0..0, cx);
                    assert!(!state.backspace_selections(window, cx));
                    assert!(!state.replace_selections("", window, cx));
                    state.redo(&Redo, window, cx);
                    assert_eq!(state.value(), "b");
                }
                state.set_value("one two", window, cx);
                assert!(state.set_selections(
                    vec![EditorSelection::new(1, 0, 3), EditorSelection::new(2, 4, 7)],
                    2,
                    cx
                ));
                assert!(state.replace_selections("one", window, cx));
                assert_eq!(state.value(), "one one");
                assert_eq!(
                    state.selections(),
                    vec![EditorSelection::new(1, 3, 3), EditorSelection::new(2, 7, 7)]
                );
                state.undo(&Undo, window, cx);
                assert!(state.replace_selections("X", window, cx));
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "X X");
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_large_atomic_transaction_and_rejection(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                let original = "a ".repeat(1100);
                state.set_value(original.clone(), window, cx);
                let before = (0..1100)
                    .map(|id| EditorSelection::new(id, id as usize * 2, id as usize * 2 + 1))
                    .collect::<Vec<_>>();
                assert!(state.set_selections(before.clone(), 500, cx));
                state.set_readonly(true, cx);
                assert!(!state.replace_selections("X", window, cx));
                assert_eq!(state.selections(), before);
                state.set_readonly(false, cx);
                state.set_disabled(true, cx);
                assert!(!state.delete_selections(window, cx));
                state.set_disabled(false, cx);
                assert!(state.replace_selections("X", window, cx));
                assert_eq!(state.value(), "X ".repeat(1100));
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), original);
                assert_eq!(state.selections(), before);
                assert!(!state.undo_manager.has_undos());
                assert!(!state.replace_selections("a", window, cx));
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "X ".repeat(1100));
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_ordinary_movement_ime_and_events(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = changes.clone();
    cx.update(|cx| {
        cx.subscribe(&view.input, move |_, event, _| {
            if matches!(event, InputEvent::Change) {
                observed.set(observed.get() + 1);
            }
        })
        .detach()
    });
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("abcd", window, cx);
                let selections = vec![EditorSelection::new(1, 0, 1), EditorSelection::new(2, 2, 3)];
                state.set_selections(selections.clone(), 2, cx);
                state.move_to(3, None, cx);
                assert_eq!(state.selections(), vec![EditorSelection::new(2, 3, 3)]);
                state.set_selections(selections.clone(), 2, cx);
                state.select_all(window, cx);
                assert_eq!(state.selections(), vec![EditorSelection::new(2, 0, 4)]);
                state.set_selections(selections.clone(), 2, cx);
                state.replace_and_mark_text_in_range(None, "q", Some(1..1), window, cx);
                assert_eq!(state.selections().len(), 1);
                let marked = state.selections();
                let value = state.value();
                assert!(!state.set_selections(selections.clone(), 2, cx));
                assert!(!state.replace_selections("X", window, cx));
                assert!(!state.backspace_selections(window, cx));
                assert!(!state.delete_selections(window, cx));
                assert_eq!(state.selections(), marked);
                assert_eq!(state.value(), value);
                state.unmark_text(window, cx);
                state.set_value("abcd", window, cx);
                state.set_selections(selections, 2, cx);
            });
        })
        .unwrap();
    let before = changes.get();
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.replace_selections("X", window, cx);
            })
        })
        .unwrap();
    assert_eq!(changes.get(), before + 1);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.undo(&Undo, window, cx);
            })
        })
        .unwrap();
    assert_eq!(changes.get(), before + 2);
}

#[gpui::test]
fn multi_selection_adjacent_ranges_and_mixed_deletions(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("abcdef", window, cx);
                let before = vec![
                    EditorSelection::new(1, 0, 2),
                    EditorSelection::new(2, 2, 4),
                    EditorSelection::new(3, 5, 5),
                    EditorSelection::new(4, 6, 6),
                ];
                assert!(state.set_selections(before.clone(), 3, cx));
                assert_eq!(state.selections(), before);
                assert!(state.backspace_selections(window, cx));
                assert_eq!(state.value(), "");
                assert_eq!(state.selections(), vec![EditorSelection::new(3, 0, 0)]);
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), before);
                assert!(state.replace_selections("🙂\r\n", window, cx));
                assert_eq!(state.value(), "🙂\r\n🙂\r\ne🙂\r\nf🙂\r\n");
                let carets = state
                    .selections()
                    .iter()
                    .map(|s| s.head())
                    .collect::<Vec<_>>();
                assert_eq!(carets, vec![6, 12, 19, 26]);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "abcdef");
                assert_eq!(state.selections(), before);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_legacy_replacement_and_search_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for search in [false, true] {
                    state.set_value("one two one", window, cx);
                    let before = vec![
                        EditorSelection::new(1, 0, 3),
                        EditorSelection::new(2, 7, 4)
                            .with_line_end_affinity(true)
                            .with_preferred_column(Some((px(3.), 4))),
                    ];
                    state.set_selections(before.clone(), 2, cx);
                    if search {
                        state.set_search_query("one", false, cx);
                        assert_eq!(state.replace_all_search_matches("X", window, cx), 2);
                        assert_eq!(state.value(), "X two X");
                    } else {
                        state.replace("X", window, cx);
                        assert_eq!(state.value(), "one X one");
                    }
                    let after = state.selections();
                    assert_eq!(after.len(), 1);
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), "one two one");
                    assert_eq!(state.selections(), before);
                    state.redo(&Redo, window, cx);
                    assert_eq!(state.selections(), after);
                }
            })
        })
        .unwrap();
}

struct MultiSelectionLanguage;
impl crate::input::InputHighlighter for MultiSelectionLanguage {
    fn language(&self) -> SharedString {
        "test".into()
    }
    fn update(
        &mut self,
        _: Option<crate::input::InputEdit>,
        _: &Rope,
        _: bool,
        _: &mut Window,
        _: &mut Context<crate::input::EditorState>,
    ) {
    }
    fn styles(
        &self,
        _: &Range<usize>,
        _: &dyn crate::input::HighlightStyleResolver,
    ) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
        vec![]
    }
    fn fold_ranges(&self, _: &Rope) -> Vec<crate::input::FoldRange> {
        vec![]
    }
    fn newline_indent(
        &self,
        _: &Rope,
        _: Range<usize>,
        _: &str,
    ) -> Option<crate::input::NewlineIndent> {
        Some(crate::input::NewlineIndent::new("  ").with_closing_indent(""))
    }
    fn comment_syntax(&self, _: &Rope, _: Range<usize>) -> Option<crate::input::CommentSyntax> {
        Some(crate::input::CommentSyntax::line("//"))
    }
}

#[gpui::test]
fn multi_selection_single_language_command_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for comment in [false, true] {
                    state.set_value("{}\nxx", window, cx);
                    *state.mode.highlighter().unwrap().borrow_mut() =
                        Some(Box::new(MultiSelectionLanguage));
                    let primary = if comment {
                        EditorSelection::new(2, 2, 0)
                    } else {
                        EditorSelection::new(2, 1, 1)
                    };
                    state.set_selections(
                        vec![
                            primary.with_line_end_affinity(true),
                            EditorSelection::new(1, 3, 5),
                        ],
                        2,
                        cx,
                    );
                    let before = state.selections();
                    if comment {
                        state.toggle_comment(&ToggleComment, window, cx);
                    } else {
                        state.enter(
                            &Enter {
                                secondary: false,
                                shift: false,
                            },
                            window,
                            cx,
                        );
                    }
                    let after = state.selections();
                    let value = state.value();
                    assert_eq!(after.len(), 2);
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), "{}\nxx");
                    assert_eq!(state.selections(), before);
                    state.redo(&Redo, window, cx);
                    assert_eq!(state.value(), value);
                    assert_eq!(state.selections(), after);
                }
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_pointer_context_collapse(cx: &mut TestAppContext) {
    cx.update(crate::GlobalState::init);
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one two", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 3), EditorSelection::new(2, 4, 7)],
                    1,
                    cx,
                );
                state.handle_right_click_menu(point(px(0.), px(0.)), 1, window, cx);
                assert_eq!(state.selections(), vec![EditorSelection::new(1, 0, 3)]);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_local_folds_noop_search_and_scalar_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("a\u{301}🙂\r\nb", window, cx);
                state.set_selected_range(1..3, cx);
                let scalar_before = state.selections();
                assert!(state.replace_selections("X", window, cx));
                assert_eq!(state.value(), "X🙂\r\nb");
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), scalar_before);
                assert_eq!(state.selected_range(), 1..3);

                let source = "foo\r\nfn other() {\r\n    body();\r\n}\r\nfoo";
                state.set_value(source, window, cx);
                state.apply_highlighter_fold_candidates(
                    vec![crate::input::FoldRange::new(1, 3)],
                    cx,
                );
                state.display_map.set_folded(1, true);
                let last = source.rfind("foo").unwrap();
                state.set_selections(
                    vec![
                        EditorSelection::new(1, 0, 3),
                        EditorSelection::new(2, last + 3, last),
                    ],
                    2,
                    cx,
                );
                let before = state.selections();
                assert!(state.replace_selections("猫🙂", window, cx));
                assert_eq!(state.value(), source.replace("foo", "猫🙂"));
                assert_eq!(state.display_map.folded_ranges().len(), 1);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), source);
                assert_eq!(state.selections(), before);
                state.set_search_query("foo", false, cx);
                assert_eq!(state.replace_all_search_matches("foo", window, cx), 2);
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), source.replace("foo", "猫🙂"));
                assert_eq!(state.display_map.folded_ranges().len(), 1);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_singleton_ordinary_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("abc", window, cx);
                let before = vec![
                    EditorSelection::new(7, 2, 1)
                        .with_line_end_affinity(true)
                        .with_preferred_column(Some((px(17.), 4))),
                ];
                state.set_selections(before.clone(), 7, cx);
                state.replace("X", window, cx);
                let after = state.selections();
                state.set_selections(vec![EditorSelection::new(99, 0, 0)], 99, cx);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "abc");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "aXc");
                assert_eq!(state.selections(), after);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_singleton_typing_coalesces_and_noop_preserves_redo(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("", window, cx);
                let before = vec![
                    EditorSelection::new(7, 0, 0)
                        .with_line_end_affinity(true)
                        .with_preferred_column(Some((px(17.), 4))),
                ];
                state.set_selections(before.clone(), 7, cx);
                state.replace_text_in_range(None, "a", window, cx);
                state.replace_text_in_range(None, "b", window, cx);
                let after = state.selections();
                state.set_selections(vec![EditorSelection::new(99, 0, 0)], 99, cx);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "");
                assert_eq!(state.selections(), before);
                assert!(!state.undo_manager.has_undos());
                state.replace("", window, cx);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "ab");
                assert_eq!(state.selections(), after);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_singleton_ime_history_and_save_boundary(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("abc", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(7, 1, 2).with_line_end_affinity(true)],
                    7,
                    cx,
                );
                let before = state.selections();
                state.replace_and_mark_text_in_range(None, "q", Some(1..1), window, cx);
                state.replace_and_mark_text_in_range(None, "qu", Some(2..2), window, cx);
                state.break_undo_coalescing();
                let saved = state.selections();
                state.replace_text_in_range(None, "文", window, cx);
                let after = state.selections();
                state.set_selections(vec![EditorSelection::new(99, 0, 0)], 99, cx);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "aquc");
                assert_eq!(state.selections(), saved);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "abc");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "a文c");
                assert_eq!(state.selections(), after);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_singleton_language_and_indentation_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for command in 0..4 {
                    let (source, anchor, head) = match command {
                        0 => ("{}\nxx", 1, 1),
                        1 => ("{}\nxx", 2, 0),
                        2 => ("abc\nxyz", 3, 0),
                        _ => ("    abc\nxyz", 7, 4),
                    };
                    state.set_value(source, window, cx);
                    *state.mode.highlighter().unwrap().borrow_mut() =
                        Some(Box::new(MultiSelectionLanguage));
                    state.set_selections(
                        vec![EditorSelection::new(7, anchor, head).with_line_end_affinity(true)],
                        7,
                        cx,
                    );
                    let before = state.selections();
                    match command {
                        0 => state.enter(
                            &Enter {
                                secondary: false,
                                shift: false,
                            },
                            window,
                            cx,
                        ),
                        1 => state.toggle_comment(&ToggleComment, window, cx),
                        2 => state.indent(true, window, cx),
                        _ => state.outdent(true, window, cx),
                    }
                    let value = state.value();
                    let after = state.selections();
                    state.set_selections(vec![EditorSelection::new(99, 0, 0)], 99, cx);
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), source, "command {command}");
                    assert_eq!(state.selections(), before, "command {command}");
                    state.redo(&Redo, window, cx);
                    assert_eq!(state.value(), value, "command {command}");
                    assert_eq!(state.selections(), after, "command {command}");
                }
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_singleton_deletion_coalesces_with_affinity(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for backwards in [false, true] {
                    state.set_value("abcd", window, cx);
                    let offset = if backwards { 4 } else { 0 };
                    state.set_selections(
                        vec![EditorSelection::new(7, offset, offset).with_line_end_affinity(true)],
                        7,
                        cx,
                    );
                    let before = state.selections();
                    for _ in 0..2 {
                        if backwards {
                            state.backspace(&Backspace, window, cx);
                        } else {
                            state.delete(&Delete, window, cx);
                        }
                    }
                    let after = state.selections();
                    let value = state.value();
                    state.set_selections(vec![EditorSelection::new(99, 0, 0)], 99, cx);
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), "abcd");
                    assert_eq!(state.selections(), before);
                    assert!(!state.undo_manager.has_undos());
                    state.redo(&Redo, window, cx);
                    assert_eq!(state.value(), value);
                    assert_eq!(state.selections(), after);
                }
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_indentation_batches_collection(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for outdent in [false, true] {
                    let (source, end, secondary, expected) = if outdent {
                        ("  abc\n  xyz\nlast", 11, 12, "abc\nxyz\nlast")
                    } else {
                        ("abc\nxyz\nlast", 7, 8, "  abc\n  xyz\n  last")
                    };
                    state.set_value(source, window, cx);
                    let before = vec![
                        EditorSelection::new(7, end, 0)
                            .with_line_end_affinity(true)
                            .with_preferred_column(Some((px(17.), 4))),
                        EditorSelection::new(8, secondary, secondary),
                    ];
                    state.set_selections(before.clone(), 7, cx);
                    if outdent {
                        state.outdent(true, window, cx);
                    } else {
                        state.indent(true, window, cx);
                    }
                    let after = state.selections();
                    assert_eq!(state.value(), expected);
                    assert_eq!(after.len(), 2);
                    state.undo(&Undo, window, cx);
                    assert_eq!(state.value(), source);
                    assert_eq!(state.selections(), before);
                    assert!(!state.undo_manager.has_undos());
                    state.redo(&Redo, window, cx);
                    assert_eq!(state.value(), expected);
                    assert_eq!(state.selections(), after);
                }
                state.set_value("abc\nxyz", window, cx);
                let before = vec![EditorSelection::new(7, 3, 0), EditorSelection::new(8, 4, 4)];
                state.set_selections(before.clone(), 7, cx);
                state.replace_selections("X", window, cx);
                let after = state.selections();
                let value = state.value();
                state.undo(&Undo, window, cx);
                state.outdent(true, window, cx);
                assert_eq!(state.selections(), before);
                assert_eq!(state.value(), "abc\nxyz");
                assert!(!state.undo_manager.has_undos());
                state.set_readonly(true, cx);
                state.indent(true, window, cx);
                assert_eq!(state.selections(), before);
                state.set_readonly(false, cx);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), value);
                assert_eq!(state.selections(), after);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_ime_primary_commit_and_cancellation_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("abcd", window, cx);
                let before = vec![
                    EditorSelection::new(1, 0, 1),
                    EditorSelection::new(2, 3, 2)
                        .with_line_end_affinity(true)
                        .with_preferred_column(Some((px(7.), 2))),
                ];
                state.set_selections(before.clone(), 2, cx);
                state.replace_and_mark_text_in_range(None, "q", Some(1..1), window, cx);
                state.replace_text_in_range(None, "Q", window, cx);
                let after = state.selections();
                assert_eq!(state.value(), "abQd");
                assert_eq!(after.len(), 1);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "abcd");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "abQd");
                assert_eq!(state.selections(), after);
                state.undo(&Undo, window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 0), EditorSelection::new(2, 2, 2)],
                    2,
                    cx,
                );
                state.replace_and_mark_text_in_range(None, "q", Some(1..1), window, cx);
                state.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert_eq!(state.value(), "abcd");
                assert!(!state.undo_manager.has_undos());
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "abQd");
                assert_eq!(state.selections(), after);
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_numeric_normalization_plans_same_bytes_and_noops(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("a b", window, cx);
                let before = vec![EditorSelection::new(1, 0, 1), EditorSelection::new(2, 2, 3)];
                state.set_selections(before.clone(), 2, cx);
                state.ensure_number_mask();
                assert!(state.replace_selections("１", window, cx));
                assert_eq!(state.value(), "1 1");
                let after = vec![EditorSelection::new(1, 1, 1), EditorSelection::new(2, 3, 3)];
                assert_eq!(state.selections(), after);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "a b");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.selections(), after);
                state.set_selections(before.clone(), 2, cx);
                state.replace_selections("2", window, cx);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "1 1");
                let before_noop = state.selections();
                assert!(!state.replace_selections("１", window, cx));
                assert_eq!(state.selections(), before_noop);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "2 2");
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_history_replays_recorded_bytes_without_normalization(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("１ a", window, cx);
                let before = vec![EditorSelection::new(1, 0, 3), EditorSelection::new(2, 5, 4)];
                state.set_selections(before.clone(), 2, cx);
                state.ensure_number_mask();
                assert!(state.replace_selections("2", window, cx));
                let after = state.selections();
                assert_eq!(state.value(), "2 2");
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "１ a");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "2 2");
                assert_eq!(state.selections(), after);
                // set_value also ignores history, but still applies ordinary normalization.
                state.set_value("４ ５", window, cx);
                assert_eq!(state.value(), "4 5");
            })
        })
        .unwrap();
}

#[gpui::test]
fn multi_selection_history_ignores_normalizer_changes_after_edit(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("１ a", window, cx);
                let before = vec![EditorSelection::new(1, 0, 3), EditorSelection::new(2, 4, 5)];
                state.set_selections(before.clone(), 2, cx);
                assert!(state.replace_selections("３", window, cx));
                let after = state.selections();
                assert_eq!(state.value(), "３ ３");
                state.ensure_number_mask();
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "１ a");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "３ ３");
                assert_eq!(state.selections(), after);
            })
        })
        .unwrap();
}
