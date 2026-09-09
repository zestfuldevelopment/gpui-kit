#[gpui::test]
fn mouse_word_drag_handles_whitespace_unicode_eof_and_masking(cx: &mut TestAppContext) {
    let view = InputView::build(cx, |state| state);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("one  rök", window, cx);
            state.select_word(1, window, cx);
            for (offset, expected) in [(4, 0..5), (7, 0..9), (9, 0..9), (1, 0..3)] {
                state.select_by_mouse(offset, false, cx);
                assert_eq!(state.selected_range(), expected);
            }
            state.masked = true;
            state.select_word(1, window, cx);
            state.select_by_mouse(0, false, cx);
            assert_eq!(state.selected_range(), 0..9);
            state.select_by_mouse(9, false, cx);
            assert_eq!(state.selected_range(), 0..9);
        });
    });
}

#[gpui::test]
fn mouse_line_drag_keeps_blank_lines_crlf_and_eof(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("one\r\n\r\ntwo\r\n", window, cx);
            state.select_line(8, window, cx);
            // Preserve the existing logical-line convention: LF is excluded,
            // while its preceding CR is part of line_range.
            assert_eq!(state.selected_range(), 7..11);
            for (offset, expected) in [(5, 5..11), (1, 0..11), (8, 7..11), (12, 7..12)] {
                state.select_by_mouse(offset, false, cx);
                assert_eq!(state.selected_range(), expected);
            }
        });
    });
}

#[gpui::test]
fn ime_edit_cancels_mouse_selection_anchor(cx: &mut TestAppContext) {
    let view = InputView::new(cx);
    let mut cx = VisualTestContext::from_window(view.window_handle.into(), cx);
    cx.update(|window, cx| {
        view.input.update(cx, |state, cx| {
            state.set_value("alpha\nbeta", window, cx);
            state.select_line(8, window, cx);
            state.selecting = true;
            state.replace_and_mark_text_in_range(None, "x", Some(1..1), window, cx);
            assert_eq!(state.value(), "alpha\nx");
            assert!(!state.selecting);
            assert!(state.mouse_selection.is_none());
        });
    });
}
