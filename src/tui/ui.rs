//! UI rendering — draws the TUI layout each frame.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::{App, Focus, Role};
use super::theme;

/// Main render function — called every frame.
pub fn render(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Dynamic input height: 3 lines base, grows with newlines up to 7 (5 visible lines + border)
    let newline_count = app.input.chars().filter(|c| *c == '\n').count();
    let input_height = (newline_count as u16 + 3).min(7); // 3 base (1 line + border), max 7 (5 lines + border)

    // Top-level layout: status bar + main content + input
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),           // status bar
            Constraint::Min(5),             // main content
            Constraint::Length(input_height), // input (dynamic)
        ])
        .split(size);

    render_status_bar(frame, app, outer[0]);
    render_main_content(frame, app, outer[1]);
    render_input(frame, app, outer[2]);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let thinking = if app.thinking { " [thinking...]" } else { "" };
    let mode_label = app.mode.label();

    // Show round info if max_rounds is set
    let round_info = if app.max_rounds > 0 {
        format!(" | round {}/{}", app.current_round, app.max_rounds)
    } else {
        String::new()
    };

    let status = format!(
        " Mithril | {} | {}{} | {}{}  tools:{}",
        app.fellowship_name, mode_label, round_info, app.session_id, thinking,
        app.tool_call_count
    );

    // Pad or truncate to fill the width
    let width = area.width as usize;
    let display = if status.len() >= width {
        status[..width].to_string()
    } else {
        format!("{:<width$}", status, width = width)
    };

    let bar = Paragraph::new(display).style(theme::status_bar());
    frame.render_widget(bar, area);
}

fn render_main_content(frame: &mut Frame, app: &App, area: Rect) {
    if app.sidebar_visible && area.width > 60 {
        // Split: chat (70%) + sidebar (30%)
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);

        render_chat_panel(frame, app, split[0]);
        render_sidebar(frame, app, split[1]);
    } else {
        render_chat_panel(frame, app, area);
    }
}

fn render_chat_panel(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Chat {
        theme::border_focused()
    } else {
        theme::border_style()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Chat ")
        .title_style(Style::default().fg(theme::ACCENT));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build chat lines
    let mut lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        match &msg.role {
            Role::User => {
                lines.push(Line::from(vec![
                    Span::styled("▶ ", theme::user_style()),
                    Span::styled(&msg.content, theme::user_style()),
                ]));
                lines.push(Line::from(""));
            }
            Role::Assistant => {
                // Wrap long assistant messages
                for text_line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(text_line, theme::assistant_style())));
                }
                lines.push(Line::from(""));
            }
            Role::Tool { name, success } => {
                let icon = if *success { "⚙" } else { "✗" };
                let style = theme::tool_style(*success);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", icon), style),
                    Span::styled(name, Style::default().fg(theme::WARNING)),
                    Span::styled(" → ", style),
                    Span::styled(&msg.content, style),
                ]));
            }
            Role::System => {
                lines.push(Line::from(Span::styled(
                    format!("  ⚡ {}", &msg.content),
                    theme::system_style(),
                )));
                lines.push(Line::from(""));
            }
            Role::AgentTrace { agent, detail } => {
                // Dimmed trace line showing agent activity
                lines.push(Line::from(Span::styled(
                    format!("  ┄┄ {} ┄┄ {}", agent, detail),
                    Style::default().fg(theme::DIM),
                )));
            }
            Role::Summary { rounds, tokens } => {
                // Dimmed summary line
                lines.push(Line::from(Span::styled(
                    format!("  ✓ {} rounds | {}", rounds, tokens),
                    Style::default().fg(theme::DIM),
                )));
                lines.push(Line::from(""));
            }
        }
    }

    // Thinking indicator
    if app.thinking {
        lines.push(Line::from(Span::styled(
            "  ⛏ thinking...",
            theme::thinking_style(),
        )));
    }

    // Scroll: show last N lines that fit
    let visible_height = inner.height as usize;
    let total_lines = lines.len();
    let scroll = app.scroll_offset as usize;
    let start = if total_lines > visible_height + scroll {
        total_lines - visible_height - scroll
    } else {
        0
    };

    let visible_lines: Vec<Line> = lines.into_iter().skip(start).take(visible_height).collect();
    let chat = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
    frame.render_widget(chat, inner);
}

fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Sidebar {
        theme::border_focused()
    } else {
        theme::border_style()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Info ")
        .title_style(Style::default().fg(theme::ACCENT2));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Files touched
    lines.push(Line::from(Span::styled("📁 Files", theme::sidebar_title())));
    if app.files_touched.is_empty() {
        lines.push(Line::from(Span::styled("  (none yet)", Style::default().fg(theme::DIM))));
    } else {
        for f in app.files_touched.iter().rev().take(10) {
            lines.push(Line::from(Span::styled(
                format!("  {}", f),
                theme::file_path_style(),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("🤖 Fellowship", theme::sidebar_title())));
    lines.push(Line::from(format!("  {}", app.fellowship_name)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("📊 Stats", theme::sidebar_title())));
    lines.push(Line::from(format!("  Sent: {}", app.messages.iter().filter(|m| m.role == super::app::Role::User).count())));
    lines.push(Line::from(format!("  Tools: {}", app.tool_call_count)));
    lines.push(Line::from(format!("  Iterations: {}", app.iteration_count)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("🪙 Tokens (est.)", theme::sidebar_title())));
    lines.push(Line::from(format!("  ~{}", app.estimated_tokens_display())));

    let sidebar = Paragraph::new(lines);
    frame.render_widget(sidebar, inner);
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let border_style = if app.focus == Focus::Input {
        theme::border_focused()
    } else {
        theme::border_style()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" > ")
        .title_style(Style::default().fg(theme::ACCENT));

    let input_text = if app.input.is_empty() && !app.thinking {
        Paragraph::new(Span::styled(
            "type a message... (/ for commands)",
            Style::default().fg(theme::DIM),
        ))
    } else {
        Paragraph::new(Span::styled(&*app.input, theme::input_style()))
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(input_text, inner);

    // Render command suggestions popup above input
    if !app.suggestions.is_empty() && app.focus == Focus::Input {
        let suggestions_height = app.suggestions.len().min(6) as u16 + 2; // +2 for borders
        let popup_area = Rect {
            x: area.x,
            y: area.y.saturating_sub(suggestions_height),
            width: area.width.min(40),
            height: suggestions_height,
        };

        let items: Vec<Line> = app.suggestions.iter().enumerate().map(|(i, cmd)| {
            let desc = super::app::COMMANDS.iter()
                .find(|(c, _)| c == cmd)
                .map(|(_, d)| *d)
                .unwrap_or("");
            let style = if i == app.suggestion_index {
                Style::default().fg(theme::ACCENT).add_modifier(ratatui::style::Modifier::BOLD)
            } else {
                Style::default().fg(theme::DIM)
            };
            Line::from(vec![
                Span::styled(format!(" {} ", cmd), style),
                Span::styled(desc, Style::default().fg(theme::DIM)),
            ])
        }).collect();

        let popup = Paragraph::new(items)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::ACCENT2)));
        frame.render_widget(popup, popup_area);
    }

    // Set cursor position
    if app.focus == Focus::Input {
        frame.set_cursor_position((
            inner.x + app.cursor as u16,
            inner.y,
        ));
    }
}
