use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// False when `key` isn't a typing key.
pub fn edit(text: &mut String, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    // ⌘ shortcuts (the prefix, ⌘c…) never type their letter.
    if key.modifiers.contains(KeyModifiers::SUPER) {
        return false;
    }
    match key.code {
        KeyCode::Backspace if ctrl || alt => delete_word(text),
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char('u') if ctrl => text.clear(),
        KeyCode::Char('w' | 'h') if ctrl => delete_word(text),
        KeyCode::Char(c) if !ctrl && !alt => text.push(c),
        _ => return false,
    }
    true
}

fn delete_word(text: &mut String) {
    let kept = text.trim_end().trim_end_matches(|c: char| !c.is_whitespace()).len();
    text.truncate(kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(start: &str, keys: &[KeyEvent]) -> String {
        let mut text = start.to_string();
        for &key in keys {
            edit(&mut text, key);
        }
        text
    }

    #[test]
    fn edits_like_readline() {
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        let ctrl = |c| key(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(typed("fix the", &[key(KeyCode::Char('m'), KeyModifiers::NONE)]), "fix them");
        assert_eq!(typed("fix the flaky  ", &[ctrl('w')]), "fix the ");
        assert_eq!(typed("fix the", &[key(KeyCode::Backspace, KeyModifiers::ALT)]), "fix ");
        assert_eq!(typed("fix the", &[ctrl('u')]), "");
        let mut text = String::from("x");
        assert!(!edit(&mut text, ctrl('e')));
        assert_eq!(text, "x");
    }
}
