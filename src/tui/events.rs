//! Event handling for the TUI — keyboard input, resize, etc.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::time::Duration;

use super::app::{App, Focus};

/// Possible actions after processing an event.
pub enum Action {
    /// Nothing happened
    None,
    /// User submitted input (send to LLM)
    Submit(String),
    /// User wants to exit
    Exit,
    /// Run a slash command
    Command(String),
}

/// Poll for events and update app state. Returns an action if needed.
pub fn handle_events(app: &mut App) -> std::io::Result<Action> {
    if !event::poll(Duration::from_millis(100))? {
        return Ok(Action::None);
    }

    match event::read()? {
        Event::Key(key) => Ok(handle_key(app, key)),
        Event::Mouse(mouse) => Ok(handle_mouse(app, mouse)),
        Event::Resize(_, _) => Ok(Action::None),
        _ => Ok(Action::None),
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) -> Action {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.scroll_offset = app.scroll_offset.saturating_add(3);
            Action::None
        }
        MouseEventKind::ScrollDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(3);
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    // Global shortcuts (work in any focus)
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Action::Exit,
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => return Action::Exit,
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
            app.sidebar_visible = !app.sidebar_visible;
            return Action::None;
        }
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
            app.messages.clear();
            app.scroll_offset = 0;
            return Action::None;
        }
        _ => {}
    }

    // Global scroll shortcuts (work in any focus)
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            app.scroll_offset = app.scroll_offset.saturating_add(10);
            return Action::None;
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(10);
            return Action::None;
        }
        (KeyModifiers::CONTROL, KeyCode::Up) => {
            app.scroll_offset = app.scroll_offset.saturating_add(3);
            return Action::None;
        }
        (KeyModifiers::CONTROL, KeyCode::Down) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(3);
            return Action::None;
        }
        _ => {}
    }

    // Focus-specific handling
    match app.focus {
        Focus::Input => handle_input_key(app, key),
        Focus::Chat => handle_chat_key(app, key),
        Focus::Sidebar => handle_sidebar_key(app, key),
    }
}

