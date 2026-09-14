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
