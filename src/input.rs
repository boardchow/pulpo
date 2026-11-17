use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// False when `key` isn't a typing key.
pub fn edit(text: &mut String, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if matches!(key.code, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End | KeyCode::Delete)
        || (ctrl && matches!(key.code, KeyCode::Char('a' | 'e')))
    {
        return false;
    }
    let mut at = text.len();
    edit_at(text, &mut at, key)
}

/// Like `edit`, at `at`, a byte index the arrows move.
pub fn edit_at(text: &mut String, at: &mut usize, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    // ⌘ shortcuts (the prefix, ⌘c…) never type their letter.
    if key.modifiers.contains(KeyModifiers::SUPER) {
        return false;
    }
    let line_start = text[..*at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[*at..].find('\n').map_or(text.len(), |i| *at + i);
    match key.code {
        KeyCode::Left => *at -= text[..*at].chars().next_back().map_or(0, char::len_utf8),
        KeyCode::Right => *at += text[*at..].chars().next().map_or(0, char::len_utf8),
        KeyCode::Home => *at = line_start,
        KeyCode::Char('a') if ctrl => *at = line_start,
        KeyCode::End => *at = line_end,
        KeyCode::Char('e') if ctrl => *at = line_end,
        KeyCode::Up | KeyCode::Down => {
            let start = match key.code {
                KeyCode::Up if line_start > 0 => text[..line_start - 1].rfind('\n').map_or(0, |i| i + 1),
                KeyCode::Down if line_end < text.len() => line_end + 1,
                _ => return true,
            };
            let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
            let column = text[line_start..*at].chars().count();
            *at = text[start..end].char_indices().nth(column).map_or(end, |(i, _)| start + i);
        }
        KeyCode::Backspace if ctrl || alt => delete_word(text, at),
        KeyCode::Char('w' | 'h') if ctrl => delete_word(text, at),
        KeyCode::Backspace => {
            if let Some(c) = text[..*at].chars().next_back() {
                *at -= c.len_utf8();
                text.remove(*at);
            }
        }
        KeyCode::Delete if *at < text.len() => {
            text.remove(*at);
        }
        KeyCode::Delete => {}
        KeyCode::Char('u') if ctrl => {
            text.replace_range(line_start..*at, "");
            *at = line_start;
        }
        KeyCode::Char(c) if !ctrl && !alt => {
            text.insert(*at, c);
            *at += c.len_utf8();
        }
        _ => return false,
    }
    true
}

fn delete_word(text: &mut String, at: &mut usize) {
    let kept = text[..*at].trim_end().trim_end_matches(|c: char| !c.is_whitespace()).len();
    text.replace_range(kept..*at, "");
    *at = kept;
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

    #[test]
    fn edits_anywhere_at_a_cursor() {
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let (mut text, mut at) = (String::from("fix it\nand é"), 3);
        edit_at(&mut text, &mut at, key(KeyCode::Char('!')));
        assert_eq!((text.as_str(), at), ("fix! it\nand é", 4));
        edit_at(&mut text, &mut at, key(KeyCode::Down));
        assert_eq!(&text[at..], "é");
        edit_at(&mut text, &mut at, key(KeyCode::End));
        edit_at(&mut text, &mut at, key(KeyCode::Backspace));
        assert_eq!(text, "fix! it\nand ");
        edit_at(&mut text, &mut at, key(KeyCode::Up));
        assert_eq!(&text[at..], " it\nand ");
        edit_at(&mut text, &mut at, KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!((text.as_str(), at), (" it\nand ", 0));
    }
}
