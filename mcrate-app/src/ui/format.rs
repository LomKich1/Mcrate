//! Текстовое оформление данных треков для экранов.

use mcrate_core::Track;

pub(crate) fn fmt_time(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub(crate) fn artists_text(track: &Track) -> String {
    if track.artists.is_empty() { "Неизвестный исполнитель".to_string() } else { track.artists.join(", ") }
}

/// «трек», «трека», «треков» по правилам русского языка.
pub(crate) fn tracks_word(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (_, 11..=14) => "треков",
        (1, _) => "трек",
        (2..=4, _) => "трека",
        _ => "треков",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_word_follows_russian_plural_rules() {
        for (n, word) in [
            (0, "треков"),
            (1, "трек"),
            (2, "трека"),
            (4, "трека"),
            (5, "треков"),
            (11, "треков"),
            (12, "треков"),
            (14, "треков"),
            (21, "трек"),
            (22, "трека"),
            (111, "треков"),
            (101, "трек"),
        ] {
            assert_eq!(tracks_word(n), word, "n = {n}");
        }
    }

    #[test]
    fn fmt_time_pads_seconds() {
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(65), "1:05");
        assert_eq!(fmt_time(3599), "59:59");
    }
}
