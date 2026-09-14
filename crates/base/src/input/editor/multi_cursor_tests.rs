#[gpui::test]
fn multi_cursor_actual_typing_clipboard_and_history(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one one one", window, cx);
                let before = vec![
                    EditorSelection::new(1, 0, 3),
                    EditorSelection::new(2, 7, 4),
                    EditorSelection::new(3, 8, 11),
                ];
                state.set_selections(before.clone(), 2, cx);
                state.replace_text_in_range(None, "猫", window, cx);
                assert_eq!(state.value(), "猫 猫 猫");
                assert_eq!(state.selections().len(), 3);
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "one one one");
                assert_eq!(state.selections(), before);
                state.copy(&Copy, window, cx);
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    "one\none\none"
                );
                state.cut(&Cut, window, cx);
                assert_eq!(state.value(), "  ");
                state.undo(&Undo, window, cx);
                cx.write_to_clipboard(ClipboardItem::new_string("A\r\nB".into()));
                state.paste(&Paste, window, cx);
                assert_eq!(state.value(), "A\r\nB A\r\nB A\r\nB");
                state.set_value("a b", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 1), EditorSelection::new(2, 2, 3)],
                    2,
                    cx,
                );
                state.ensure_number_mask();
                cx.write_to_clipboard(ClipboardItem::new_string("１".into()));
                state.paste(&Paste, window, cx);
                assert_eq!(state.value(), "1 1");
                assert_eq!(
                    state.selections(),
                    vec![EditorSelection::new(1, 1, 1), EditorSelection::new(2, 3, 3)]
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_actual_deletion_and_indent(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("a🙂b\r\nc🙂d", window, cx);
                let before = vec![
                    EditorSelection::new(1, 5, 5),
                    EditorSelection::new(2, 13, 13),
                ];
                state.set_selections(before.clone(), 1, cx);
                state.backspace(&Backspace, window, cx);
                assert_eq!(state.value(), "ab\r\ncd");
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), before);
                state.set_selections(
                    vec![EditorSelection::new(1, 1, 1), EditorSelection::new(2, 9, 9)],
                    1,
                    cx,
                );
                state.delete(&Delete, window, cx);
                assert_eq!(state.value(), "ab\r\ncd");
                state.undo(&Undo, window, cx);
                state.set_value("abc\nxyz", window, cx);
                state.set_selections(
                    vec![
                        EditorSelection::new(1, 0, 0),
                        EditorSelection::new(2, 2, 2),
                        EditorSelection::new(3, 4, 4),
                    ],
                    1,
                    cx,
                );
                state.indent(true, window, cx);
                assert_eq!(state.value(), "  abc\n  xyz");
                assert_eq!(state.selections().len(), 3);
                state.outdent(true, window, cx);
                assert_eq!(state.value(), "abc\nxyz");
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_occurrences_wrap_unicode_reverse_and_readonly(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one one one", window, cx);
                state.set_selected_range(1..1, cx);
                let primary = state.primary_selection_id();
                for expected in [
                    vec![0..3],
                    vec![0..3, 4..7],
                    vec![0..3, 4..7, 8..11],
                    vec![0..3, 4..7, 8..11],
                ] {
                    state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                    assert_eq!(
                        state
                            .selections()
                            .iter()
                            .map(EditorSelection::range)
                            .collect::<Vec<_>>(),
                        expected
                    );
                    assert_eq!(state.primary_selection_id(), primary);
                }
                state.set_selections(vec![EditorSelection::new(8, 11, 8)], 8, cx);
                state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                assert_eq!(
                    state
                        .selections()
                        .iter()
                        .map(EditorSelection::range)
                        .collect::<Vec<_>>(),
                    vec![0..3, 8..11]
                );
                assert_eq!(state.primary_selection_id(), 8);
                assert_eq!(state.selections()[1].head(), 8);
                state.escape(&Escape, window, cx);
                assert_eq!(state.selections(), vec![EditorSelection::new(8, 11, 8)]);
                state.set_value("rök\r\nrök RÖK", window, cx);
                state.set_selected_range(1..1, cx);
                state.set_readonly(true, cx);
                state.select_all_occurrences(&SelectAllOccurrences, window, cx);
                assert_eq!(
                    state
                        .selections()
                        .iter()
                        .map(EditorSelection::range)
                        .collect::<Vec<_>>(),
                    vec![0..4, 6..10]
                );
                let before = state.selections();
                state.replace_text_in_range(None, "X", window, cx);
                state.backspace(&Backspace, window, cx);
                assert_eq!(state.value(), "rök\r\nrök RÖK");
                assert_eq!(state.selections(), before);
                state.set_readonly(false, cx);
                for (source, offset) in [("   ", 1), ("", 0), ("...", 1), ("one\r\none", 4)] {
                    state.set_value(source, window, cx);
                    state.set_selected_range(offset..offset, cx);
                    let before = state.selections();
                    state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                    state.select_all_occurrences(&SelectAllOccurrences, window, cx);
                    assert_eq!(state.selections(), before);
                }
                state.set_value("aaaaa", window, cx);
                state.set_selections(vec![EditorSelection::new(7, 2, 0)], 7, cx);
                state.select_all_occurrences(&SelectAllOccurrences, window, cx);
                assert_eq!(
                    state
                        .selections()
                        .iter()
                        .map(EditorSelection::range)
                        .collect::<Vec<_>>(),
                    vec![0..2, 2..4]
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_occurrences_unfold_without_replacing_primary(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one\nfn block() {\n one\n}\n", window, cx);
                state.apply_highlighter_fold_candidates(
                    vec![crate::input::FoldRange::new(1, 3)],
                    cx,
                );
                state.display_map.set_folded(1, true);
                state.set_selections(vec![EditorSelection::new(7, 3, 0)], 7, cx);
                state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                assert!(state.display_map.folded_ranges().is_empty());
                assert_eq!(state.selections()[0], EditorSelection::new(7, 3, 0));
                assert_eq!(state.selections()[1].range(), 18..21);
                assert_eq!(state.primary_selection_id(), 7);
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_explicit_native_ranges_and_ime_lifecycle(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("a\u{301} b", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 3), EditorSelection::new(2, 4, 5)],
                    2,
                    cx,
                );
                let before = state.selections();
                state.replace_text_in_range(Some(1..2), "", window, cx);
                assert_eq!(state.value(), "a b");
                assert_eq!(state.selections().len(), 1);
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), before);
                state.replace_and_mark_text_in_range(None, "q", Some(1..1), window, cx);
                assert_eq!(state.value(), "a\u{301} q");
                assert_eq!(state.selections().len(), 1);
                state.replace_and_mark_text_in_range(None, "qu", Some(2..2), window, cx);
                assert_eq!(state.value(), "a\u{301} qu");
                assert_eq!(state.selections().len(), 1);
                state.replace_text_in_range(None, "文", window, cx);
                assert_eq!(state.value(), "a\u{301} 文");
                let after = state.selections();
                state.undo(&Undo, window, cx);
                assert_eq!(state.value(), "a\u{301} b");
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.selections(), after);
                state.undo(&Undo, window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 0), EditorSelection::new(2, 4, 4)],
                    2,
                    cx,
                );
                state.replace_and_mark_text_in_range(None, "q", Some(1..1), window, cx);
                state.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert_eq!(state.value(), "a\u{301} b");
                assert_eq!(state.selections().len(), 1);
                assert!(!state.undo_manager.has_undos());
                state.redo(&Redo, window, cx);
                assert_eq!(state.value(), "a\u{301} 文");
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_empty_clipboard_noops_and_change_count(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let changes = Rc::new(Cell::new(0));
    let count = changes.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&view.input, move |_, event, _| {
            if matches!(event, InputEvent::Change) {
                count.set(count.get() + 1);
            }
        })
    });
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("ab cd", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 0), EditorSelection::new(2, 3, 3)],
                    1,
                    cx,
                );
                cx.write_to_clipboard(ClipboardItem::new_string("saved".into()));
                state.copy(&Copy, window, cx);
                state.cut(&Cut, window, cx);
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "saved");
                assert_eq!(state.value(), "ab cd");
            });
        })
        .unwrap();
    let before = changes.get();
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.replace_text_in_range(None, "X", window, cx);
            });
        })
        .unwrap();
    assert_eq!(changes.get(), before + 1);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.undo(&Undo, window, cx);
            });
        })
        .unwrap();
    assert_eq!(changes.get(), before + 2);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.redo(&Redo, window, cx);
            });
        })
        .unwrap();
    assert_eq!(changes.get(), before + 3);
}

