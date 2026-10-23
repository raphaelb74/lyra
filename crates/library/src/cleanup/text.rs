//! Rules on one text value: hygiene, encoding repair, capitals, featured
//! artists. Each returns the corrected value, or `None` to leave it alone.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::key::{is_invisible, key};

/// Spaces trimmed and collapsed, invisible characters removed, NFC.
pub(crate) fn hygiene(value: &str) -> Option<String> {
    let mut tidy = String::with_capacity(value.len());
    let mut space = false;
    for c in value.chars().filter(|&c| !is_invisible(c)) {
        if c.is_whitespace() {
            space = !tidy.is_empty();
        } else {
            if space {
                tidy.push(' ');
                space = false;
            }
            tidy.push(c);
        }
    }
    let tidy: String = tidy.nfc().collect();
    (tidy != value && !tidy.is_empty()).then_some(tidy)
}

/// UTF-8 read as Latin-1 or Windows-1252 (`BeyoncÃ©`), read back as UTF-8.
/// Proposed only when every character maps back to one byte and the bytes
/// are valid UTF-8, which correct text almost never is.
pub(crate) fn repair_encoding(value: &str) -> Option<String> {
    if value.is_ascii() {
        return None;
    }
    let bytes = value
        .chars()
        .map(|c| match u32::from(c) {
            code @ 0..=0xFF => u8::try_from(code).ok(),
            _ => windows_1252_byte(c),
        })
        .collect::<Option<Vec<u8>>>()?;
    let fixed = String::from_utf8(bytes).ok()?;
    (fixed != value).then_some(fixed)
}

/// The Windows-1252 byte of a character it places in 0x80–0x9F.
fn windows_1252_byte(c: char) -> Option<u8> {
    const HIGH: [(char, u8); 27] = [
        ('€', 0x80),
        ('‚', 0x82),
        ('ƒ', 0x83),
        ('„', 0x84),
        ('…', 0x85),
        ('†', 0x86),
        ('‡', 0x87),
        ('ˆ', 0x88),
        ('‰', 0x89),
        ('Š', 0x8A),
        ('‹', 0x8B),
        ('Œ', 0x8C),
        ('Ž', 0x8E),
        ('\u{2018}', 0x91),
        ('\u{2019}', 0x92),
        ('\u{201C}', 0x93),
        ('\u{201D}', 0x94),
        ('•', 0x95),
        ('–', 0x96),
        ('—', 0x97),
        ('˜', 0x98),
        ('™', 0x99),
        ('š', 0x9A),
        ('›', 0x9B),
        ('œ', 0x9C),
        ('ž', 0x9E),
        ('Ÿ', 0x9F),
    ];
    HIGH.iter()
        .find(|(high, _)| *high == c)
        .map(|&(_, byte)| byte)
}

/// Words that stay lower case inside a title.
const SMALL_WORDS: [&str; 17] = [
    "a", "an", "and", "as", "at", "but", "by", "for", "from", "in", "nor", "of", "on", "or", "the",
    "to", "with",
];

/// Roman numerals kept in capitals (`Rocky II`).
const NUMERALS: [&str; 19] = [
    "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv",
    "xvi", "xvii", "xviii", "xix", "xx",
];

/// Text entirely in capitals, or with `lower_too` entirely in lower case,
/// as title case. Text with fewer than two words is left alone: a single
/// word in one case is usually meant (`ABBA`, `MGMT`, `deadmau5`), and so is
/// mixed case. Names pass `lower_too = false`: `k.d. lang` is meant too.
pub(crate) fn capitals(value: &str, lower_too: bool) -> Option<String> {
    let words: Vec<&str> = value.split(' ').collect();
    let worded = words
        .iter()
        .filter(|word| word.chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .count();
    if worded < 2 {
        return None;
    }
    let mut cased = value
        .chars()
        .filter(|c| c.is_uppercase() || c.is_lowercase());
    let upper = cased.clone().all(char::is_uppercase);
    let lower = lower_too && cased.all(char::is_lowercase);
    if !upper && !lower {
        return None;
    }
    let last = words.len() - 1;
    let titled: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let lower = word.to_lowercase();
            if index > 0 && index < last && SMALL_WORDS.contains(&lower.as_str()) {
                lower
            } else if NUMERALS.contains(&lower.as_str()) {
                word.to_uppercase()
            } else {
                capitalize(&lower)
            }
        })
        .collect();
    let titled = titled.join(" ");
    (titled != value).then_some(titled)
}