fn handle_input_key(app: &mut App, key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        // Multiline: Shift+Enter, Alt+Enter, or Ctrl+J inserts newline
        (KeyModifiers::SHIFT, KeyCode::Enter)
        | (KeyModifiers::ALT, KeyCode::Enter)
        | (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
            app.input.insert(app.cursor, '\n');
            app.cursor += 1;
            Action::None
        }
        // Enter: if suggestions visible → accept and send as command; else submit normally
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if app.thinking {
                return Action::None;
            }
            // If suggestion menu is showing, accept it and send immediately
            if !app.suggestions.is_empty() {
                app.accept_suggestion();
                if let Some(text) = app.submit_input() {
                    return Action::Command(text);
                }
                return Action::None;
            }
            if let Some(text) = app.submit_input() {
                if text.starts_with('/') {
                    return Action::Command(text);
                }
                return Action::Submit(text);
            }
            Action::None
        }
        // History navigation / suggestion navigation
        (KeyModifiers::NONE, KeyCode::Up) => {
            if !app.suggestions.is_empty() {
                app.suggestion_index = app.suggestion_index.saturating_sub(1);
            } else {
                app.history_up();
            }
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            if !app.suggestions.is_empty() {
                if app.suggestion_index + 1 < app.suggestions.len() {
                    app.suggestion_index += 1;
                }
            } else {
                app.history_down();
            }
            Action::None
        }
        // Tab: accept suggestion if visible, toggle Plan/Build if input empty, else focus switch
        (KeyModifiers::NONE, KeyCode::Tab) => {
            if !app.suggestions.is_empty() {
                app.accept_suggestion();
            } else {
                // Try #agent completion
                let agent_suggestions = app.get_agent_suggestions();
                if agent_suggestions.len() == 1 {
                    app.input = format!("#{} ", agent_suggestions[0]);
                    app.cursor = app.input.len();
                } else {
                    // Try @file completion
                    let file_suggestions = app.get_file_suggestions();
                    if file_suggestions.len() == 1 {
                        let text = &app.input[..app.cursor.min(app.input.len())];
                        if let Some(at_pos) = text.rfind('@') {
                            let replacement = format!("@{}", file_suggestions[0]);
                            app.input = format!("{}{}{}", &app.input[..at_pos], replacement, &app.input[app.cursor..]);
                            app.cursor = at_pos + replacement.len();
                        }
                    } else if app.input.is_empty() {
                        app.mode = app.mode.toggle();
                    }
                }
            }
            Action::None
        }
        // Escape
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.input.clear();
            app.cursor = 0;
            Action::None
        }
        // Character input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            app.input.insert(app.cursor, c);
            app.cursor += 1;
            app.update_suggestions();
            Action::None
        }
        // Backspace
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            if app.cursor > 0 {
                app.cursor -= 1;
                app.input.remove(app.cursor);
            }
            app.update_suggestions();
            Action::None
        }
        // Delete
        (KeyModifiers::NONE, KeyCode::Delete) => {
            if app.cursor < app.input.len() {
                app.input.remove(app.cursor);
            }
            Action::None
        }
        // Left/Right cursor
        (KeyModifiers::NONE, KeyCode::Left) => {
            app.cursor = app.cursor.saturating_sub(1);
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::Right) => {
            if app.cursor < app.input.len() {
                app.cursor += 1;
            }
            Action::None
        }
        // Home/End
        (KeyModifiers::NONE, KeyCode::Home) => {
            app.cursor = 0;
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::End) => {
            app.cursor = app.input.len();
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_chat_key(app: &mut App, key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        // Scroll
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            app.scroll_offset = app.scroll_offset.saturating_add(1);
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(1);
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            app.scroll_offset = app.scroll_offset.saturating_add(10);
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            app.scroll_offset = app.scroll_offset.saturating_sub(10);
            Action::None
        }
        // Tab → go to input
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.focus = Focus::Sidebar;
            Action::None
        }
        // Escape → back to input
        (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::NONE, KeyCode::Char('i')) => {
            app.focus = Focus::Input;
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_sidebar_key(app: &mut App, key: KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.focus = Focus::Input;
            Action::None
        }
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.focus = Focus::Input;
            Action::None
        }
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{App, ChatLine, Role, AgentMode};

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn make_app() -> App {
        App::new("test", "model", "session-id-12345678")
    }

    #[test]
    fn test_ctrl_c_exits() {
        let mut app = make_app();
        let action = handle_key(&mut app, make_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        matches!(action, Action::Exit);
    }

    #[test]
    fn test_ctrl_d_exits() {
        let mut app = make_app();
        let action = handle_key(&mut app, make_key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        matches!(action, Action::Exit);
    }

    #[test]
    fn test_ctrl_s_toggles_sidebar() {
        let mut app = make_app();
        assert!(!app.sidebar_visible); // starts hidden
        handle_key(&mut app, make_key(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(app.sidebar_visible);
        handle_key(&mut app, make_key(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(!app.sidebar_visible);
    }

    #[test]
    fn test_ctrl_l_clears_messages() {
        let mut app = make_app();
        app.messages.push(ChatLine {
            role: Role::User,
            content: "test".to_string(),
        });
        assert!(!app.messages.is_empty());
        handle_key(&mut app, make_key(KeyCode::Char('l'), KeyModifiers::CONTROL));
        assert!(app.messages.is_empty());
    }

    #[test]
    fn test_page_up_scrolls() {
        let mut app = make_app();
        app.scroll_offset = 0;
        handle_key(&mut app, make_key(KeyCode::PageUp, KeyModifiers::NONE));
        assert!(app.scroll_offset > 0);
    }

    #[test]
    fn test_page_down_scrolls() {
        let mut app = make_app();
        app.scroll_offset = 20;
        handle_key(&mut app, make_key(KeyCode::PageDown, KeyModifiers::NONE));
        assert!(app.scroll_offset < 20);
    }

    #[test]
    fn test_ctrl_up_scrolls() {
        let mut app = make_app();
        app.scroll_offset = 0;
        handle_key(&mut app, make_key(KeyCode::Up, KeyModifiers::CONTROL));
        assert_eq!(app.scroll_offset, 3);
    }

    #[test]
    fn test_ctrl_down_scrolls() {
        let mut app = make_app();
        app.scroll_offset = 10;
        handle_key(&mut app, make_key(KeyCode::Down, KeyModifiers::CONTROL));
        assert_eq!(app.scroll_offset, 7);
    }

    // Input focus tests
    #[test]
    fn test_input_enter_submits() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "hello".to_string();
        let action = handle_key(&mut app, make_key(KeyCode::Enter, KeyModifiers::NONE));
        match action {
            Action::Submit(text) => assert_eq!(text, "hello"),
            _ => panic!("Expected Submit action"),
        }
    }

    #[test]
    fn test_input_enter_empty_does_nothing() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "".to_string();
        let action = handle_key(&mut app, make_key(KeyCode::Enter, KeyModifiers::NONE));
        matches!(action, Action::None);
    }

    #[test]
    fn test_input_enter_command() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "/help".to_string();
        let action = handle_key(&mut app, make_key(KeyCode::Enter, KeyModifiers::NONE));
        match action {
            Action::Command(cmd) => assert_eq!(cmd, "/help"),
            _ => panic!("Expected Command action"),
        }
    }

    #[test]
    fn test_input_enter_while_thinking() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.thinking = true;
        app.input = "test".to_string();
        let action = handle_key(&mut app, make_key(KeyCode::Enter, KeyModifiers::NONE));
        matches!(action, Action::None);
    }

    #[test]
    fn test_input_char_inserts() {
        let mut app = make_app();
        app.focus = Focus::Input;
        handle_key(&mut app, make_key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.input, "a");
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_input_char_shift_inserts() {
        let mut app = make_app();
        app.focus = Focus::Input;
        handle_key(&mut app, make_key(KeyCode::Char('A'), KeyModifiers::SHIFT));
        assert_eq!(app.input, "A");
    }

    #[test]
    fn test_input_backspace() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 3;
        handle_key(&mut app, make_key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.input, "ab");
        assert_eq!(app.cursor, 2);
    }

    #[test]
    fn test_input_backspace_at_start() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 0;
        handle_key(&mut app, make_key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.input, "abc");
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_input_delete() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 1;
        handle_key(&mut app, make_key(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.input, "ac");
    }

    #[test]
    fn test_input_delete_at_end() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 3;
        handle_key(&mut app, make_key(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.input, "abc");
    }

    #[test]
    fn test_input_left_moves_cursor() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 2;
        handle_key(&mut app, make_key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_input_right_moves_cursor() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 1;
        handle_key(&mut app, make_key(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor, 2);
    }

    #[test]
    fn test_input_home() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 2;
        handle_key(&mut app, make_key(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_input_end() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "abc".to_string();
        app.cursor = 0;
        handle_key(&mut app, make_key(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(app.cursor, 3);
    }

    #[test]
    fn test_input_escape_clears() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "some text".to_string();
        app.cursor = 5;
        handle_key(&mut app, make_key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.input.is_empty());
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_input_tab_toggles_mode_empty() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "".to_string();
        app.mode = AgentMode::Build;
        handle_key(&mut app, make_key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mode, AgentMode::Plan);
    }

    #[test]
    fn test_input_tab_with_text_stays_in_input() {
        let mut app = make_app();
        app.focus = Focus::Input;
        app.input = "some text".to_string();
        app.cursor = 9;
        handle_key(&mut app, make_key(KeyCode::Tab, KeyModifiers::NONE));
        // Tab with text no longer switches focus (it tries @file completion)
        assert_eq!(app.focus, Focus::Input);
    }

    // Chat focus tests
    #[test]
    fn test_chat_up_scrolls() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        app.scroll_offset = 0;
        handle_key(&mut app, make_key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 1);
    }

    #[test]
    fn test_chat_down_scrolls() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        app.scroll_offset = 5;
        handle_key(&mut app, make_key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 4);
    }

    #[test]
    fn test_chat_j_scrolls_down() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        app.scroll_offset = 5;
        handle_key(&mut app, make_key(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 4);
    }

    #[test]
    fn test_chat_k_scrolls_up() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        app.scroll_offset = 0;
        handle_key(&mut app, make_key(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 1);
    }

    #[test]
    fn test_chat_tab_to_sidebar() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        handle_key(&mut app, make_key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn test_chat_escape_to_input() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        handle_key(&mut app, make_key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Input);
    }

    #[test]
    fn test_chat_i_to_input() {
        let mut app = make_app();
        app.focus = Focus::Chat;
        handle_key(&mut app, make_key(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Input);
    }

    // Sidebar focus tests
    #[test]
    fn test_sidebar_tab_to_input() {
        let mut app = make_app();
        app.focus = Focus::Sidebar;
        handle_key(&mut app, make_key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Input);
    }

    #[test]
    fn test_sidebar_escape_to_input() {
        let mut app = make_app();
        app.focus = Focus::Sidebar;
        handle_key(&mut app, make_key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Input);
    }

    // Mouse tests
    #[test]
    fn test_mouse_scroll_up() {
        let mut app = make_app();
        app.scroll_offset = 0;
        let action = handle_mouse(&mut app, MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 3);
        matches!(action, Action::None);
    }

    #[test]
    fn test_mouse_scroll_down() {
        let mut app = make_app();
        app.scroll_offset = 10;
        let action = handle_mouse(&mut app, MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll_offset, 7);
        matches!(action, Action::None);
    }

    #[test]
    fn test_scroll_offset_saturating() {
        let mut app = make_app();
        app.scroll_offset = 0;
        // Try to scroll down (should saturate at 0)
        handle_key(&mut app, make_key(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_action_enum_variants() {
        // Test that all Action variants can be created
        let _none = Action::None;
        let _submit = Action::Submit("test".to_string());
        let _exit = Action::Exit;
        let _command = Action::Command("/help".to_string());
    }
}