#[gpui::test]
fn multi_cursor_adjacent_newlines_keep_distinct_carets(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("ab", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 1), EditorSelection::new(2, 1, 2)],
                    2,
                    cx,
                );
                state.enter(
                    &Enter {
                        secondary: false,
                        shift: false,
                    },
                    window,
                    cx,
                );
                assert_eq!(state.value(), "\n\n");
                assert_eq!(
                    state.selections(),
                    vec![EditorSelection::new(1, 1, 1), EditorSelection::new(2, 2, 2)]
                );
                state.set_value("aaaaa", window, cx);
                state.set_selections(vec![EditorSelection::new(1, 1, 3)], 1, cx);
                state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                assert_eq!(
                    state
                        .selections()
                        .iter()
                        .map(EditorSelection::range)
                        .collect::<Vec<_>>(),
                    vec![1..3, 3..5]
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_clipboard_with_empty_primary(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("abc def", window, cx);
                state.set_selections(
                    vec![EditorSelection::new(1, 0, 0), EditorSelection::new(2, 7, 4)],
                    1,
                    cx,
                );
                cx.write_to_clipboard(ClipboardItem::new_string("saved".into()));
                state.copy(&Copy, window, cx);
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "def");
                assert!(state.context_menu_capabilities().has_selection());
                state.cut(&Cut, window, cx);
                assert_eq!(state.value(), "abc ");
                assert_eq!(
                    state.selections(),
                    vec![EditorSelection::new(1, 0, 0), EditorSelection::new(2, 4, 4)]
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_many_occurrences_responsiveness(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle.update(cx, |_, window, cx| {
        view.input.update(cx, |state, cx| {
            for count in [100, 1000, 5000] {
                state.set_value("one\n".repeat(count), window, cx);
                state.set_selected_range(0..3, cx);
                let start = std::time::Instant::now();
                state.select_all_occurrences(&SelectAllOccurrences, window, cx);
                let select_time = start.elapsed();
                assert_eq!(state.selections().len(), count);
                let start = std::time::Instant::now();
                state.replace_text_in_range(None, "猫", window, cx);
                let type_time = start.elapsed();
                assert_eq!(state.value(), "猫\n".repeat(count));
                let start = std::time::Instant::now();
                state.indent(true, window, cx);
                let indent_time = start.elapsed();
                assert_eq!(state.value(), "  猫\n".repeat(count));
                assert_eq!(state.selections().len(), count);
                eprintln!("{count} selections: occurrences {select_time:?}, typing {type_time:?}, indent {indent_time:?}");
            }
        });
    }).unwrap();
}

#[gpui::test]
fn multi_cursor_readonly_escape_dispatch(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut visual = VisualTestContext::from_window(view.window_handle.into(), cx);
    visual.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("one one", window, cx);
            state.set_selections(
                vec![EditorSelection::new(1, 0, 3), EditorSelection::new(2, 4, 7)],
                2,
                cx,
            );
            state.set_readonly(true, cx);
            state.focus(window, cx);
        });
    });
    visual.run_until_parked();
    visual.simulate_keystrokes("escape");
    visual.update(|_, cx| {
        view.input.read_with(cx, |state, _| {
            assert_eq!(state.selections(), vec![EditorSelection::new(2, 4, 7)])
        });
    });
}

