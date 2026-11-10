//! GPUI views: the application window ([`app::AetherApp`]), the player
//! ([`player::PlayerView`]) and a text field.
pub mod app;
pub mod field;
pub mod player;

use gpui::{
    App, ClickEvent, ElementId, FontWeight, Rgba, SharedString, Window, div, prelude::*, px,
};

pub mod theme {
    use gpui::Rgba;

    const fn hex(value: u32) -> Rgba {
        Rgba {
            r: ((value >> 16) & 0xff) as f32 / 255.0,
            g: ((value >> 8) & 0xff) as f32 / 255.0,
            b: (value & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const BACKGROUND: Rgba = hex(0x0d0f14);
    pub const SURFACE: Rgba = hex(0x161a22);
    pub const RAISED: Rgba = hex(0x1f2430);
    pub const BORDER: Rgba = hex(0x2b3140);
    pub const TEXT: Rgba = hex(0xeef1f6);
    pub const MUTED: Rgba = hex(0x8d95a5);
    pub const ACCENT: Rgba = hex(0x7c8cff);
    pub const ACCENT_TEXT: Rgba = hex(0x0d0f14);
    pub const DANGER: Rgba = hex(0xff7a7a);
    pub const OVERLAY: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.65,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Primary,
    Secondary,
    Quiet,
}

/// A clickable label; `id` must be unique among its siblings.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tone: Tone,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (background, text, hover): (Rgba, Rgba, Rgba) = match tone {
        Tone::Primary => (theme::ACCENT, theme::ACCENT_TEXT, theme::TEXT),
        Tone::Secondary => (theme::RAISED, theme::TEXT, theme::BORDER),
        Tone::Quiet => (
            Rgba {
                a: 0.0,
                ..theme::BACKGROUND
            },
            theme::TEXT,
            theme::RAISED,
        ),
    };
    div()
        .id(id.into())
        .px_4()
        .h(px(36.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(background)
        .text_color(text)
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(on_click)
        .child(label.into())
}

/// A thin progress bar, `fraction` in 0..=1.
pub fn progress_bar(fraction: f32) -> impl IntoElement {
    div()
        .w_full()
        .h(px(4.))
        .rounded_full()
        .bg(theme::BORDER)
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(theme::ACCENT),
        )
}
