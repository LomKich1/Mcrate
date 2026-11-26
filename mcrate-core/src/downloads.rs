//! Список онлайн-треков, сохранённых пользователем. Метаданные переживают перезапуск приложения;
//! наличие самого аудиофайла проверяет `mcrate-audio`, потому что только он владеет хранилищем.

use std::path::PathBuf;

use crate::track_codec::{decode_track, encode_track, save_or_log};
use crate::Track;

/// Индекс метаданных сохранённых треков. Новые загрузки находятся в начале списка.
#[derive(Debug)]
pub struct Downloads {
    items: Vec<Track>,
    /// `None` отключает запись на диск (например, в изолированных проверках).
    path: Option<PathBuf>,
}

impl Downloads {
    /// Загружает индекс из файла; отсутствующий или повреждённый файл считается пустым.
    pub fn open(path: Option<PathBuf>) -> Self {
        let items = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).map(|text| parse(&text)).unwrap_or_default();
        Self { items, path }
    }

    /// Треки, самые недавно добавленные первыми.
    pub fn items(&self) -> &[Track] {
        &self.items
    }

    /// Добавляет или обновляет метаданные трека и поднимает его в начало списка.
    pub fn add(&mut self, track: &Track) {
        self.items.retain(|item| !(item.provider == track.provider && item.id == track.id));
        self.items.insert(0, track.clone());
        save_or_log(&self.path, "индекс загрузок", &serialize(&self.items));
    }

    /// Удаляет метаданные трека и сохраняет индекс. Возвращает `true`, если запись существовала.
    pub fn remove(&mut self, provider: crate::ProviderKind, id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|track| !(track.provider == provider && track.id == id));
        let removed = self.items.len() != before;
        if removed {
            save_or_log(&self.path, "индекс загрузок", &serialize(&self.items));
        }
        removed
    }
}

fn serialize(items: &[Track]) -> String {
    items.iter().map(|track| encode_track(track) + "\n").collect()
}

fn parse(text: &str) -> Vec<Track> {
    let mut items = Vec::new();
    for track in text.lines().filter_map(decode_track) {
        if items.iter().any(|item: &Track| item.provider == track.provider && item.id == track.id) {
            continue;
        }
        items.push(track);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderKind;
    use crate::track_codec::tests::{cleanup, temp_file, track};

    #[test]
    fn downloads_keep_latest_first_without_duplicates() {
        let mut downloads = Downloads::open(None);
        downloads.add(&track("a", ProviderKind::SoundCloud));
        downloads.add(&track("b", ProviderKind::SoundCloud));
        downloads.add(&track("a", ProviderKind::SoundCloud));
        let ids: Vec<&str> = downloads.items().iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn same_id_from_different_services_are_distinct() {
        let mut downloads = Downloads::open(None);
        downloads.add(&track("1", ProviderKind::SoundCloud));
        downloads.add(&track("1", ProviderKind::Vk));
        assert_eq!(downloads.items().len(), 2);
    }

    #[test]
    fn metadata_survives_restart() {
        let path = temp_file("downloads", "restart");
        let mut downloads = Downloads::open(Some(path.clone()));
        downloads.add(&track("42", ProviderKind::SoundCloud));
        let reopened = Downloads::open(Some(path.clone()));
        assert_eq!(reopened.items().len(), 1);
        assert_eq!(reopened.items()[0].title, "Трек 42");
        assert_eq!(reopened.items()[0].artists, ["А", "Б"]);
        assert_eq!(reopened.items()[0].duration_secs, 200);
        cleanup(&path);
    }

    #[test]
    fn remove_download_metadata_is_persisted_and_provider_specific() {
        let path = temp_file("downloads", "remove");
        let mut downloads = Downloads::open(Some(path.clone()));
        downloads.add(&track("same", ProviderKind::SoundCloud));
        downloads.add(&track("same", ProviderKind::Vk));

        assert!(downloads.remove(ProviderKind::SoundCloud, "same"));
        assert!(!downloads.remove(ProviderKind::SoundCloud, "same"));
        let reopened = Downloads::open(Some(path.clone()));
        assert_eq!(reopened.items().len(), 1);
        assert_eq!(reopened.items()[0].provider, ProviderKind::Vk);
        cleanup(&path);
    }

    #[test]
    fn broken_lines_are_skipped() {
        let good = serialize(&[track("ok", ProviderKind::SoundCloud)]);
        assert_eq!(parse(&format!("мусор\n{good}vk\tне число\n")).len(), 1);
    }
}