#[gpui::test]
fn multi_cursor_mouse_geometry_toggle_drag_and_disabled(cx: &mut TestAppContext) {
    cx.update(crate::GlobalState::init);
    let view = InputView::new(cx);
    let mut visual = VisualTestContext::from_window(view.window_handle.into(), cx);
    visual.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("one a\u{301}🙂 two\r\nthree four", window, cx);
            state.set_selected_range(0..0, cx);
            state.focus(window, cx);
        });
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            let position = |offset| {
                let pos = state.line_and_position_for_offset(offset).2.unwrap();
                state.last_bounds.unwrap().origin
                    + pos
                    + point(px(0.), state.last_layout.as_ref().unwrap().line_height / 2.)
            };
            let second = position(12);
            let first = position(0);
            let event = |position, alt, count| MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers: gpui::Modifiers {
                    alt,
                    ..Default::default()
                },
                click_count: count,
                ..Default::default()
            };
            state.on_mouse_down(&event(second, true, 1), window, cx);
            assert_eq!(
                state
                    .selections()
                    .iter()
                    .map(EditorSelection::head)
                    .collect::<Vec<_>>(),
                vec![0, 12]
            );
            assert!(!state.selecting);
            state.on_mouse_down(&event(second, true, 1), window, cx);
            assert_eq!(state.selections().len(), 1);
            state.on_mouse_down(&event(first, true, 1), window, cx);
            assert_eq!(state.selections().len(), 1);
            state.on_mouse_down(&event(second, true, 1), window, cx);
            state.on_mouse_down(&event(first, true, 1), window, cx);
            assert_eq!(state.selections().len(), 1);
            assert_eq!(state.selections()[0].head(), 12);
            state.toggle_caret(5, false, cx);
            assert_eq!(state.selections()[0].head(), 4);
            state.disabled = true;
            let before = state.selections();
            state.on_mouse_down(&event(first, false, 1), window, cx);
            assert_eq!(state.selections(), before);
            state.disabled = false;
            state.on_mouse_down(&event(first, false, 2), window, cx);
            assert_eq!(state.selections().len(), 1);
            assert_eq!(state.selected_range(), 0..3);
            state.select_by_mouse(14, false, cx);
            assert_eq!(state.selected_range(), 0..15);
            state.on_mouse_down(&event(second, false, 3), window, cx);
            assert_eq!(state.selected_range(), 0..15);
            state.select_by_mouse(19, false, cx);
            assert_eq!(state.selected_range(), 0..27);
        });
    });
}

