//! «Понравившиеся»: треки, которым владелец поставил сердечко, самый свежий первым. Без лимита.
//! Хранятся на диске (`likes.txt`, формат строки в `track_codec`) и переживают перезапуск.

use std::path::PathBuf;

use crate::track_codec::{decode_track, encode_track, save_or_log};
use crate::{ProviderKind, Track};

/// Понравившиеся треки с сохранением в файл.
#[derive(Debug)]
pub struct Likes {
    items: Vec<Track>,
    /// Куда сохранять; `None` = только в памяти (нет папки приложения, или тест).
    path: Option<PathBuf>,
}

impl Likes {
    /// Открывает список из файла `path` (нет файла = пусто). `None` = только в памяти.
    pub fn open(path: Option<PathBuf>) -> Self {
        let items = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|text| parse(&text)).unwrap_or_default();
        Self { items, path }
    }

    /// Треки, самый свежий лайк первым.
    pub fn items(&self) -> &[Track] {
        &self.items
    }

    /// Стоит ли лайк на треке (трек определяют сервис и id).
    pub fn contains(&self, provider: ProviderKind, id: &str) -> bool {
        self.items.iter().any(|t| t.provider == provider && t.id == id)
    }

    /// Поставить лайк (трек наверх) или снять его, если он уже стоит; сохранить.
    /// Возвращает, стоит ли лайк теперь.
    pub fn toggle(&mut self, track: &Track) -> bool {
        let liked = toggle_in(&mut self.items, track);
        save_or_log(&self.path, "понравившиеся", &serialize(&self.items));
        liked
    }
}

fn toggle_in(items: &mut Vec<Track>, track: &Track) -> bool {
    let before = items.len();
    items.retain(|t| !(t.provider == track.provider && t.id == track.id));
    if items.len() < before {
        return false;
    }
    items.insert(0, track.clone());
    true
}

fn serialize(items: &[Track]) -> String {
    items.iter().map(|t| encode_track(t) + "\n").collect()
}

fn parse(text: &str) -> Vec<Track> {
    text.lines().filter_map(decode_track).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track_codec::tests::{cleanup, temp_file, track};

    #[test]
    fn toggle_likes_then_unlikes() {
        let mut likes = Likes::open(None);
        let t = track("a", ProviderKind::Local);
        assert!(likes.toggle(&t));
        assert!(likes.contains(ProviderKind::Local, "a"));
        assert!(!likes.toggle(&t));
        assert!(!likes.contains(ProviderKind::Local, "a"));
        assert!(likes.items().is_empty());
    }

    #[test]
    fn newest_like_is_first() {
        let mut likes = Likes::open(None);
        likes.toggle(&track("a", ProviderKind::Local));
        likes.toggle(&track("b", ProviderKind::Local));
        let ids: Vec<&str> = likes.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn same_id_from_different_services_are_different_tracks() {
        let mut likes = Likes::open(None);
        likes.toggle(&track("1", ProviderKind::Local));
        likes.toggle(&track("1", ProviderKind::SoundCloud));
        assert_eq!(likes.items().len(), 2);
        assert!(likes.contains(ProviderKind::SoundCloud, "1"));
        likes.toggle(&track("1", ProviderKind::Local)); // снимаем лайк только у локального
        assert!(!likes.contains(ProviderKind::Local, "1"));
        assert!(likes.contains(ProviderKind::SoundCloud, "1"));
    }

    #[test]
    fn survives_restart_and_has_no_limit() {
        let path = temp_file("likes", "restart");
        let mut likes = Likes::open(Some(path.clone()));
        for i in 0..800 {
            likes.toggle(&track(&i.to_string(), ProviderKind::Local)); // больше лимита истории (700)
        }
        likes.toggle(&track("5", ProviderKind::Local)); // снять
        let reopened = Likes::open(Some(path.clone()));
        assert_eq!(reopened.items().len(), 799);
        assert_eq!(reopened.items()[0].id, "799");
        assert!(!reopened.contains(ProviderKind::Local, "5"));
        assert_eq!(reopened.items()[0].artists, ["А", "Б"]);
        cleanup(&path);
    }

    #[test]
    fn broken_lines_are_skipped_not_fatal() {
        let good = encode_track(&track("ok", ProviderKind::Local));
        let items = parse(&format!("мусор\n{good}\nlocal\tx\n"));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "ok");
    }

    #[test]
    fn no_file_means_no_likes() {
        assert!(Likes::open(Some(temp_file("likes", "missing"))).items().is_empty());
    }
}
