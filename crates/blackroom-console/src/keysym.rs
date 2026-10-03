//! Text typed on the browser's soft keyboard, as X11 keysyms: Mutter turns each into the key presses of the laptop's own
//! layout, so the characters arrive right whatever that layout is.

const RETURN: u32 = 0xff0d;
const TAB: u32 = 0xff09;
/// Keysyms above Latin-1 are the Unicode code point plus this.
const UNICODE_BASE: u32 = 0x0100_0000;
pub const MAX_TEXT_CHARS: usize = 256;

pub fn keysym(ch: char) -> Option<u32> {
    match ch {
        '\n' | '\r' => Some(RETURN),
        '\t' => Some(TAB),
        ' '..='~' | '\u{a0}'..='\u{ff}' => Some(u32::from(ch)),
        c if c.is_control() => None,
        c => Some(UNICODE_BASE + u32::from(c)),
    }
}

/// The keysyms of `text`, or why it is refused: too long, or a control character.
pub fn keysyms(text: &str) -> Result<Vec<u32>, &'static str> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err("text too long");
    }
    text.chars()
        .map(|ch| keysym(ch).ok_or("control character in text"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_latin1_and_unicode_map_to_keysyms() {
        assert_eq!(keysym('a'), Some(0x61));
        assert_eq!(keysym('~'), Some(0x7e));
        assert_eq!(keysym('\u{e9}'), Some(0xe9));
        assert_eq!(keysym('\u{20ac}'), Some(0x0100_20ac));
        assert_eq!(keysym('\u{1f600}'), Some(0x0101_f600));
        assert_eq!(keysym('\n'), Some(0xff0d));
        assert_eq!(keysym('\t'), Some(0xff09));
    }

    #[test]
    fn control_characters_and_long_text_are_refused() {
        for ch in ['\0', '\u{7}', '\u{1b}', '\u{7f}', '\u{85}'] {
            assert_eq!(keysym(ch), None, "{ch:?}");
        }
        assert!(keysyms("ok\u{7}").is_err());
        assert!(keysyms(&"x".repeat(MAX_TEXT_CHARS)).is_ok());
        assert!(keysyms(&"x".repeat(MAX_TEXT_CHARS + 1)).is_err());
        assert_eq!(keysyms("h\u{e9}").unwrap(), vec![0x68, 0xe9]);
    }
}
