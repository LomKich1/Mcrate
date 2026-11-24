//! История прослушивания: последние сыгранные треки, самый свежий первым. Хранится на диске и
//! переживает перезапуск приложения.
//!
//! Файл `history.txt`: одна строка = один трек (формат строки в `track_codec`). Нечитаемые строки
//! пропускаются: порча одной записи не должна стирать всю историю.

use std::path::PathBuf;

use crate::Track;
use crate::track_codec::{decode_track, encode_track, save_or_log};

/// Сколько треков помним в истории.
pub const HISTORY_MAX: usize = 700;

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
        save_or_log(&self.path, "историю", &serialize(&self.items));
    }
}

/// Трек наверх списка без повторов (трек определяют сервис и id), список не длиннее `HISTORY_MAX`.
fn push_front(items: &mut Vec<Track>, track: Track) {
    items.retain(|t| !(t.id == track.id && t.provider == track.provider));
    items.insert(0, track);
    items.truncate(HISTORY_MAX);
}

fn serialize(items: &[Track]) -> String {
    items.iter().map(|t| encode_track(t) + "\n").collect()
}

fn parse(text: &str) -> Vec<Track> {
    let mut items: Vec<Track> = text.lines().filter_map(decode_track).collect();
    items.truncate(HISTORY_MAX);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderKind;
    use crate::track_codec::tests::{cleanup, temp_file as codec_temp_file, track};

    fn temp_file(name: &str) -> PathBuf {
        codec_temp_file("history", name)
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
        cleanup(&path);
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
