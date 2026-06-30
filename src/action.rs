use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Focus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    ToggleFocus,
    BrowserUp,
    BrowserDown,
    OpenSelected,
    CollapseOrParent,
    ContentUp(usize),
    ContentDown(usize),
    ContentTop,
    ContentBottom,
}

/// Pure input mapping function.
/// Returns (Option<Action>, next_pending_g)
///
/// Encodes the current key behavior:
/// - `q` and `Ctrl-C` -> Quit (in any focus)
/// - Browser: Tab->ToggleFocus, Down/j->BrowserDown, Up/k->BrowserUp,
///   Enter/l->OpenSelected, h->CollapseOrParent
/// - Content: Tab/h->ToggleFocus, Down/j->ContentDown(1), Up/k->ContentUp(1),
///   PageDown->ContentDown(20), PageUp->ContentUp(20),
///   G->ContentBottom, g-> if pending_g then ContentTop else set next pending_g=true
/// - Browser does NOT honor `gg`
pub fn map_key(focus: Focus, key: KeyEvent, pending_g: bool) -> (Option<Action>, bool) {
    let KeyEvent {
        code, modifiers, ..
    } = key;

    // Handle quit keys first (applies to all focus modes)
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
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('q')), false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_q_in_content() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('q')), false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_ctrl_c_in_browser() {
        let (action, pending) = map_key(Focus::Browser, ctrl_key('c'), false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    #[test]
    fn quit_key_ctrl_c_in_content() {
        let (action, pending) = map_key(Focus::Content, ctrl_key('c'), false);
        assert_eq!(action, Some(Action::Quit));
        assert!(!pending);
    }

    // --- Browser focus key mappings ---

    #[test]
    fn browser_tab_toggles_focus() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Tab), false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn browser_down_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Down), false);
        assert_eq!(action, Some(Action::BrowserDown));
        assert!(!pending);
    }

    #[test]
    fn browser_j_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('j')), false);
        assert_eq!(action, Some(Action::BrowserDown));
        assert!(!pending);
    }

    #[test]
    fn browser_up_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Up), false);
        assert_eq!(action, Some(Action::BrowserUp));
        assert!(!pending);
    }

    #[test]
    fn browser_k_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('k')), false);
        assert_eq!(action, Some(Action::BrowserUp));
        assert!(!pending);
    }

    #[test]
    fn browser_enter_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Enter), false);
        assert_eq!(action, Some(Action::OpenSelected));
        assert!(!pending);
    }

    #[test]
    fn browser_l_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('l')), false);
        assert_eq!(action, Some(Action::OpenSelected));
        assert!(!pending);
    }

    #[test]
    fn browser_h_maps() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('h')), false);
        assert_eq!(action, Some(Action::CollapseOrParent));
        assert!(!pending);
    }

    #[test]
    fn browser_gg_not_honored() {
        // First 'g' should not produce an action, but set pending_g
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('g')), false);
        assert_eq!(action, None);
        // In Browser focus, 'g' is not handled, so pending_g is not set
        assert!(!pending);

        // Even with pending_g true, second 'g' in Browser should not produce ContentTop
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('g')), true);
        assert_eq!(action, None);
        assert!(!pending);
    }

    // --- Content focus key mappings ---

    #[test]
    fn content_tab_toggles_focus() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Tab), false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn content_h_toggles_focus() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('h')), false);
        assert_eq!(action, Some(Action::ToggleFocus));
        assert!(!pending);
    }

    #[test]
    fn content_down_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Down), false);
        assert_eq!(action, Some(Action::ContentDown(1)));
        assert!(!pending);
    }

    #[test]
    fn content_j_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('j')), false);
        assert_eq!(action, Some(Action::ContentDown(1)));
        assert!(!pending);
    }

    #[test]
    fn content_up_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Up), false);
        assert_eq!(action, Some(Action::ContentUp(1)));
        assert!(!pending);
    }

    #[test]
    fn content_k_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('k')), false);
        assert_eq!(action, Some(Action::ContentUp(1)));
        assert!(!pending);
    }

    #[test]
    fn content_pagedown_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::PageDown), false);
        assert_eq!(action, Some(Action::ContentDown(20)));
        assert!(!pending);
    }

    #[test]
    fn content_pageup_maps() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::PageUp), false);
        assert_eq!(action, Some(Action::ContentUp(20)));
        assert!(!pending);
    }

    #[test]
    fn content_g_capital_maps_to_bottom() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('G')), false);
        assert_eq!(action, Some(Action::ContentBottom));
        assert!(!pending);
    }

    #[test]
    fn content_gg_motion() {
        // First 'g' with no pending should set pending_g=true, no action
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('g')), false);
        assert_eq!(action, None);
        assert!(pending);

        // Second 'g' with pending_g=true should produce ContentTop, clear pending
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('g')), true);
        assert_eq!(action, Some(Action::ContentTop));
        assert!(!pending);
    }

    #[test]
    fn content_unknown_key_no_action() {
        let (action, pending) = map_key(Focus::Content, key(KeyCode::Char('x')), false);
        assert_eq!(action, None);
        assert!(!pending);
    }

    #[test]
    fn browser_unknown_key_no_action() {
        let (action, pending) = map_key(Focus::Browser, key(KeyCode::Char('x')), false);
        assert_eq!(action, None);
        assert!(!pending);
    }
}
