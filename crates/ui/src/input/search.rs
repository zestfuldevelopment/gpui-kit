use rust_i18n::t;

use gpui::{
    App, AppContext as _, Context, Empty, Entity, FocusHandle, Focusable, Half,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels, Render, Styled,
    Subscription, WeakEntity, Window, actions, div, prelude::FluentBuilder as _,
};

use crate::{
    ActiveTheme, Disableable, ElementExt, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{
        Enter, Escape, IndentInline, Input, InputBaseState, InputEvent, InputState, OutdentInline,
        Replace, Search,
    },
    label::Label,
    tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};

const CONTEXT: &'static str = "SearchPanel";

actions!(input, [Tab, TabPrev]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", Tab, Some(CONTEXT)),
        KeyBinding::new("shift-tab", TabPrev, Some(CONTEXT)),
        KeyBinding::new("escape", Escape, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-f", Search, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-f", Search, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-f", Replace, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-h", Replace, Some(CONTEXT)),
    ]);
}

#[cfg(test)]
use gpui_base::input::SearchMatcher;

#[derive(Clone, Copy)]
#[cfg(test)]
enum MoveDirection {
    Up,
    Down,
}

#[cfg(test)]
fn next_scroll_direction(
    previous_match_ix: usize,
    current_match_ix: usize,
) -> Option<MoveDirection> {
    if current_match_ix <= previous_match_ix {
        None
    } else {
        Some(MoveDirection::Down)
    }
}

#[cfg(test)]
fn prev_scroll_direction(
    previous_match_ix: usize,
    current_match_ix: usize,
) -> Option<MoveDirection> {
    if current_match_ix >= previous_match_ix {
        None
    } else {
        Some(MoveDirection::Up)
    }
}

pub(super) struct SearchPanel<M: crate::input::overlay::OverlayMode> {
    editor: WeakEntity<InputBaseState<M>>,
    search_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    case_focus: FocusHandle,
    word_focus: FocusHandle,
    replace_mode_focus: FocusHandle,
    previous_focus: FocusHandle,
    next_focus: FocusHandle,
    replace_current_focus: FocusHandle,
    replace_all_focus: FocusHandle,
    close_focus: FocusHandle,
    session: gpui_base::input::SearchSession,
    input_width: Pixels,

    _subscriptions: Vec<Subscription>,
}

