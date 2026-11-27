//! Запись трека одной строкой текста и атомарная запись файла. Общее для `history.txt`, `likes.txt`
//! и `playlists.txt`, чтобы формат трека жил в одном месте.
//!
//! Строка трека: поля через табуляцию: `сервис`, `длительность в секундах`, `id`, `название`,
//! `исполнители` (через символ 0x1F) и необязательное `адрес обложки`. Шестое поле пишется, только если
//! обложка есть: строки без неё остались такими же, как до появления обложек, и старые файлы читаются как есть.
//! Табуляция, перевод строки, обратная косая и 0x1F внутри текста экранируются (`\t`, `\n`, `\r`, `\\`, `\u`). Нечитаемая строка даёт `None`: вызывающий её пропускает,
//! потому что порча одной записи не должна стирать весь файл.

use std::path::{Path, PathBuf};

use crate::{ProviderKind, Track};

/// Разделитель исполнителей внутри одного поля строки.
const ARTIST_SEP: char = '\u{1f}';

/// Трек одной строкой (без перевода строки в конце).
pub(crate) fn encode_track(t: &Track) -> String {
    let artists = t.artists.iter().map(|a| escape(a)).collect::<Vec<_>>().join(&ARTIST_SEP.to_string());
    let line = format!("{}\t{}\t{}\t{}\t{artists}", t.provider.key(), t.duration_secs, escape(&t.id), escape(&t.title));
    match t.cover_url.as_deref().filter(|url| !url.is_empty()) {
        Some(url) => format!("{line}\t{}", escape(url)),
        None => line,
    }
}

/// Трек из строки; `None`, если строка битая (неизвестный сервис, не число, пустой id, больше шести полей).
pub(crate) fn decode_track(line: &str) -> Option<Track> {
    let mut f = line.split('\t');
    let provider = ProviderKind::from_key(f.next()?)?;
    let duration_secs = f.next()?.parse().ok()?;
    let id = unescape(f.next()?);
    let title = unescape(f.next()?);
    let artists_field = f.next()?;
    let cover_url = f.next().map(unescape).filter(|url| !url.is_empty()); // шестого поля у старых строк нет
    if f.next().is_some() || id.is_empty() {
        return None;
    }
    let artists = if artists_field.is_empty() { Vec::new() } else { artists_field.split(ARTIST_SEP).map(unescape).collect() };
    Some(Track { id, provider, title, artists, duration_secs, cover_url })
}

/// Экранирует текст поля: см. описание формата в начале файла.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ARTIST_SEP => out.push_str("\\u"),
            c => out.push(c),
        }
    }
    out
}

/// Обратно к `escape`.
pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('u') => out.push(ARTIST_SEP),
            Some(other) => out.push(other), // `\\` и неизвестное: берём символ как есть
            None => out.push('\\'),
        }
    }
    out
}

/// Запись во временный файл и переименование: сбой посреди записи оставляет старый файл целым.
/// Нужной папки может не быть: создаём.
pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Сохраняет, если путь задан (`None` = только память). Ошибку только печатает: данные в памяти
/// из-за неудачной записи не теряем. `what` это название для сообщения («историю»).
pub(crate) fn save_or_log(path: &Option<PathBuf>, what: &str, text: &str) {
    let Some(path) = path else { return };
    if let Err(e) = write_atomic(path, text) {
        eprintln!("не удалось сохранить {what}: {e}");
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn track(id: &str, provider: ProviderKind) -> Track {
        Track {
            id: id.to_string(),
            provider,
            title: format!("Трек {id}"),
            artists: vec!["А".into(), "Б".into()],
            duration_secs: 200,
            cover_url: None,
        }
    }

    /// Путь к файлу в свежей временной папке (самой папки ещё нет: запись должна её создать).
    pub(crate) fn temp_file(prefix: &str, name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-{prefix}-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("sub").join(format!("{prefix}.txt"))
    }

    pub(crate) fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn special_characters_roundtrip() {
        let t = Track {
            id: "C:\\музыка\\a\tb.mp3".into(),
            provider: ProviderKind::Local,
            title: "Строка\nс переводом\\и\tтабом".into(),
            artists: vec!["Б\u{1f}В".into(), "Г".into()],
            duration_secs: 7,
            cover_url: None,
        };
        let line = encode_track(&t);
        assert!(!line.contains('\n'), "строка трека не должна содержать перевод строки");
        let back = decode_track(&line).unwrap();
        assert_eq!((back.id.as_str(), back.title.as_str()), (t.id.as_str(), t.title.as_str()));
        assert_eq!(back.artists, t.artists);
    }

    #[test]
    fn broken_lines_give_none() {
        let good = encode_track(&track("ok", ProviderKind::Local));
        assert!(decode_track(&good).is_some());
        assert!(decode_track("мусор").is_none());
        assert!(decode_track("local\tне число\tx\ty\t").is_none());
        assert!(decode_track("vk\t1\t\tпустой id\t").is_none());
        assert!(decode_track(&format!("{good}\tобложка\tлишнее поле")).is_none(), "после обложки полей быть не должно");
    }

    #[test]
    fn cover_url_roundtrips_and_is_written_only_when_present() {
        let mut t = track("sc1", ProviderKind::SoundCloud);
        assert_eq!(encode_track(&t).split('\t').count(), 5, "без обложки строка прежняя, из пяти полей");
        t.cover_url = Some("https://i1.sndcdn.com/artworks-x-t500x500.jpg?a=1\tb".into());
        let line = encode_track(&t);
        assert_eq!(line.split('\t').count(), 6);
        assert_eq!(decode_track(&line).unwrap().cover_url, t.cover_url, "адрес с табуляцией должен пережить запись");
    }

    #[test]
    fn line_from_before_covers_still_reads() {
        let old = "soundcloud\t247\t123456\tГруппа крови\tКино";
        let t = decode_track(old).unwrap();
        assert_eq!((t.title.as_str(), t.cover_url), ("Группа крови", None));
        assert_eq!(decode_track(&format!("{old}\t")).unwrap().cover_url, None, "пустое шестое поле это отсутствие обложки");
    }

    #[test]
    fn write_atomic_creates_folder_and_leaves_no_tmp() {
        let path = temp_file("codec", "atomic");
        write_atomic(&path, "первый").unwrap();
        write_atomic(&path, "второй").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "второй");
        assert!(!path.with_extension("tmp").exists());
        cleanup(&path);
    }
}