#[gpui::test]
fn multi_cursor_secondary_geometry_roundtrips_wrap_fold_and_scroll(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut visual = VisualTestContext::from_window(view.window_handle.into(), cx);
    visual.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value(
                format!(
                    "{}\nfold {{\n hidden\n}}\nlast word\n{}",
                    "word ".repeat(200),
                    "tail\n".repeat(80)
                ),
                window,
                cx,
            );
            state.focus(window, cx);
        });
    });
    visual.run_until_parked();
    visual.update(|_, cx| {
        view.input.update(cx, |state, cx| {
            let layout = state.last_layout.as_ref().unwrap();
            assert!(layout.lines[0].wrapped_lines.len() > 1);
            let wrap = layout.lines[0].wrapped_lines[0].len();
            let tail = state.text.to_string().find("last word").unwrap();
            state.apply_highlighter_fold_candidates(vec![crate::input::FoldRange::new(1, 3)], cx);
            state.display_map.set_folded(1, true);
            state.set_selections(
                vec![
                    EditorSelection::new(1, 0, 0),
                    EditorSelection::new(2, wrap, wrap).with_line_end_affinity(true),
                    EditorSelection::new(3, tail, tail + 4),
                ],
                1,
                cx,
            );
        });
    });
    visual.run_until_parked();
    for scroll in [false, true] {
        if scroll {
            visual.update(|_, cx| {
                view.input.update(cx, |state, cx| {
                    state.update_scroll_offset(
                        Some(point(
                            px(0.),
                            -state.last_layout.as_ref().unwrap().line_height,
                        )),
                        cx,
                    );
                });
            });
            visual.run_until_parked();
        }
        visual.update(|window, cx| {
            let (layout, mut bounds) = view.input.read_with(cx, |state, _| {
                (
                    state.last_layout.clone().unwrap(),
                    state.last_bounds.unwrap(),
                )
            });
            let carets = view.input.read_with(cx, |state, _| {
                crate::input::element::TextElement::<EditorMode>::secondary_caret_bounds(
                    state, &layout, &bounds,
                )
            });
            assert_eq!(carets.len(), 2);
            let expected = view.input.read_with(cx, |state, _| state.selections());
            view.input.read_with(cx, |state, _| {
                for (caret, selection) in carets.iter().zip(expected.iter().skip(1)) {
                    let (offset, affinity) =
                        state.index_for_mouse_position(point(caret.left(), caret.center().y));
                    assert_eq!(offset, selection.head());
                    if selection.line_end_affinity() {
                        assert!(affinity);
                    }
                }
                assert!(!layout.visible_buffer_lines.contains(&2));
            });
            let element = crate::input::element::TextElement::new(view.input.clone());
            let paths = element.layout_selections(&layout, &mut bounds, window, cx);
            assert_eq!(paths.len(), 1);
            assert!((paths[0].bounds.right() - carets[1].left()).abs() < px(0.01));
            assert!(
                paths[0].bounds.top() <= carets[1].center().y
                    && carets[1].center().y < paths[0].bounds.bottom()
            );
        });
    }
}

