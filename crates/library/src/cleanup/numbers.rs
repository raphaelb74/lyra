//! Rules on numbers and dates: `3/12`, `03`, `Album (Disc 2)`, `13.04.1973`.

use crate::values::{self, Field};

/// `3/12` → (3, Some(12)); `03` and ` 3 ` → (3, None). Zero and anything
/// else is no number.
pub(crate) fn number_parts(value: &str) -> Option<(u32, Option<u32>)> {
    let (number, total) = match value.split_once('/') {
        Some((number, total)) => (number, Some(total)),
        None => (value, None),
    };
    let number = whole(number)?;
    let total = match total {
        Some(total) => Some(whole(total)?),
        None => None,
    };
    Some((number, total))
}

fn whole(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|&n| n > 0)
}

/// `Album (Disc 2)`, `Album [CD2]`, `Album - Disc 2`, `Album CD 2` →
/// (`Album`, 2).
pub(crate) fn disc_in_album(album: &str) -> Option<(String, u32)> {
    let lower = album.to_ascii_lowercase();
    let body = lower.trim_end();
    let (body, closed) = match body.strip_suffix([')', ']']) {
        Some(inner) => (inner, true),
        None => (body, false),
    };
    let digits = body.bytes().rev().take_while(u8::is_ascii_digit).count();
    let number = whole(&body[body.len() - digits..])?;
    let before = body[..body.len() - digits].trim_end();
    let word = ["disc", "disk", "cd"]
        .into_iter()
        .find(|word| before.ends_with(word))?;
    let mut start = before.len() - word.len();
    let mut rest = album[..start].trim_end();
    if closed {
        rest = rest.strip_suffix(['(', '['])?.trim_end();
    } else if !album[..start].ends_with(' ') {
        // `ABCD2` is no disc: the word must stand alone.
        return None;
    }
    rest = rest.strip_suffix('-').map_or(rest, str::trim_end);
    start = rest.len();
    let base = album[..start].trim_end();
    (!base.is_empty()).then(|| (base.to_owned(), number))
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// A date as ISO 8601 (`1973`, `1973-04`, `1973-04-13`), keeping the
/// precision it has. `None` when it already is, or when it cannot be read
/// without guessing (`04/05/1973`: April or May?).
pub(crate) fn iso_date(value: &str) -> Option<String> {
    let value = value.trim();
    if values::is_valid(Field::Date, value) {
        return None;
    }
    let iso = numeric_date(value).or_else(|| named_date(value))?;
    values::is_valid(Field::Date, &iso).then_some(iso)
}

/// `1973-4-13`, `1973/04/13`, `13.04.1973`, `13/04/1973`, `4/13/1973`,
/// and a timestamp's date (`1973-04-13T20:00:00`).
fn numeric_date(value: &str) -> Option<String> {
    let value = value.split_once('T').map_or(value, |(date, _)| date);
    let separator = ['-', '/', '.'].into_iter().find(|&s| value.contains(s))?;
    let parts: Vec<&str> = value.split(separator).collect();
    let numbers = parts
        .iter()
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
        })
        .collect::<Option<Vec<u32>>>()?;
    let (year, month, day) = match (parts.as_slice(), numbers.as_slice()) {
        ([y, ..], &[year, month]) if y.len() == 4 => (year, month, None),
        ([y, ..], &[year, month, day]) if y.len() == 4 => (year, month, Some(day)),
        ([.., y], &[first, second, year]) if y.len() == 4 => {
            let (day, month) = if separator == '.' || first > 12 {
                (first, second)
            } else if second > 12 {
                (second, first)
            } else {
                return None;
            };
            (year, month, Some(day))
        }
        _ => return None,
    };
    format_date(year, month, day)
}

/// `April 1973`, `Apr 1973`, `13 April 1973`, `April 13, 1973`.
fn named_date(value: &str) -> Option<String> {
    let words: Vec<String> = value
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let month_of = |word: &str| {
        let word = word.trim_end_matches('.');
        MONTHS
            .iter()
            .position(|month| word.len() >= 3 && month.starts_with(word))
            .and_then(|index| u32::try_from(index + 1).ok())
    };
    let number = |word: &str| word.parse::<u32>().ok();
    let (year, month, day) = match words.as_slice() {
        [month, year] => (number(year)?, month_of(month)?, None),
        [day, month, year] => match month_of(month) {
            Some(month) => (number(year)?, month, Some(number(day)?)),
            None => (number(year)?, month_of(day)?, Some(number(month)?)),
        },
        _ => return None,
    };
    (year >= 1000).then_some(())?;
    format_date(year, month, day)
}

fn format_date(year: u32, month: u32, day: Option<u32>) -> Option<String> {
    if !(1000..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    match day {
        None => Some(format!("{year:04}-{month:02}")),
        Some(day) if (1..=days_in(year, month)).contains(&day) => {
            Some(format!("{year:04}-{month:02}-{day:02}"))
        }
        Some(_) => None,
    }
}

fn days_in(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::{disc_in_album, iso_date, number_parts};

    #[test]
    fn numbers_and_totals_are_read() {
        assert_eq!(number_parts("3/12"), Some((3, Some(12))));
        assert_eq!(number_parts(" 03 "), Some((3, None)));
        assert_eq!(number_parts("3 / 12"), Some((3, Some(12))));
        for bad in ["0", "", "three", "3/", "3/x", "-1", "3.5"] {
            assert_eq!(number_parts(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn disc_numbers_leave_album_titles() {
        for (album, base, disc) in [
            ("Mellon Collie (Disc 2)", "Mellon Collie", 2),
            ("Mellon Collie [CD2]", "Mellon Collie", 2),
            ("Mellon Collie - Disc 1", "Mellon Collie", 1),
            ("Mellon Collie CD 2", "Mellon Collie", 2),
            ("Mellon Collie (disk 02)", "Mellon Collie", 2),
        ] {
            assert_eq!(
                disc_in_album(album),
                Some((base.to_owned(), disc)),
                "{album}"
            );
        }
        for album in [
            "ABCD2",
            "Discovery",
            "1999",
            "CD 2",
            "Mellon Collie",
            "Hits (Disco 2)",
        ] {
            assert_eq!(disc_in_album(album), None, "{album}");
        }
    }

    #[test]
    fn dates_become_iso_without_guessing() {
        for (date, iso) in [
            ("13.04.1973", "1973-04-13"),
            ("1973/04/13", "1973-04-13"),
            ("1973-4-3", "1973-04-03"),
            ("13/04/1973", "1973-04-13"),
            ("4/13/1973", "1973-04-13"),
            ("1973-04-13T20:00:00", "1973-04-13"),
            ("April 1973", "1973-04"),
            ("Apr. 1973", "1973-04"),
            ("13 April 1973", "1973-04-13"),
            ("April 13, 1973", "1973-04-13"),
            ("1973/04", "1973-04"),
        ] {
            assert_eq!(iso_date(date).as_deref(), Some(iso), "{date}");
        }
        for date in [
            "1973",
            "1973-04-13",
            "04/05/1973",
            "31.02.1973",
            "73",
            "Spring 1973",
            "May",
        ] {
            assert_eq!(iso_date(date), None, "{date}");
        }
    }
}
