//! A one-line text field: typing, paste, backspace, Enter to submit and Tab
//! to move on. GPUI ships no text input; IME composition is not supported
//! (server addresses, logins and passwords do not need it).
use super::theme;
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

pub enum FieldEvent {
    Submit,
    Next,
}

pub struct TextField {
    focus: FocusHandle,
    value: String,
    placeholder: SharedString,
    secret: bool,
}

impl EventEmitter<FieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// What a key does to a field's text; kept apart from GPUI for tests.
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    Insert(String),
    Backspace,
    Clear,
    Paste,
    Submit,
    Next,
    Ignore,
}

pub fn edit_for(key: &str, key_char: Option<&str>, control: bool, platform: bool) -> Edit {
    let command = control || platform;
    match key {
        "enter" => Edit::Submit,
        "tab" => Edit::Next,
        "backspace" if command => Edit::Clear,
        "backspace" => Edit::Backspace,
        "v" if command => Edit::Paste,
        _ if command => Edit::Ignore,
        _ => match key_char {
            Some(text) if !text.chars().any(char::is_control) => Edit::Insert(text.to_owned()),
            _ => Edit::Ignore,
        },
    }
}

impl TextField {
    pub fn new(cx: &mut Context<Self>, placeholder: &str, secret: bool) -> Self {
        Self {
            focus: cx.focus_handle(),
            value: String::new(),
            placeholder: placeholder.to_owned().into(),
            secret,
        }
    }
    pub fn text(&self) -> &str {
        &self.value
    }
    pub fn set_text(&mut self, value: &str, cx: &mut Context<Self>) {
        value.clone_into(&mut self.value);
        cx.notify();
    }
    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let edit = edit_for(
            &keystroke.key,
            keystroke.key_char.as_deref(),
            keystroke.modifiers.control,
            keystroke.modifiers.platform,
        );
        match edit {
            Edit::Insert(text) => self.value.push_str(&text),
            Edit::Backspace => {
                self.value.pop();
            }
            Edit::Clear => self.value.clear(),
            Edit::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    // One line only: an address or a password pasted with its newline.
                    self.value.extend(text.chars().filter(|c| !c.is_control()));
                }
            }
            Edit::Submit => {
                cx.emit(FieldEvent::Submit);
                return;
            }
            Edit::Next => {
                cx.emit(FieldEvent::Next);
                return;
            }
            Edit::Ignore => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let shown: SharedString = if self.value.is_empty() {
            self.placeholder.clone()
        } else if self.secret {
            "•".repeat(self.value.chars().count()).into()
        } else {
            self.value.clone().into()
        };
        div()
            .id("field")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_click(cx.listener(|this, _, window, _| window.focus(&this.focus)))
            .w_full()
            .h(px(40.))
            .px_3()
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            })
            .bg(theme::SURFACE)
            .cursor_text()
            .overflow_hidden()
            .child(
                div()
                    .whitespace_nowrap()
                    .text_color(if self.value.is_empty() {
                        theme::MUTED
                    } else {
                        theme::TEXT
                    })
                    .child(shown),
            )
            .when(focused, |field| {
                field.child(div().w(px(1.5)).h(px(18.)).ml(px(1.)).bg(theme::ACCENT))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_edits() {
        assert_eq!(
            edit_for("a", Some("a"), false, false),
            Edit::Insert("a".into())
        );
        assert_eq!(
            edit_for("2", Some("é"), false, false),
            Edit::Insert("é".into())
        );
        assert_eq!(
            edit_for("space", Some(" "), false, false),
            Edit::Insert(" ".into())
        );
        assert_eq!(edit_for("backspace", None, false, false), Edit::Backspace);
        assert_eq!(edit_for("backspace", None, true, false), Edit::Clear);
        assert_eq!(edit_for("v", Some("v"), true, false), Edit::Paste);
        assert_eq!(edit_for("v", Some("v"), false, true), Edit::Paste);
        assert_eq!(edit_for("c", Some("c"), true, false), Edit::Ignore);
        assert_eq!(edit_for("enter", None, false, false), Edit::Submit);
        assert_eq!(edit_for("tab", None, false, false), Edit::Next);
        assert_eq!(edit_for("escape", None, false, false), Edit::Ignore);
        assert_eq!(edit_for("x", Some("\u{1b}"), false, false), Edit::Ignore);
    }
}
