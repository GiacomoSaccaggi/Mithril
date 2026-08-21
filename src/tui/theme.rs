//! Mithril TUI theme — dark forge aesthetic.

#![allow(dead_code)]
use ratatui::style::{Color, Modifier, Style};

// ── Mithril color palette ────────────────────────────────────────────────────
// Inspired by the dark of Moria, lit by the gleam of mithril-silver.

pub const BG: Color = Color::Rgb(15, 23, 42);        // slate-900
pub const SURFACE: Color = Color::Rgb(30, 41, 59);    // slate-800
pub const BORDER: Color = Color::Rgb(51, 65, 85);     // slate-700
pub const TEXT: Color = Color::Rgb(226, 232, 240);     // slate-200
pub const DIM: Color = Color::Rgb(148, 163, 184);     // slate-400
pub const ACCENT: Color = Color::Rgb(56, 189, 248);   // sky-400 (mithril gleam)
pub const ACCENT2: Color = Color::Rgb(167, 139, 250); // violet-400
pub const SUCCESS: Color = Color::Rgb(52, 211, 153);  // emerald-400
pub const ERROR: Color = Color::Rgb(248, 113, 113);   // red-400
pub const WARNING: Color = Color::Rgb(251, 146, 60);  // orange-400
pub const USER: Color = Color::Rgb(129, 140, 248);    // indigo-400

// ── Style helpers ────────────────────────────────────────────────────────────

pub fn status_bar() -> Style {
    Style::default().fg(DIM).bg(SURFACE)
}

pub fn input_style() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub fn border_style() -> Style {
    Style::default().fg(BORDER)
}

pub fn border_focused() -> Style {
    Style::default().fg(ACCENT)
}

pub fn user_style() -> Style {
    Style::default().fg(USER).add_modifier(Modifier::BOLD)
}

pub fn assistant_style() -> Style {
    Style::default().fg(TEXT)
}

pub fn tool_style(success: bool) -> Style {
    if success {
        Style::default().fg(DIM)
    } else {
        Style::default().fg(ERROR)
    }
}

pub fn system_style() -> Style {
    Style::default().fg(DIM).add_modifier(Modifier::ITALIC)
}

pub fn thinking_style() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::DIM)
}

pub fn sidebar_title() -> Style {
    Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)
}

pub fn file_path_style() -> Style {
    Style::default().fg(WARNING)
}