impl<M: crate::input::overlay::OverlayMode> SearchPanel<M> {
    pub(super) fn sync_session(
        &mut self,
        session: &gpui_base::input::SearchSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session = session.clone();
        if self.search_input.read(cx).value().as_ref() != session.query {
            self.search_input.update(cx, |input, cx| {
                input.set_value(session.query.clone(), window, cx)
            });
        }
        if self.replace_input.read(cx).value().as_ref() != session.replacement {
            self.replace_input.update(cx, |input, cx| {
                input.set_value(session.replacement.clone(), window, cx)
            });
        }
    }

    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        self.search_input
            .update(cx, |input, cx| input.select_all(window, cx));
    }

    pub(crate) fn new(
        editor: Entity<InputBaseState<M>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let replace_input = cx.new(|cx| InputState::new(window, cx));

        cx.new(|cx| {
            let _subscriptions = vec![
                cx.subscribe(&search_input, |this: &mut Self, _, ev: &InputEvent, cx| {
                    // Handle search input changes
                    match ev {
                        InputEvent::Change => {
                            this.update_search_query(cx);
                        }
                        _ => {}
                    }
                }),
                cx.subscribe(&replace_input, |this: &mut Self, _, ev: &InputEvent, cx| {
                    if matches!(ev, InputEvent::Change) {
                        let replacement = this.replace_input.read(cx).value();
                        let _ = this.editor.update(cx, |state, cx| {
                            state.set_search_replacement(replacement.to_string(), cx);
                        });
                    }
                }),
            ];

            Self {
                editor: editor.downgrade(),
                search_input,
                replace_input,
                case_focus: cx.focus_handle(),
                word_focus: cx.focus_handle(),
                replace_mode_focus: cx.focus_handle(),
                previous_focus: cx.focus_handle(),
                next_focus: cx.focus_handle(),
                replace_current_focus: cx.focus_handle(),
                replace_all_focus: cx.focus_handle(),
                close_focus: cx.focus_handle(),
                session: gpui_base::input::SearchSession::default(),
                input_width: Pixels::ZERO,
                _subscriptions,
            }
        })
    }

    fn update_search_query(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            state.set_search_query(query.to_string(), self.session.case_insensitive, cx);
        });
        cx.notify();
    }

    fn replaceable(&self, cx: &App) -> bool {
        self.editor
            .read_with(cx, |editor, _| editor.is_replaceable())
            .unwrap_or(false)
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_with_focus(true, window, cx);
    }

    pub(super) fn hide_with_focus(
        &mut self,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.open = false;
        let _ = self.editor.update(cx, |state, cx| state.close_search(cx));
        if focus_editor {
            if let Some(editor) = self.editor.upgrade() {
                editor.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
        cx.notify();
    }

    fn on_action_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
            && !self
                .replace_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            cx.propagate();
            return;
        }
        if action.shift {
            self.prev(window, cx);
        } else {
            self.next(window, cx);
        }
    }

    fn on_action_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    fn on_action_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(false, window, cx);
    }

    fn on_action_tab_prev(
        &mut self,
        _: &OutdentInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_focus(true, window, cx);
    }

    fn on_action_control_tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(false, window, cx);
    }

    fn on_action_control_tab_prev(
        &mut self,
        _: &TabPrev,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_focus(true, window, cx);
    }

    /// Keep the fixed order even if the focused button becomes disabled after
    /// replacement. The next Tab can then advance to the next enabled control.
    fn cycle_focus(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let replaceable = self.replaceable(cx);
        let replacing = self.session.replace_mode && replaceable;
        let matches = !self.session.matcher.is_empty();
        let controls = [
            (self.search_input.read(cx).focus_handle(cx), true),
            (self.replace_input.read(cx).focus_handle(cx), replacing),
            (self.case_focus.clone(), true),
            (self.word_focus.clone(), true),
            (self.replace_mode_focus.clone(), replaceable),
            (self.previous_focus.clone(), matches),
            (self.next_focus.clone(), matches),
            (self.replace_current_focus.clone(), replacing && matches),
            (self.replace_all_focus.clone(), replacing && matches),
            (self.close_focus.clone(), true),
        ];
        let current = controls
            .iter()
            .position(|(handle, _)| handle.is_focused(window))
            .unwrap_or(0);
        for step in 1..=controls.len() {
            let next = if reverse {
                (current + controls.len() - step) % controls.len()
            } else {
                (current + step) % controls.len()
            };
            if controls[next].1 {
                controls[next].0.focus(window, cx);
                break;
            }
        }
    }

    fn on_action_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .editor
            .update(cx, |state, cx| state.refocus_search(false, cx));
    }

    fn on_action_replace(&mut self, _: &Replace, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .editor
            .update(cx, |state, cx| state.refocus_search(true, cx));
    }

    /// Toggle the replace field, and move focus to the field that is going to be used.
    fn toggle_replace_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            return;
        }

        self.session.replace_mode = !self.session.replace_mode;
        let replace_mode = self.session.replace_mode;
        let _ = self.editor.update(cx, |state, cx| {
            state.set_search_replace_mode(replace_mode, cx);
        });
        let focus_handle = if self.session.replace_mode {
            self.replace_input.read(cx).focus_handle(cx)
        } else {
            self.search_input.read(cx).focus_handle(cx)
        };
        focus_handle.focus(window, cx);
        cx.notify();
    }

    fn prev(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.previous_search_match(cx);
        });
    }

    fn next(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.next_search_match(cx);
        });
    }

    fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.session.replace_mode = false;
            cx.notify();
            return;
        }

        let replacement = self.replace_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.replace_current_search_match(&replacement, window, cx);
        });
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.session.replace_mode = false;
            cx.notify();
            return;
        }

        let replacement = self.replace_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.replace_all_search_matches(&replacement, window, cx);
        });
    }
}

impl<M: crate::input::overlay::OverlayMode> Focusable for SearchPanel<M> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_input.read(cx).focus_handle(cx)
    }
}