struct MultiCursorLanguage;
impl crate::input::InputHighlighter for MultiCursorLanguage {
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
        text: &Rope,
        range: Range<usize>,
        _: &str,
    ) -> Option<crate::input::NewlineIndent> {
        let row = text.offset_to_point(range.start).row;
        let indent: String = text
            .slice_line(row)
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        if text.char_at(range.start.saturating_sub(1)) == Some(':') {
            Some(crate::input::NewlineIndent::new(format!("{indent}    ")))
        } else {
            Some(
                crate::input::NewlineIndent::new(format!("{indent}  ")).with_closing_indent(indent),
            )
        }
    }
    fn closing_brace_indent(&self, text: &Rope, offset: usize) -> Option<String> {
        Some(
            if text.offset_to_point(offset).row == 1 {
                ""
            } else {
                "  "
            }
            .into(),
        )
    }
    fn comment_syntax(&self, _: &Rope, _: Range<usize>) -> Option<crate::input::CommentSyntax> {
        Some(crate::input::CommentSyntax::line("//"))
    }
}

#[gpui::test]
fn multi_cursor_local_language_newline_closer_and_unique_comments(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("{}\r\n    {}\r\nif ready:", window, cx);
                *state.mode.highlighter().unwrap().borrow_mut() =
                    Some(Box::new(MultiCursorLanguage));
                let end = state.text.len();
                state.set_selections(
                    vec![
                        EditorSelection::new(1, 1, 1),
                        EditorSelection::new(2, 9, 9),
                        EditorSelection::new(3, end, end),
                    ],
                    2,
                    cx,
                );
                let before = state.selections();
                state.enter(
                    &Enter {
                        secondary: false,
                        shift: false,
                    },
                    window,
                    cx,
                );
                assert_eq!(
                    state.value(),
                    "{\r\n  \r\n}\r\n    {\r\n      \r\n    }\r\nif ready:\r\n    "
                );
                let after = state.selections();
                assert_eq!(
                    after.iter().map(EditorSelection::head).collect::<Vec<_>>(),
                    vec![5, 23, 47]
                );
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), before);
                state.redo(&Redo, window, cx);
                assert_eq!(state.selections(), after);
                state.set_value("{\n    \n  {\n      \n  }\n}", window, cx);
                *state.mode.highlighter().unwrap().borrow_mut() =
                    Some(Box::new(MultiCursorLanguage));
                state.set_selections(
                    vec![
                        EditorSelection::new(1, 6, 6),
                        EditorSelection::new(2, 17, 17),
                    ],
                    2,
                    cx,
                );
                let before = state.selections();
                state.replace_text_in_range(None, "}", window, cx);
                assert_eq!(state.value(), "{\n}\n  {\n  }\n  }\n}");
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), before);
                state.set_value("{\n      \n}", window, cx);
                *state.mode.highlighter().unwrap().borrow_mut() =
                    Some(Box::new(MultiCursorLanguage));
                state.set_selections(
                    vec![EditorSelection::new(1, 4, 4), EditorSelection::new(2, 6, 6)],
                    2,
                    cx,
                );
                state.replace_text_in_range(None, "}", window, cx);
                assert_eq!(state.value(), "{\n}  }  \n}");
                assert_eq!(state.selections().len(), 2);
                state.set_value("abc\r\nxyz", window, cx);
                *state.mode.highlighter().unwrap().borrow_mut() =
                    Some(Box::new(MultiCursorLanguage));
                state.set_selections(
                    vec![
                        EditorSelection::new(1, 0, 0),
                        EditorSelection::new(2, 3, 1),
                        EditorSelection::new(3, 5, 5),
                    ],
                    2,
                    cx,
                );
                let before = state.selections();
                state.toggle_comment(&ToggleComment, window, cx);
                assert_eq!(state.value(), "// abc\r\n// xyz");
                assert_eq!(state.selections().len(), 3);
                assert!(state.selections()[1].anchor() > state.selections()[1].head());
                state.undo(&Undo, window, cx);
                assert_eq!(state.selections(), before);
            });
        })
        .unwrap();
}

