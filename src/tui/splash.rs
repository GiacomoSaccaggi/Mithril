//! Mithril startup splash — fullscreen dwarf mining animation.

use std::io;
use std::thread;
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Play the fullscreen splash animation.
pub fn play_splash(terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>) {
    let frames: Vec<(&dyn Fn(Rect) -> Vec<Line<'static>>, u64)> = vec![
        (&frame_idle, 1200),
        (&frame_charge, 700),
        (&frame_strike1, 600),
        (&frame_charge2, 500),
        (&frame_strike2, 600),
        (&frame_charge3, 500),
        (&frame_strike3, 700),
        (&frame_explode, 900),
        (&frame_final, 4000),
    ];

    for (render_fn, duration) in &frames {
        let _ = terminal.draw(|f| {
            let area = f.area();
            let lines = render_fn(area);
            let p = Paragraph::new(lines);
            f.render_widget(p, area);
        });
        thread::sleep(Duration::from_millis(*duration));
    }
}

fn fullscreen_frame(area: Rect, art_lines: &[&str], fg: Color, bright: Color, fill: char) -> Vec<Line<'static>> {
    let art_height = art_lines.len();
    let v_offset = (area.height as usize).saturating_sub(art_height) / 2;
    let width = area.width as usize;

    let fill_line: String = std::iter::repeat(fill).take(width).collect();
    let style = Style::default().fg(fg);
    let bright_style = Style::default().fg(bright);
    let fill_style = Style::default().fg(Color::Rgb(30, 30, 40));

    (0..area.height as usize)
        .map(|row| {
            if row >= v_offset && row < v_offset + art_height {
                let art_row = row - v_offset;
                let text = art_lines[art_row];
                let text_len = text.chars().count();
                let h_pad = width.saturating_sub(text_len) / 2;
                let padded = format!("{:>w$}{}", "", text, w = h_pad);
                let full: String = if padded.chars().count() < width {
                    format!("{:<w$}", padded, w = width)
                } else {
                    padded
                };
                let line_style = if text.contains('✦') || text.contains("MITHRIL") || text.contains('█') || text.contains('*') || text.contains("CRACK") || text.contains("CLANG") {
                    bright_style
                } else {
                    style
                };
                Line::from(Span::styled(full, line_style))
            } else {
                Line::from(Span::styled(fill_line.clone(), fill_style))
            }
        })
        .collect()
}

// === IDLE: dwarf with pickaxe at side, facing rock ===
fn frame_idle(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                          _▄▄▄▄▄_",
        "                        ▐░░░░░░░░░▌",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓       ▐ ●       ● ▌",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓╾━━━━╬════▐▓▓▓▓▓▐════╬",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
    ];
    fullscreen_frame(area, &art, Color::Rgb(140, 145, 160), Color::Rgb(180, 190, 210), '▓')
}

// === CHARGE: pickaxe raised above head ===
fn frame_charge(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                               E━━━╗",
        "                          _▄▄▄▄▄_  ║",
        "                        ▐░░░░░░░░░▌╱",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓       ▐ ●       ● ▌",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓     ║    ▐▓▓▓▓▓▐     ║",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
    ];
    fullscreen_frame(area, &art, Color::Rgb(150, 155, 170), Color::Rgb(200, 205, 220), '▓')
}

// === STRIKE 1: pickaxe hits rock, sparks ===
fn frame_strike1(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                                          · * · ·",
        "                                         * · * · *",
        "                          _▄▄▄▄▄_",
        "                        ▐░░░░░░░░░▌",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓       ▐ ●       ● ▌",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓╾━━━━╬════▐▓▓▓▓▓▐════╬",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
        "              *** C L A N G ! ***",
    ];
    fullscreen_frame(area, &art, Color::Rgb(200, 210, 220), Color::Rgb(255, 255, 255), '░')
}

// === CHARGE 2: raise again ===
fn frame_charge2(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                               E━━━╗",
        "                          _▄▄▄▄▄_  ║",
        "                        ▐░░░░░░░░░▌╱",
        "   ▓▓▓▓▓▓▓▓╱▓▓▓       ▐ ●       ● ▌",
        " ▓▓▓▓▓▓▓╱▓▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓▓▓▓▓╱▓▓▓▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓▓▓╱▓▓▓▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓     ║    ▐▓▓▓▓▓▐     ║",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
    ];
    fullscreen_frame(area, &art, Color::Rgb(180, 190, 200), Color::Rgb(240, 245, 255), '░')
}

// === STRIKE 2: crack appears ===
fn frame_strike2(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                                        · * · * · ·",
        "                                       * · * · * · *",
        "                          _▄▄▄▄▄_",
        "                        ▐░░░░░░░░░▌",
        "   ▓▓▓▓▓▓▓▓╱▓▓▓       ▐ ●       ● ▌",
        " ▓▓▓▓▓▓▓╱▓▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓▓▓▓▓╱▓▓▓▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓▓▓╱▓▓▓▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓╾━━━━╬════▐▓▓▓▓▓▐════╬",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
        "",
        "              *** C R A C K ! ***",
    ];
    fullscreen_frame(area, &art, Color::Rgb(200, 210, 220), Color::Rgb(255, 255, 255), '░')
}