impl<M: crate::input::overlay::OverlayMode> SearchPanel<M> {
    // Keep native debug builds within Windows' 1 MiB main-thread stack.
    // Build each region separately and erase its large builder type before
    // returning, so their temporary values do not share one render frame.
    fn render_search_options(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        h_flex()
            .gap_1()
            .child(
                Button::new("case-insensitive")
                    .track_focus(&self.case_focus)
                    .selected(!self.session.case_insensitive)
                    .toggled(!self.session.case_insensitive)
                    .xsmall()
                    .compact()
                    .text()
                    .icon(IconName::CaseSensitive)
                    .tooltip("Match Case")
                    .accessibility_label("Match Case")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.case_insensitive = !this.session.case_insensitive;
                        this.update_search_query(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("whole-word")
                    .track_focus(&self.word_focus)
                    .selected(self.session.whole_word)
                    .toggled(self.session.whole_word)
                    .xsmall()
                    .compact()
                    .text()
                    .label("Word")
                    .tooltip("Whole Word")
                    .accessibility_label("Whole Word")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let whole_word = !this.session.whole_word;
                        this.session.whole_word = whole_word;
                        let _ = this.editor.update(cx, |state, cx| {
                            state.set_search_whole_word(whole_word, cx);
                        });
                    })),
            )
            .into_any_element()
    }

    fn render_search_input(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_1()
            .gap_1()
            .child(
                Input::new(&self.search_input)
                    .aria_label("Find")
                    .focus_bordered(true)
                    .suffix(self.render_search_options(cx))
                    .small()
                    .w_full()
                    .shadow_none(),
            )
            .on_prepaint({
                let view = cx.entity();
                move |bounds, _, cx| view.update(cx, |r, _| r.input_width = bounds.size.width)
            })
            .into_any_element()
    }

    fn render_replace_mode_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let replacement_visibility_label = if self.session.replace_mode {
            "Hide replacement"
        } else {
            "Show replacement"
        };
        Button::new("replace-mode")
            .track_focus(&self.replace_mode_focus)
            .xsmall()
            .ghost()
            .icon(IconName::Replace)
            .accessibility_label(replacement_visibility_label)
            .tooltip(replacement_visibility_label)
            .selected(self.session.replace_mode)
            .toggled(self.session.replace_mode)
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_replace_mode(window, cx);
            }))
            .into_any_element()
    }

    fn render_previous_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        Button::new("prev")
            .track_focus(&self.previous_focus)
            .xsmall()
            .ghost()
            .icon(IconName::ChevronLeft)
            .accessibility_label("Previous match")
            .tooltip("Previous match (Shift+Enter)")
            .disabled(self.session.matcher.is_empty())
            .on_click(cx.listener(|this, _, window, cx| {
                this.prev(window, cx);
            }))
            .into_any_element()
    }

    fn render_next_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        Button::new("next")
            .track_focus(&self.next_focus)
            .xsmall()
            .ghost()
            .icon(IconName::ChevronRight)
            .accessibility_label("Next match")
            .tooltip("Next match (Enter)")
            .disabled(self.session.matcher.is_empty())
            .on_click(cx.listener(|this, _, window, cx| {
                this.next(window, cx);
            }))
            .into_any_element()
    }

    fn render_close_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        Button::new("close")
            .track_focus(&self.close_focus)
            .xsmall()
            .ghost()
            .icon(IconName::Close)
            .accessibility_label("Close find")
            .tooltip("Close find (Escape)")
            .on_click(cx.listener(|this, _, window, cx| {
                this.on_action_escape(&Escape, window, cx);
            }))
            .into_any_element()
    }

    fn render_replace_current_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        Button::new("replace-one")
            .track_focus(&self.replace_current_focus)
            .small()
            .label(t!("Input.Replace"))
            .accessibility_label("Replace current match")
            .tooltip("Replace current match with literal replacement text")
            .disabled(self.session.matcher.is_empty())
            .on_click(cx.listener(|this, _, window, cx| {
                this.replace_next(window, cx);
            }))
            .into_any_element()
    }

    fn render_replace_all_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        Button::new("replace-all")
            .track_focus(&self.replace_all_focus)
            .small()
            .label(t!("Input.Replace All"))
            .accessibility_label("Replace all matches")
            .tooltip("Replace all matches with literal replacement text")
            .disabled(self.session.matcher.is_empty())
            .on_click(cx.listener(|this, _, window, cx| {
                this.replace_all(window, cx);
            }))
            .into_any_element()
    }

    fn render_find_row(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let has_matches = !self.session.matcher.is_empty();
        let allow_replace = self.replaceable(cx);
        let match_label = if has_matches {
            format!(
                "Match {} of {}",
                self.session.matcher.current_match_index() + 1,
                self.session.matcher.len()
            )
        } else {
            "No matches".to_owned()
        };
        h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .id("find-label")
                    .w_12()
                    .flex_shrink_0()
                    .child(Label::new("Find"))
                    .managed_tooltip(|window, cx| {
                        Tooltip::new(
                            "Find matches literal text. Regular expressions are not interpreted.",
                        )
                        .build(window, cx)
                    }),
            )
            .child(self.render_search_input(cx))
            .when(allow_replace, |this| {
                this.child(self.render_replace_mode_button(cx))
            })
            .child(self.render_previous_button(cx))
            .child(self.render_next_button(cx))
            .child(
                Label::new(match_label)
                    .when(!has_matches, |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .text_left()
                    .min_w_16(),
            )
            .child(div().w_7())
            .child(self.render_close_button(cx))
            .into_any_element()
    }

    fn render_replace_row(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .gap_2()
            .child(
                div().id("replace-label").w_12().flex_shrink_0()
                    .child(Label::new("Replace"))
                    .managed_tooltip(|window, cx| {
                        Tooltip::new("Replacement text is inserted literally. Leave empty to delete matches.")
                            .build(window, cx)
                    }),
            )
            .child(
                Input::new(&self.replace_input)
                    .aria_label("Replace")
                    .focus_bordered(true)
                    .small()
                    .w(self.input_width)
                    .shadow_none(),
            )
            .child(
                self.render_replace_current_button(cx),
            )
            .child(
                self.render_replace_all_button(cx),
            )
            .into_any_element()
    }
}

