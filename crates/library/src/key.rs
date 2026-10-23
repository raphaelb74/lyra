//! Matching keys: when two spellings name the same artist or album.
//!
//! Shown names stay as tagged; only comparisons use the key.

use unicode_normalization::UnicodeNormalization;

/// `text` lower-cased in full Unicode, in NFC, with runs of whitespace as one
/// space and invisible characters removed. `Beyonce` + a combining accent,
/// `BEYONCÉ` and `beyoncé` share a key; punctuation is kept, so `AC/DC`
/// stays `ac/dc`.
pub fn key(text: &str) -> String {
    let mut lowered = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if is_invisible(c) {
            continue;
        }
        if c.is_whitespace() {
            space = !lowered.is_empty();
            continue;
        }
        if space {
            lowered.push(' ');
            space = false;
        }
        lowered.extend(c.to_lowercase());
    }
    lowered.nfc().collect()
}

/// Characters that change nothing a reader sees: controls, zero-width
/// characters, direction marks, the byte-order mark and soft hyphens.
pub(crate) fn is_invisible(c: char) -> bool {
    (c.is_control() && !c.is_whitespace())
        || matches!(
            c,
            '\u{00AD}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn spellings_of_one_name_share_a_key() {
        let nfc = key("Beyonc\u{e9}");
        assert_eq!(nfc, "beyonc\u{e9}");
        assert_eq!(key("Beyonce\u{301}"), nfc, "decomposed accent");
        assert_eq!(key("BEYONC\u{c9}"), nfc, "capitals beyond ASCII");
        assert_eq!(key("  Beyonc\u{e9} \u{200B}"), nfc, "spaces and zero-width");
        assert_eq!(key("\u{FEFF}Beyonc\u{e9}"), nfc, "byte-order mark");
        assert_eq!(key("Simon  &\tGarfunkel"), "simon & garfunkel");
    }

    #[test]
    fn different_names_keep_different_keys() {
        assert_eq!(key("AC/DC"), "ac/dc");
        assert_ne!(key("Beyonc\u{e9}"), key("Beyonce"), "accents count");
        assert_ne!(key("The Beatles"), key("Beatles"));
        assert_eq!(key("坂本龍一"), "坂本龍一");
        assert_eq!(key("שלום"), "שלום");
        assert_eq!(key(""), "");
    }
}