// === CHARGE 3: raise again, more cracks, light inside ===
fn frame_charge3(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                               E━━━╗",
        "                          _▄▄▄▄▄_  ║",
        "                        ▐░░░░░░░░░▌╱",
        "   ▓▓╱▓╱▓▓▓▓▓▓▓       ▐ ●       ● ▌",
        " ▓▓╱▓╱▓▓· ▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓╱▓╱▓▓·✦·▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓╱▓▓▓· ▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓     ║    ▐▓▓▓▓▓▐     ║",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
    ];
    fullscreen_frame(area, &art, Color::Rgb(190, 200, 215), Color::Rgb(240, 248, 255), '░')
}

// === STRIKE 3: deep cracks, light shining through ===
fn frame_strike3(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "                                      * · * · * · * ·",
        "                                     · * · * · * · * ·",
        "                          _▄▄▄▄▄_",
        "                        ▐░░░░░░░░░▌",
        "   ▓▓╱▓╱▓▓▓▓▓▓▓       ▐ ●       ● ▌",
        " ▓▓╱▓╱▓▓· ▓▓▓▓▓▓▓     ▐     ◆     ▌",
        "▓▓╱▓╱▓▓·✦·▓▓▓▓▓▓▓▓    ▐  ▄▄▄▄▄▄▄  ▌",
        "▓▓▓╱▓▓▓· ▓▓▓▓▓▓▓▓▓     ▀▌///////▐▀",
        " ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▌///////▐",
        "  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ╔════╧═══════╧════╗",
        "   ▓▓▓▓▓▓▓▓▓▓▓▓▓   ║    ▐▓▓▓▓▓▐     ║",
        "    ▓▓▓▓▓▓▓▓▓▓▓╾━━━━╬════▐▓▓▓▓▓▐════╬",
        "     ▓▓▓▓▓▓▓▓       ╚════╧══╤══╧════╝",
        "      ▓▓▓▓▓               ▐  │  ▐",
        "░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓░│░▓▓▓░░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒████░│░████▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
        "",
        "            *** C R A C K K ! ! ***",
    ];
    fullscreen_frame(area, &art, Color::Rgb(210, 220, 230), Color::Rgb(255, 255, 255), '░')
}

// === EXPLODE: rock shatters ===
fn frame_explode(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "",
        "         *         ·    *    ·         *",
        "    ·        *    ╲  │  ╱    *        ·",
        "         ╲    ·  ──── ✦ ────  ·    ╱",
        "    ╲    ·    *   ╱  │  ╲   *    ·    ╱",
        "         *    ·         *         ·",
        "    ·    *    ·    *    ·",
        "",
        "       * * * *  C R A C K ! ! *  * * *",
        "",
        "                  _▄▄▄▄▄_",
        "                ▐░░░░░░░░░▌",
        "               ▐ ●       ● ▌",
        "               ▐  ▄▄▄▄▄▄▄  ▌",
        "                ▀▌///////▐▀",
        "            ╔════╧═══════╧════╗",
        "            ║    ▐▓▓▓▓▓▐     ║",
        "            ╚════╧══╤══╧════╝",
        "                ▓▓▓ │ ▓▓▓",
        "░░░░░░░░░░░░░░░████░│░████░░░░░░░░░░░░░░░",
        "▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
    ];
    fullscreen_frame(area, &art, Color::Rgb(255, 255, 255), Color::Rgb(255, 255, 220), ' ')
}

// === FINAL: Mithril title + dwarf + quote ===
fn frame_final(area: Rect) -> Vec<Line<'static>> {
    let art = vec![
        "",
        "",
        "  ███╗   ███╗██╗████████╗██╗  ██╗██████╗ ██╗██╗      ",
        "  ████╗ ████║██║╚══██╔══╝██║  ██║██╔══██╗██║██║      ",
        "  ██╔████╔██║██║   ██║   ███████║██████╔╝██║██║      ",
        "  ██║╚██╔╝██║██║   ██║   ██╔══██║██╔══██╗██║██║      ",
        "  ██║ ╚═╝ ██║██║   ██║   ██║  ██║██║  ██║██║███████╗ ",
        "  ╚═╝     ╚═╝╚═╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝",
        "",
        "",
        "                     _▄▄▄▄▄_",
        "                   ▐░░░░░░░░░▌",
        "                  ▐ ✦       ✦ ▌",
        "                  ▐  ▄▄▄▄▄▄▄  ▌",
        "                   ▀▌///////▐▀",
        "",
        "",
        "   \"...light and yet harder than tempered steel.\"",
        "                                   — J.R.R. Tolkien",
        "",
        "",
    ];
    fullscreen_frame(area, &art, Color::Rgb(200, 225, 255), Color::Rgb(235, 245, 255), '█')
}