/// The first letter in capitals, after any opening punctuation (`(live)`).
fn capitalize(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut done = false;
    for c in word.chars() {
        if !done && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// How guests are named: `feat.`, `ft.` and `featuring`, in any case.
/// Never `&`, `,`, `and` or `with`, which also join single names.
const MARKERS: [&str; 5] = ["featuring", "feat.", "feat", "ft.", "ft"];

/// A guest marker in `text`: where it starts, where the guest begins, and
/// whether it was in brackets (`(feat. X)`). Markers are ASCII, so byte
/// positions in the ASCII-lowered copy are positions in `text`.
fn marker(text: &str) -> Option<(usize, usize, Option<char>)> {
    let lower = text.to_ascii_lowercase();
    let mut best: Option<(usize, usize, Option<char>)> = None;
    for marker in MARKERS {
        for (at, _) in lower.match_indices(marker) {
            let before = lower[..at].chars().next_back();
            let after = lower[at + marker.len()..].chars().next();
            let bracket = match before {
                Some(open @ ('(' | '[')) => Some(open),
                Some(' ') => None,
                _ => continue,
            };
            if after != Some(' ') || at == 0 {
                continue;
            }
            let start = if bracket.is_some() { at - 1 } else { at };
            if best.is_none_or(|(known, ..)| start < known) {
                best = Some((start, at + marker.len() + 1, bracket));
            }
        }
    }
    best
}

/// `Song (feat. X)`, `Song [ft. X]`, `Song feat. X` → (`Song`, `X`).
pub(crate) fn guest_in_title(title: &str) -> Option<(String, String)> {
    let (start, guest_at, bracket) = marker(title)?;
    let rest = &title[guest_at..];
    let guest = match bracket {
        Some(open) => {
            let close = if open == '(' { ')' } else { ']' };
            rest.strip_suffix(close)?
        }
        None => rest,
    }
    .trim();
    let song = title[..start].trim_end();
    (!guest.is_empty() && !song.is_empty()).then(|| (song.to_owned(), guest.to_owned()))
}

/// `A ft. B`, `A featuring B`, `A Feat B` → `A feat. B`.
pub(crate) fn guest_marker(artist: &str) -> Option<String> {
    let (start, guest_at, bracket) = marker(artist)?;
    if bracket.is_some() {
        return None;
    }
    let (main, guest) = (artist[..start].trim_end(), artist[guest_at..].trim());
    if main.is_empty() || guest.is_empty() {
        return None;
    }
    let fixed = format!("{main} feat. {guest}");
    (fixed != artist).then_some(fixed)
}

/// The main artist of a credit: `A feat. B` → `A`.
pub(crate) fn main_artist(artist: &str) -> &str {
    match marker(artist) {
        Some((start, _, None)) => artist[..start].trim_end(),
        _ => artist,
    }
}

/// `artist` with `guest` credited, unless already credited.
pub(crate) fn with_guest(artist: &str, guest: &str) -> String {
    if key(artist).contains(&key(guest)) {
        artist.to_owned()
    } else {
        format!("{artist} feat. {guest}")
    }
}

/// A key that also ignores accents and a leading `The`: `The Beatles`,
/// `Beatles` and `beatles` match, as do `Beyoncé` and `Beyonce`.
pub(crate) fn loose_key(name: &str) -> String {
    let key = key(name);
    let key = key
        .strip_prefix("the ")
        .filter(|rest| !rest.is_empty() && *rest != "the")
        .unwrap_or(&key);
    key.nfd().filter(|&c| !is_combining_mark(c)).collect()
}

/// `The Beatles` → `Beatles, The`, for sorting.
pub(crate) fn sort_name(name: &str) -> Option<String> {
    let rest = name
        .strip_prefix("The ")
        .or_else(|| name.strip_prefix("the "))?;
    let rest = rest.trim();
    (!rest.is_empty() && !rest.eq_ignore_ascii_case("the")).then(|| format!("{rest}, The"))
}

#[cfg(test)]
mod tests {
    use super::{
        capitals, guest_in_title, guest_marker, hygiene, loose_key, main_artist, repair_encoding,
        sort_name, with_guest,
    };

    /// `(input, expected)`; `None` means the rule leaves it alone.
    fn check(rule: fn(&str) -> Option<String>, cases: &[(&str, Option<&str>)]) {
        for (input, expected) in cases {
            assert_eq!(rule(input).as_deref(), *expected, "{input:?}");
        }
    }

    #[test]
    fn hygiene_tidies_spaces_invisibles_and_forms() {
        check(
            hygiene,
            &[
                ("  Aladdin  Sane ", Some("Aladdin Sane")),
                ("Tab\tand\nline", Some("Tab and line")),
                ("Zero\u{200B}width", Some("Zerowidth")),
                ("\u{FEFF}Bom", Some("Bom")),
                ("Beyonce\u{301}", Some("Beyonc\u{e9}")),
                // Already tidy, or nothing left.
                ("Aladdin Sane", None),
                ("AC/DC", None),
                ("\u{200B}", None),
            ],
        );
    }

    #[test]
    fn encoding_repair_reads_mojibake_back() {
        check(
            repair_encoding,
            &[
                ("Beyonc\u{c3}\u{a9}", Some("Beyonc\u{e9}")),
                ("Bj\u{c3}\u{b6}rk", Some("Bj\u{f6}rk")),
                // A curly apostrophe through Windows-1252.
                ("Don\u{e2}\u{20ac}\u{2122}t", Some("Don\u{2019}t")),
                // Correct text stays.
                ("Beyonc\u{e9}", None),
                ("Sigur R\u{f3}s", None),
                ("坂本龍一", None),
                ("Plain", None),
            ],
        );
    }

    #[test]
    fn capitals_only_for_text_in_one_case() {
        check(
            |value| capitals(value, true),
            &[
                (
                    "THE DARK SIDE OF THE MOON",
                    Some("The Dark Side of the Moon"),
                ),
                (
                    "the dark side of the moon",
                    Some("The Dark Side of the Moon"),
                ),
                ("ROCKY II SOUNDTRACK", Some("Rocky II Soundtrack")),
                ("DON'T STOP (LIVE)", Some("Don't Stop (Live)")),
                ("SIGUR RÓS", Some("Sigur Rós")),
                // Single words in one case are often meant.
                ("ABBA", None),
                ("MGMT", None),
                ("deadmau5", None),
                ("AC/DC", None),
                // Mixed case is left as written.
                ("The xx", None),
                ("Dark Side Of The Moon", None),
            ],
        );
        // Names: capitals only; lower case is a style.
        check(
            |value| capitals(value, false),
            &[
                ("THE BEATLES", Some("The Beatles")),
                ("k.d. lang", None),
                ("of montreal", None),
            ],
        );
    }

    #[test]
    fn guests_move_from_titles_to_artists() {
        let cases = [
            (
                "Teardrop (feat. Elizabeth Fraser)",
                Some(("Teardrop", "Elizabeth Fraser")),
            ),
            ("Song [ft. X]", Some(("Song", "X"))),
            ("Song Featuring X & Y", Some(("Song", "X & Y"))),
            ("Song feat X", Some(("Song", "X"))),
            // Not a marker.
            ("Lift (Software Update)", None),
            ("Craft Work", None),
            ("Feat of Strength", None),
            ("Left (ft)", None),
        ];
        for (title, expected) in cases {
            let found = guest_in_title(title);
            assert_eq!(
                found.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                expected,
                "{title}"
            );
        }
        check(
            guest_marker,
            &[
                ("A ft. B", Some("A feat. B")),
                ("A Featuring B", Some("A feat. B")),
                ("A Feat B", Some("A feat. B")),
                ("A feat. B", None),
                ("Simon & Garfunkel", None),
                ("Crosby, Stills & Nash", None),
            ],
        );
        assert_eq!(
            with_guest("Massive Attack", "Elizabeth Fraser"),
            "Massive Attack feat. Elizabeth Fraser"
        );
        assert_eq!(with_guest("A feat. X", "x"), "A feat. X");
        assert_eq!(main_artist("Pink Floyd feat. David Gilmour"), "Pink Floyd");
        assert_eq!(main_artist("Simon & Garfunkel"), "Simon & Garfunkel");
    }

    #[test]
    fn loose_keys_and_sort_names() {
        assert_eq!(loose_key("The Beatles"), loose_key("beatles"));
        assert_eq!(loose_key("Beyonc\u{e9}"), loose_key("BEYONCE"));
        assert_ne!(loose_key("The The"), loose_key("The"));
        assert_ne!(loose_key("Air"), loose_key("Aïr Supply"));
        check(
            sort_name,
            &[
                ("The Beatles", Some("Beatles, The")),
                ("The The", None),
                ("Theatre of Tragedy", None),
                ("Beatles", None),
            ],
        );
    }
}
