//! История прослушивания: последние сыгранные треки, самый свежий первым. Хранится на диске и
//! переживает перезапуск приложения.
//!
//! Файл `history.txt`: одна строка = один трек, поля через табуляцию:
//! `сервис`, `длительность в секундах`, `id`, `название`, `исполнители` (через символ 0x1F).
//! Табуляция, перевод строки, обратная косая и 0x1F внутри текста экранируются (`\t`, `\n`, `\r`, `\\`, `\u`).
//! Нечитаемые строки пропускаются: порча одной записи не должна стирать всю историю.

use std::path::PathBuf;

use crate::{ProviderKind, Track};

/// Сколько треков помним в истории.
pub const HISTORY_MAX: usize = 700;

/// Разделитель исполнителей внутри одного поля файла.
const ARTIST_SEP: char = '\u{1f}';

/// История прослушивания с сохранением в файл.
#[derive(Debug)]
pub struct History {
    items: Vec<Track>,
    /// Куда сохранять; `None` = сохранение выключено (нет папки приложения, или тест).
    path: Option<PathBuf>,
}

impl History {
    /// Открывает историю из файла `path` (нет файла = пустая история). `None` = только в памяти.
    pub fn open(path: Option<PathBuf>) -> Self {
        let items = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|text| parse(&text)).unwrap_or_default();
        Self { items, path }
    }

    /// Треки, самый свежий первым.
    pub fn items(&self) -> &[Track] {
        &self.items
    }

    /// Поднять трек наверх (убрав прежнюю запись о нём), обрезать до `HISTORY_MAX` и сохранить.
    /// Если сохранить не вышло, только сообщаем в консоль: историю в памяти из-за этого не теряем.
    pub fn push(&mut self, track: Track) {
        push_front(&mut self.items, track);
        if let Err(e) = self.save() {
            eprintln!("не удалось сохранить историю: {e}");
        }
    }

    /// Запись во временный файл и переименование: сбой посреди записи оставляет старый файл целым.
    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serialize(&self.items))?;
        std::fs::rename(&tmp, path)
    }
}

/// Трек наверх списка без повторов (трек определяют сервис и id), список не длиннее `HISTORY_MAX`.
fn push_front(items: &mut Vec<Track>, track: Track) {
    items.retain(|t| !(t.id == track.id && t.provider == track.provider));
    items.insert(0, track);
    items.truncate(HISTORY_MAX);
}

fn serialize(items: &[Track]) -> String {
    let mut text = String::new();
    for t in items {
        let artists = t.artists.iter().map(|a| escape(a)).collect::<Vec<_>>().join(&ARTIST_SEP.to_string());
        text.push_str(&format!("{}\t{}\t{}\t{}\t{artists}\n", t.provider.key(), t.duration_secs, escape(&t.id), escape(&t.title)));
    }
    text
}

fn parse(text: &str) -> Vec<Track> {
    let mut items: Vec<Track> = text.lines().filter_map(parse_line).collect();
    items.truncate(HISTORY_MAX);
    items
}

fn parse_line(line: &str) -> Option<Track> {
    let mut f = line.split('\t');
    let provider = ProviderKind::from_key(f.next()?)?;
    let duration_secs = f.next()?.parse().ok()?;
    let id = unescape(f.next()?);
    let title = unescape(f.next()?);
    let artists_field = f.next()?;
    if f.next().is_some() || id.is_empty() {
        return None;
    }
    let artists = if artists_field.is_empty() { Vec::new() } else { artists_field.split(ARTIST_SEP).map(unescape).collect() };
    Some(Track { id, provider, title, artists, duration_secs })
}

fn escape(s: &str) -> String {
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

fn unescape(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, provider: ProviderKind) -> Track {
        Track { id: id.to_string(), provider, title: format!("Трек {id}"), artists: vec!["А".into(), "Б".into()], duration_secs: 200 }
    }

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-history-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("sub").join("history.txt") // папки sub ещё нет: save должен её создать
    }

    #[test]
    fn puts_latest_first_without_duplicates() {
        let mut h = Vec::new();
        push_front(&mut h, track("a", ProviderKind::Local));
        push_front(&mut h, track("b", ProviderKind::Local));
        push_front(&mut h, track("a", ProviderKind::Local)); // сыгран снова: поднимается наверх
        let ids: Vec<&str> = h.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn same_id_from_different_services_are_different_tracks() {
        let mut h = Vec::new();
        push_front(&mut h, track("1", ProviderKind::Local));
        push_front(&mut h, track("1", ProviderKind::SoundCloud));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn is_capped_at_the_limit() {
        let mut h = Vec::new();
        for i in 0..HISTORY_MAX + 50 {
            push_front(&mut h, track(&i.to_string(), ProviderKind::Local));
        }
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h[0].id, (HISTORY_MAX + 49).to_string()); // свежие остались, старые ушли
    }

    #[test]
    fn survives_restart() {
        let path = temp_file("restart");
        let mut h = History::open(Some(path.clone()));
        h.push(track("1", ProviderKind::SoundCloud));
        h.push(track("2", ProviderKind::Local));
        let reopened = History::open(Some(path.clone()));
        assert_eq!(reopened.items().len(), 2);
        assert_eq!(reopened.items()[0].id, "2");
        assert_eq!(reopened.items()[1].provider, ProviderKind::SoundCloud);
        assert_eq!(reopened.items()[1].artists, ["А", "Б"]);
        assert_eq!(reopened.items()[1].duration_secs, 200);
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
        };
        let back = parse(&serialize(std::slice::from_ref(&t)));
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].id.as_str(), back[0].title.as_str()), (t.id.as_str(), t.title.as_str()));
        assert_eq!(back[0].artists, t.artists);
    }

    #[test]
    fn broken_lines_are_skipped_not_fatal() {
        let good = serialize(&[track("ok", ProviderKind::Local)]);
        let text = format!("мусор\nlocal\tне число\tx\ty\t\n{good}vk\t1\t\tпустой id\t\n");
        let items = parse(&text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "ok");
    }

    #[test]
    fn no_file_means_empty_history() {
        assert!(History::open(Some(temp_file("missing"))).items().is_empty());
        assert!(History::open(None).items().is_empty());
    }
}
