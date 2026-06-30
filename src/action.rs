use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Focus;

/// Handle key mapping when in search input mode (Content focus only)
fn map_key_search(code: KeyCode, modifiers: KeyModifiers) -> (Option<Action>, bool) {
    // Ctrl-C still quits even while typing a search query.
    if let KeyCode::Char('c') = code {
        if modifiers.contains(KeyModifiers::CONTROL) {
            return (Some(Action::Quit), false);
        }
    }
    match code {
        KeyCode::Enter => (Some(Action::SearchCommit), false),
        KeyCode::Esc => (Some(Action::SearchCancel), false),
        KeyCode::Backspace => (Some(Action::SearchBackspace), false),
        KeyCode::Char(c) => {
            // Only accept printable ASCII characters (not control chars)
            if c.is_ascii() && !c.is_ascii_control() {
                (Some(Action::SearchInput(c)), false)
            } else {
                (None, false)
            }
        }
        _ => (None, false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    ToggleFocus,
    BrowserUp,
    BrowserDown,
    BrowserTop,
    BrowserBottom,
    OpenSelected,
    CollapseOrParent,
    ContentUp(usize),
    ContentDown(usize),
    ContentTop,
    ContentBottom,
    // Search actions
    SearchStart,
    SearchInput(char),
    SearchBackspace,
    SearchCommit,
    SearchCancel,
    SearchNext,
    SearchPrev,
}

/// Pure input mapping function.
/// Returns (Option<Action>, next_pending_g)
///
/// Encodes the current key behavior:
/// - `q` and `Ctrl-C` -> Quit (except in Content search-input mode, where `q` is
///   inserted into the query; `Ctrl-C` still quits)
/// - Browser: Tab->ToggleFocus, Down/j->BrowserDown, Up/k->BrowserUp,
///   Enter/l->OpenSelected, h->CollapseOrParent,
///   G->BrowserBottom, g-> if pending_g then BrowserTop else set next pending_g=true
/// - Content: Tab/h->ToggleFocus, Down/j->ContentDown(1), Up/k->ContentUp(1),
///   PageDown->ContentDown(20), PageUp->ContentUp(20),
///   G->ContentBottom, g-> if pending_g then ContentTop else set next pending_g=true
/// - Content search mode (when search_active=true):
///   Printable chars -> SearchInput(char), Backspace -> SearchBackspace,
///   Enter -> SearchCommit, Esc -> SearchCancel
/// - Content normal mode: / -> SearchStart, n -> SearchNext, N -> SearchPrev
pub fn map_key(
    focus: Focus,
    key: KeyEvent,
    pending_g: bool,
    search_active: bool,
) -> (Option<Action>, bool) {
    let KeyEvent {
        code, modifiers, ..
    } = key;

    // In Content search-input mode, route keys to search handling FIRST, so that
    // printable characters (including `q`) are inserted into the query instead of
    // triggering the global quit. (Ctrl-C still quits, handled inside map_key_search.)
    if focus == Focus::Content && search_active {
        return map_key_search(code, modifiers);
    }

    // Handle quit keys (applies to all non-search-input modes)
    if let KeyCode::Char('q') = code {
        return (Some(Action::Quit), false);
    }
    if let KeyCode::Char('c') = code {
        if modifiers.contains(KeyModifiers::CONTROL) {
            return (Some(Action::Quit), false);
        }
    }

    let action = match focus {
        Focus::Browser => match code {
            KeyCode::Tab => Some(Action::ToggleFocus),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::BrowserDown),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::BrowserUp),
            KeyCode::Enter | KeyCode::Char('l') => Some(Action::OpenSelected),
            KeyCode::Char('h') => Some(Action::CollapseOrParent),
            KeyCode::Char('G') => Some(Action::BrowserBottom),
            KeyCode::Char('g') => {
                if pending_g {
                    Some(Action::BrowserTop)
                } else {
                    return (None, true);
                }
            }
            _ => None,
        },
        Focus::Content => match code {
            KeyCode::Tab | KeyCode::Char('h') => Some(Action::ToggleFocus),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::ContentDown(1)),
            KeyCode::Up | KeyCode::Char('k') => Some(Action::ContentUp(1)),
            KeyCode::PageDown => Some(Action::ContentDown(20)),
            KeyCode::PageUp => Some(Action::ContentUp(20)),
            KeyCode::Char('G') => Some(Action::ContentBottom),
            KeyCode::Char('g') => {
                if pending_g {
                    Some(Action::ContentTop)
                } else {
                    // No action, but set next pending_g to true
                    return (None, true);
                }
            }
            KeyCode::Char('/') => Some(Action::SearchStart),
            KeyCode::Char('n') => Some(Action::SearchNext),
            KeyCode::Char('N') => Some(Action::SearchPrev),
            _ => None,
        },
    };

    (action, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::empty(),
        }
    }

    fn ctrl_key(code: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(code),
            modifiers: KeyModifiers::CONTROL,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::empty(),
        }
    }

    // --- Quit keys work in any focus ---

    #[test]
    fn quit_key_q_in_browser() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('q')), false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_q_in_content() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('q')), false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_ctrl_c_in_browser() {
        let (action, pending) = map_key(Focus::Browser, ctrl_key('c'), false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_ctrl_c_in_content() {
        let (action, pending) = map_key(Focus::Content, ctrl_key('c'), false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    // --- Browser focus key mappings ---

    #[test]
    fn browser_tab_toggles_focus() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Tab), false, false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn browser_down_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Down), false, false);
        assert_eq!(action, Some(Action::BrowserDown));
        assert!(!pending);
    }

    #[test]
    fn browser_j_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('j')), false, false);
        assert_eq!(action, Some(Action::BrowserDown));
        assert!(!pending);
    }

    #[test]
    fn browser_up_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Up), false, false);
        assert_eq!(action, Some(Action::BrowserUp));
        assert!(!pending);
    }

    #[test]
    fn browser_k_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('k')), false, false);
        assert_eq!(action, Some(Action::BrowserUp));
        assert!(!pending);
    }

    #[test]
    fn browser_enter_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Enter), false, false);
        assert_eq!(action, Some(Action::OpenSelected));
        assert!(!pending);
    }

    #[test]
    fn browser_l_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('l')), false, false);
        assert_eq!(action, Some(Action::OpenSelected));
        assert!(!pending);
    }

    #[test]
    fn browser_h_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('h')), false, false);
        assert_eq!(action, Some(Action::CollapseOrParent));
        assert!(!pending);
    }

    #[test]
    fn browser_g_sets_pending() {
        // First 'g' with no pending should set pending_g=true, no action
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('g')), false, false);
        assert_eq!(action, None);
        assert!(pending);
    }

    #[test]
    fn browser_gg_maps_to_browser_top() {
        // Second 'g' with pending_g=true should produce BrowserTop, clear pending
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('g')), true, false);
        assert_eq!(action, Some(Action::BrowserTop));
        assert!(!pending);
    }

    #[test]
    fn browser_g_capital_maps_to_bottom() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('G')), false, false);
        assert_eq!(action, Some(Action::BrowserBottom));
        assert!(!pending);
    }

    // --- Content focus key mappings ---

    #[test]
    fn content_tab_toggles_focus() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Tab), false, false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn content_h_toggles_focus() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('h')), false, false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn content_down_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Down), false, false);
        assert_eq!(action, Some(Action::ContentDown(1)));
        assert!(!pending);
    }

    #[test]
    fn content_j_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('j')), false, false);
        assert_eq!(action, Some(Action::ContentDown(1)));
        assert!(!pending);
    }

    #[test]
    fn content_up_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Up), false, false);
        assert_eq!(action, Some(Action::ContentUp(1)));
        assert!(!pending);
    }

    #[test]
    fn content_k_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('k')), false, false);
        assert_eq!(action, Some(Action::ContentUp(1)));
        assert!(!pending);
    }

    #[test]
    fn content_pagedown_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::PageDown), false, false);
        assert_eq!(action, Some(Action::ContentDown(20)));
        assert!(!pending);
    }

    #[test]
    fn content_pageup_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::PageUp), false, false);
        assert_eq!(action, Some(Action::ContentUp(20)));
        assert!(!pending);
    }

    #[test]
    fn content_g_capital_maps_to_bottom() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('G')), false, false);
        assert_eq!(action, Some(Action::ContentBottom));
        assert!(!pending);
    }

    #[test]
    fn content_gg_motion() {
        // First 'g' with no pending should set pending_g=true, no action
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('g')), false, false);
        assert_eq!(action, None);
        assert!(pending);

        // Second 'g' with pending_g=true should produce ContentTop, clear pending
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('g')), true, false);
        assert_eq!(action, Some(Action::ContentTop));
        assert!(!pending);
    }

    #[test]
    fn content_unknown_key_no_action() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('x')), false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    #[test]
    fn browser_unknown_key_no_action() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('x')), false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    // --- Content search key mappings (normal mode) ---

    #[test]
    fn content_slash_starts_search() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('/')), false, false);
        assert_eq!(action, Some(Action::SearchStart));
        assert!(!pending);
    }

    #[test]
    fn content_n_search_next() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('n')), false, false);
        assert_eq!(action, Some(Action::SearchNext));
        assert!(!pending);
    }

    #[test]
    fn content_shift_n_search_prev() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('N')), false, false);
        assert_eq!(action, Some(Action::SearchPrev));
        assert!(!pending);
    }

    #[test]
    fn content_slash_in_browser_no_action() {
        // / should not work in Browser focus
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('/')), false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    #[test]
    fn content_n_in_browser_no_action() {
        // n should not work in Browser focus
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('n')), false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    // --- Search input mode key mappings ---

    #[test]
    fn search_mode_enter_commits() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Enter), false, true);
        assert_eq!(action, Some(Action::SearchCommit));
        assert!(!pending);
    }

    #[test]
    fn search_mode_esc_cancels() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Esc), false, true);
        assert_eq!(action, Some(Action::SearchCancel));
        assert!(!pending);
    }

    #[test]
    fn search_mode_backspace() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Backspace), false, true);
        assert_eq!(action, Some(Action::SearchBackspace));
        assert!(!pending);
    }

    #[test]
    fn search_mode_char_input() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('a')), false, true);
        assert_eq!(action, Some(Action::SearchInput('a')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_space_input() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char(' ')), false, true);
        assert_eq!(action, Some(Action::SearchInput(' ')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_n_is_input() {
        // In search mode, 'n' should be treated as input, not as SearchNext
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('n')), false, true);
        assert_eq!(action, Some(Action::SearchInput('n')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_in_browser_no_special_handling() {
        // Search mode should only affect Content focus
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('a')), false, true);
        assert_eq!(action, None);
        assert!(!pending);
    }

    #[test]
    fn search_mode_q_is_input_not_quit() {
        // While typing a query, `q` must be inserted, not quit the app.
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('q')), false, true);
        assert_eq!(action, Some(Action::SearchInput('q')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_ctrl_c_still_quits() {
        // Ctrl-C remains an escape hatch even in search-input mode.
        let (action, pending) = map_key(Focus::Content, ctrl_key('c'), false, true);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }
}
