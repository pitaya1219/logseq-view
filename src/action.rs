use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Focus;

/// Handle key mapping when in search input mode (either focus)
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
/// - `q` and `Ctrl-C` -> Quit (except in search-input mode, where `q` is inserted; Ctrl-C still quits)
/// - Browser: Tab->ToggleFocus, Down/j->BrowserDown, Up/k->BrowserUp,
///   Enter/l->OpenSelected, h->CollapseOrParent, G->BrowserBottom, gg->BrowserTop
/// - Content: Tab/h->ToggleFocus, Down/j->ContentDown(1), Up/k->ContentUp(1),
///   PageDown->ContentDown(20), PageUp->ContentUp(20),
///   G->ContentBottom, gg->ContentTop
/// - Search input mode (`search_input_active`): printable chars->SearchInput,
///   Backspace->SearchBackspace, Enter->SearchCommit, Esc->SearchCancel (both focuses)
/// - Browser `/`->SearchStart; n/N with `browser_has_committed_search`->SearchNext/SearchPrev
/// - Content `/`->SearchStart, n->SearchNext, N->SearchPrev (always in normal mode)
pub fn map_key(
    focus: Focus,
    key: KeyEvent,
    pending_g: bool,
    search_input_active: bool,
    browser_has_committed_search: bool,
) -> (Option<Action>, bool) {
    let KeyEvent {
        code, modifiers, ..
    } = key;

    // In search-input mode, route keys to search handling FIRST so that printable
    // characters (including `q`) are inserted into the query. Ctrl-C still quits.
    if search_input_active {
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
        Focus::Browser => {
            // `/` always starts browser name search
            if let KeyCode::Char('/') = code {
                return (Some(Action::SearchStart), false);
            }
            // n/N navigate when there is a committed browser search query
            if browser_has_committed_search {
                match code {
                    KeyCode::Char('n') => return (Some(Action::SearchNext), false),
                    KeyCode::Char('N') => return (Some(Action::SearchPrev), false),
                    _ => {}
                }
            }
            match code {
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
            }
        }
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
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('q')), false, false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_q_in_content() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('q')), false, false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_ctrl_c_in_browser() {
        let (action, pending) = map_key(Focus::Browser, ctrl_key('c'), false, false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_ctrl_c_in_content() {
        let (action, pending) = map_key(Focus::Content, ctrl_key('c'), false, false, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    // --- Browser focus key mappings ---

    #[test]
    fn browser_tab_toggles_focus() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Tab), false, false, false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn browser_down_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Down), false, false, false);
        assert_eq!(action, Some(Action::BrowserDown));
        assert!(!pending);
    }

    #[test]
    fn browser_j_maps() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('j')), false, false, false);
        assert_eq!(action, Some(Action::BrowserDown));
        assert!(!pending);
    }

    #[test]
    fn browser_up_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Up), false, false, false);
        assert_eq!(action, Some(Action::BrowserUp));
        assert!(!pending);
    }

    #[test]
    fn browser_k_maps() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('k')), false, false, false);
        assert_eq!(action, Some(Action::BrowserUp));
        assert!(!pending);
    }

    #[test]
    fn browser_enter_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Enter), false, false, false);
        assert_eq!(action, Some(Action::OpenSelected));
        assert!(!pending);
    }

    #[test]
    fn browser_l_maps() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('l')), false, false, false);
        assert_eq!(action, Some(Action::OpenSelected));
        assert!(!pending);
    }

    #[test]
    fn browser_h_maps() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('h')), false, false, false);
        assert_eq!(action, Some(Action::CollapseOrParent));
        assert!(!pending);
    }

    #[test]
    fn browser_g_sets_pending() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('g')), false, false, false);
        assert_eq!(action, None);
        assert!(pending);
    }

    #[test]
    fn browser_gg_maps_to_browser_top() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('g')), true, false, false);
        assert_eq!(action, Some(Action::BrowserTop));
        assert!(!pending);
    }

    #[test]
    fn browser_g_capital_maps_to_bottom() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('G')), false, false, false);
        assert_eq!(action, Some(Action::BrowserBottom));
        assert!(!pending);
    }

    #[test]
    fn browser_unknown_key_no_action() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('x')), false, false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    // --- Content focus key mappings ---

    #[test]
    fn content_tab_toggles_focus() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Tab), false, false, false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn content_h_toggles_focus() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('h')), false, false, false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn content_down_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Down), false, false, false);
        assert_eq!(action, Some(Action::ContentDown(1)));
        assert!(!pending);
    }

    #[test]
    fn content_j_maps() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('j')), false, false, false);
        assert_eq!(action, Some(Action::ContentDown(1)));
        assert!(!pending);
    }

    #[test]
    fn content_up_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Up), false, false, false);
        assert_eq!(action, Some(Action::ContentUp(1)));
        assert!(!pending);
    }

    #[test]
    fn content_k_maps() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('k')), false, false, false);
        assert_eq!(action, Some(Action::ContentUp(1)));
        assert!(!pending);
    }

    #[test]
    fn content_pagedown_maps() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::PageDown), false, false, false);
        assert_eq!(action, Some(Action::ContentDown(20)));
        assert!(!pending);
    }

    #[test]
    fn content_pageup_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::PageUp), false, false, false);
        assert_eq!(action, Some(Action::ContentUp(20)));
        assert!(!pending);
    }

    #[test]
    fn content_g_capital_maps_to_bottom() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('G')), false, false, false);
        assert_eq!(action, Some(Action::ContentBottom));
        assert!(!pending);
    }

    #[test]
    fn content_gg_motion() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('g')), false, false, false);
        assert_eq!(action, None);
        assert!(pending);

        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('g')), true, false, false);
        assert_eq!(action, Some(Action::ContentTop));
        assert!(!pending);
    }

    #[test]
    fn content_unknown_key_no_action() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('x')), false, false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    // --- Content search key mappings (normal mode) ---

    #[test]
    fn content_slash_starts_search() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('/')), false, false, false);
        assert_eq!(action, Some(Action::SearchStart));
        assert!(!pending);
    }

    #[test]
    fn content_n_search_next() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('n')), false, false, false);
        assert_eq!(action, Some(Action::SearchNext));
        assert!(!pending);
    }

    #[test]
    fn content_shift_n_search_prev() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('N')), false, false, false);
        assert_eq!(action, Some(Action::SearchPrev));
        assert!(!pending);
    }

    #[test]
    fn content_n_in_browser_no_action() {
        // In Browser without a committed search, n should do nothing
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('n')), false, false, false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    // --- Search input mode key mappings (both focuses) ---

    #[test]
    fn search_mode_enter_commits() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Enter), false, true, false);
        assert_eq!(action, Some(Action::SearchCommit));
        assert!(!pending);
    }

    #[test]
    fn search_mode_esc_cancels() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Esc), false, true, false);
        assert_eq!(action, Some(Action::SearchCancel));
        assert!(!pending);
    }

    #[test]
    fn search_mode_backspace() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Backspace), false, true, false);
        assert_eq!(action, Some(Action::SearchBackspace));
        assert!(!pending);
    }

    #[test]
    fn search_mode_char_input() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('a')), false, true, false);
        assert_eq!(action, Some(Action::SearchInput('a')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_space_input() {
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char(' ')), false, true, false);
        assert_eq!(action, Some(Action::SearchInput(' ')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_n_is_input() {
        // In search mode, 'n' should be treated as input, not SearchNext
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('n')), false, true, false);
        assert_eq!(action, Some(Action::SearchInput('n')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_q_is_input_not_quit() {
        // While typing a query, `q` must be inserted, not quit the app.
        let (action, pending) =
            map_key(Focus::Content, key(KeyCode::Char('q')), false, true, false);
        assert_eq!(action, Some(Action::SearchInput('q')));
        assert!(!pending);
    }

    #[test]
    fn search_mode_ctrl_c_still_quits() {
        let (action, pending) = map_key(Focus::Content, ctrl_key('c'), false, true, false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    // --- Browser search key mappings ---

    #[test]
    fn browser_slash_starts_search() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('/')), false, false, false);
        assert_eq!(action, Some(Action::SearchStart));
        assert!(!pending);
    }

    #[test]
    fn browser_search_input_mode_char() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('t')), false, true, false);
        assert_eq!(action, Some(Action::SearchInput('t')));
        assert!(!pending);
    }

    #[test]
    fn browser_search_input_mode_backspace() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Backspace), false, true, false);
        assert_eq!(action, Some(Action::SearchBackspace));
        assert!(!pending);
    }

    #[test]
    fn browser_search_input_mode_enter() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Enter), false, true, false);
        assert_eq!(action, Some(Action::SearchCommit));
        assert!(!pending);
    }

    #[test]
    fn browser_search_input_mode_esc() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Esc), false, true, false);
        assert_eq!(action, Some(Action::SearchCancel));
        assert!(!pending);
    }

    #[test]
    fn browser_search_input_mode_q_is_input() {
        let (action, pending) =
            map_key(Focus::Browser, key(KeyCode::Char('q')), false, true, false);
        assert_eq!(action, Some(Action::SearchInput('q')));
        assert!(!pending);
    }

    #[test]
    fn browser_committed_search_n() {
        let (action, _pending) =
            map_key(Focus::Browser, key(KeyCode::Char('n')), false, false, true);
        assert_eq!(action, Some(Action::SearchNext));
    }

    #[test]
    fn browser_committed_search_n_uppercase() {
        let (action, _pending) =
            map_key(Focus::Browser, key(KeyCode::Char('N')), false, false, true);
        assert_eq!(action, Some(Action::SearchPrev));
    }

    #[test]
    fn browser_no_committed_search_n_no_action() {
        let (action, _pending) =
            map_key(Focus::Browser, key(KeyCode::Char('n')), false, false, false);
        assert_eq!(action, None);
    }

    #[test]
    fn browser_no_committed_search_n_uppercase_no_action() {
        let (action, _pending) =
            map_key(Focus::Browser, key(KeyCode::Char('N')), false, false, false);
        assert_eq!(action, None);
    }
}