#[gpui::test]
fn multi_cursor_occurrence_shortcuts_do_not_bind_plain_inputs(cx: &mut TestAppContext) {
    fn check<M: InputModeKind>(view: InputView<M>, cx: &mut TestAppContext) {
        let mut visual = VisualTestContext::from_window(view.window_handle.into(), cx);
        visual.update(|window, cx| {
            view.input.update(cx, |state, cx| {
                state.set_value("one one", window, cx);
                state.set_selected_range(0..3, cx);
                state.focus(window, cx);
            });
        });
        visual.run_until_parked();
        #[cfg(target_os = "macos")]
        visual.simulate_keystrokes("cmd-d cmd-shift-l");
        #[cfg(not(target_os = "macos"))]
        visual.simulate_keystrokes("ctrl-d ctrl-shift-l");
        visual.update(|_, cx| {
            view.input.read_with(cx, |state, _| {
                assert_eq!(state.value(), "one one");
                assert_eq!(state.selected_range(), 0..3);
                assert!(!state.selection_set.has_secondary());
            });
        });
    }
    let input = InputView::build(cx, |state| state);
    check(input, cx);
    let textarea = InputView::build_textarea(cx, |state| state);
    check(textarea, cx);
}

#[gpui::test]
fn multi_cursor_next_occurrence_reveals_offscreen_without_moving_primary(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut visual = VisualTestContext::from_window(view.window_handle.into(), cx);
    visual.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value(format!("one\n{}one", "other\n".repeat(150)), window, cx);
            state.set_selected_range(0..3, cx);
            state.focus(window, cx);
        });
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.select_next_occurrence(&SelectNextOccurrence, window, cx);
            assert_eq!(state.selected_range(), 0..3);
            assert!(state.deferred_scroll_offset.unwrap().y < px(0.));
        });
    });
    visual.run_until_parked();
    visual.update(|_, cx| {
        view.input.read_with(cx, |state, _| {
            assert_eq!(state.selected_range(), 0..3);
            assert!(
                state
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .visible_buffer_lines
                    .contains(&151)
            );
            assert_eq!(
                crate::input::element::TextElement::<EditorMode>::secondary_caret_bounds(
                    state,
                    state.last_layout.as_ref().unwrap(),
                    &state.last_bounds.unwrap()
                )
                .len(),
                1
            );
        });
    });
}

#[gpui::test]
fn multi_cursor_word_at_eof_selects_then_adds_occurrences(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    view.window_handle
        .update(cx, |_, window, cx| {
            view.input.update(cx, |state, cx| {
                for (source, caret, expected) in [
                    ("one one", 3, 0..3),
                    ("one\n", 3, 0..3),
                    ("one  one", 4, 4..4),
                ] {
                    state.set_value(source, window, cx);
                    state.set_selected_range(caret..caret, cx);
                    state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                    assert_eq!(state.selected_range(), expected);
                }
                for source in ["one", "one one", "rök rök", "a\u{301} a\u{301}"] {
                    state.set_value(source, window, cx);
                    let end = state.text.len();
                    state.set_selected_range(end..end, cx);
                    state.select_next_occurrence(&SelectNextOccurrence, window, cx);
                    let start = source.rfind(' ').map(|ix| ix + 1).unwrap_or(0);
                    assert_eq!(state.selected_range(), start..end);
                    state.select_all_occurrences(&SelectAllOccurrences, window, cx);
                    assert_eq!(state.selections().len(), if start == 0 { 1 } else { 2 });
                }
            });
        })
        .unwrap();
}