impl<M: crate::input::overlay::OverlayMode> Render for SearchPanel<M> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.session.open {
            return Empty.into_any_element();
        }

        let allow_replace = self.replaceable(cx);
        if !allow_replace {
            self.session.replace_mode = false;
        }

        v_flex()
            .id("search-panel")
            .occlude()
            .track_focus(&self.focus_handle(cx))
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_action_enter))
            .on_action(cx.listener(Self::on_action_escape))
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_tab_prev))
            .on_action(cx.listener(Self::on_action_control_tab))
            .on_action(cx.listener(Self::on_action_control_tab_prev))
            .on_action(cx.listener(Self::on_action_replace))
            .on_action(cx.listener(Self::on_action_search))
            .font_family(cx.theme().font_family.clone())
            .items_center()
            .py_2()
            .px_3()
            .w_full()
            .gap_1()
            .bg(cx.theme().tokens.popover)
            .border_b_1()
            .rounded(cx.theme().radius.half())
            .border_color(cx.theme().border)
            .child(self.render_find_row(cx))
            .when_some(self.session.matcher.error(), |this, error| {
                this.child(Label::new(error.to_owned()).text_color(cx.theme().danger))
            })
            .when(self.session.replace_mode && allow_replace, |this| {
                this.child(self.render_replace_row(cx))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn test_search() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);

        assert_eq!(matcher.len(), 3);
        let mut matches = matcher.clone();
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next(), Some(18..20));
        assert_eq!(matches.next(), Some(23..25));
        assert_eq!(matches.current_match_index(), 2);
        assert_eq!(matches.next(), Some(15..17));
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next_back(), Some(23..25));
        assert_eq!(matches.current_match_index(), 2);
        assert_eq!(matches.next_back(), Some(18..20));
        assert_eq!(matches.current_match_index(), 1);
        assert_eq!(matches.next_back(), Some(15..17));
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next_back(), Some(23..25));

        matcher.update_query("IS", false);
        assert_eq!(matcher.len(), 0);
        assert_eq!(matcher.next(), None);
        assert_eq!(matcher.next_back(), None);
    }

    #[test]
    fn test_search_label() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);
        assert_eq!(matcher.label(), "1/3");
        matcher.next();
        assert_eq!(matcher.label(), "2/3");
        matcher.next();
        assert_eq!(matcher.label(), "3/3");
        matcher.next();
        assert_eq!(matcher.label(), "1/3");

        matcher.update_query("IS", false);
        assert_eq!(matcher.label(), "0/0");
    }

    #[test]
    fn test_select_range_start() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(".....aaaaa.....aaaaa.....aaaaa"));
        matcher.update_query("aaaaa", false);
        matcher.update_cursor_by_offset(0);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(5);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(12);
        assert_eq!(matcher.current_match_index(), 1);

        matcher.update_cursor_by_offset(16);
        assert_eq!(matcher.current_match_index(), 1);

        matcher.update_cursor_by_offset(30);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(31);
        assert_eq!(matcher.current_match_index(), 0);
    }

    #[test]
    fn test_next_scroll_direction_returns_down_without_wrap() {
        assert!(matches!(
            next_scroll_direction(0, 1),
            Some(MoveDirection::Down)
        ));
    }

    #[test]
    fn test_next_scroll_direction_returns_none_on_wrap() {
        assert!(next_scroll_direction(2, 0).is_none());
    }

    #[test]
    fn test_next_scroll_direction_returns_none_for_single_match() {
        assert!(next_scroll_direction(0, 0).is_none());
    }

    #[test]
    fn test_prev_scroll_direction_returns_up_without_wrap() {
        assert!(matches!(
            prev_scroll_direction(2, 1),
            Some(MoveDirection::Up)
        ));
    }

    #[test]
    fn test_prev_scroll_direction_returns_none_on_wrap() {
        assert!(prev_scroll_direction(0, 2).is_none());
    }

    #[test]
    fn test_prev_scroll_direction_returns_none_for_single_match() {
        assert!(prev_scroll_direction(0, 0).is_none());
    }
}
