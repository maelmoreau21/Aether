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

    // Halo palette (apps/web/src/lib/styles/halo.css, dark theme).
    pub const BACKGROUND: Rgba = hex(0x0b0e18);
    pub const SURFACE: Rgba = hex(0x11151f);
    pub const RAISED: Rgba = hex(0x171c28);
    pub const BORDER: Rgba = white(0.09);
    pub const PANEL: Rgba = white(0.055);
    pub const PANEL_2: Rgba = white(0.09);
    pub const PANEL_3: Rgba = white(0.14);
    pub const TEXT: Rgba = hex(0xeef1f8);
    pub const MUTED: Rgba = hex(0xa2a9ba);
    pub const MUTED_2: Rgba = hex(0x8189a0);
    pub const ACCENT: Rgba = hex(0xff7b47);
    pub const ACCENT_SOFT: Rgba = Rgba {
        r: 1.0,
        g: 123.0 / 255.0,
        b: 71.0 / 255.0,
        a: 0.16,
    };
    /// Primary buttons are light, as on the web (`--btn`).
    pub const BUTTON: Rgba = hex(0xeef1f8);
    pub const BUTTON_TEXT: Rgba = hex(0x0b0e18);
    pub const BUTTON_HOVER: Rgba = hex(0xd9dde8);
    pub const DANGER: Rgba = hex(0xff6b6b);
    pub const DANGER_SOFT: Rgba = Rgba {
        r: 1.0,
        g: 107.0 / 255.0,
        b: 107.0 / 255.0,
        a: 0.14,
    };
    pub const OVERLAY: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.65,
    };

    const fn white(alpha: f32) -> Rgba {
        Rgba {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: alpha,
        }
    }
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
        Tone::Primary => (theme::BUTTON, theme::BUTTON_TEXT, theme::BUTTON_HOVER),
        Tone::Secondary => (theme::PANEL_2, theme::TEXT, theme::PANEL_3),
        Tone::Quiet => (
            Rgba {
                a: 0.0,
                ..theme::BACKGROUND
            },
            theme::TEXT,
            theme::PANEL_2,
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

/// The large full-width button of the sign-in screens (48 px, rounded like the
/// web's `btn-submit`), with the arrow of the web design on the primary one.
pub fn wide_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tone: Tone,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (background, text, hover): (Rgba, Rgba, Rgba) = match tone {
        Tone::Primary => (theme::BUTTON, theme::BUTTON_TEXT, theme::BUTTON_HOVER),
        Tone::Secondary | Tone::Quiet => (theme::PANEL, theme::TEXT, theme::PANEL_2),
    };
    div()
        .id(id.into())
        .w_full()
        .h(px(48.))
        .px_5()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded(px(16.))
        .bg(background)
        .text_color(text)
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(on_click)
        .child(label.into())
        .when(tone == Tone::Primary, |button| button.child("→"))
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
