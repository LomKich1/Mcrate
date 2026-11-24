//! Плейлисты владельца: имя и список треков в порядке добавления. Хранятся на диске (`playlists.txt`)
//! и переживают перезапуск.
//!
//! Файл: строка `next<TAB>N` (следующий свободный id), затем для каждого плейлиста строка
//! `playlist<TAB>id<TAB>имя`, а под ней строки `track<TAB>`+строка трека (формат в `track_codec`).
//! Id плейлиста устойчив: удалённый id не выдаётся повторно, поэтому ссылка на плейлист не
//! перескочит на чужой. Битые строки пропускаются.

use std::path::PathBuf;

use crate::track_codec::{decode_track, encode_track, escape, save_or_log, unescape};
use crate::{ProviderKind, Track};

/// Один плейлист.
#[derive(Debug, Clone)]
pub struct Playlist {
    /// Устойчивый номер (не меняется при переименовании и удалении других).
    pub id: u64,
    pub name: String,
    /// Треки в порядке добавления, без повторов.
    pub tracks: Vec<Track>,
}

/// Плейлисты с сохранением в файл.
#[derive(Debug)]
pub struct Playlists {
    items: Vec<Playlist>,
    /// Какой id получит следующий плейлист.
    next_id: u64,
    /// Куда сохранять; `None` = только в памяти.
    path: Option<PathBuf>,
}

impl Playlists {
    /// Открывает плейлисты из файла `path` (нет файла = пусто). `None` = только в памяти.
    pub fn open(path: Option<PathBuf>) -> Self {
        let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let (items, next_id) = parse(&text);
        Self { items, next_id, path }
    }

    /// Все плейлисты в порядке создания.
    pub fn list(&self) -> &[Playlist] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&Playlist> {
        self.items.iter().find(|p| p.id == id)
    }

    /// Создать плейлист. Имя обрезается по краям; пустое имя не принимается (`None`).
    /// Возвращает id нового плейлиста.
    pub fn create(&mut self, name: &str) -> Option<u64> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Playlist { id, name: name.to_string(), tracks: Vec::new() });
        self.save();
        Some(id)
    }

    /// Переименовать. `false`, если плейлиста нет или имя пустое.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim();
        let Some(p) = self.items.iter_mut().find(|p| p.id == id) else { return false };
        if name.is_empty() {
            return false;
        }
        p.name = name.to_string();
        self.save();
        true
    }

    /// Удалить плейлист. `false`, если его нет.
    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|p| p.id != id);
        let removed = self.items.len() < before;
        if removed {
            self.save();
        }
        removed
    }

    /// Добавить трек в конец. `false`, если плейлиста нет или такой трек (сервис и id) в нём уже есть.
    pub fn add_track(&mut self, id: u64, track: &Track) -> bool {
        let Some(p) = self.items.iter_mut().find(|p| p.id == id) else { return false };
        if p.tracks.iter().any(|t| t.provider == track.provider && t.id == track.id) {
            return false;
        }
        p.tracks.push(track.clone());
        self.save();
        true
    }

    /// Убрать трек. `false`, если плейлиста или трека нет.
    pub fn remove_track(&mut self, id: u64, provider: ProviderKind, track_id: &str) -> bool {
        let Some(p) = self.items.iter_mut().find(|p| p.id == id) else { return false };
        let before = p.tracks.len();
        p.tracks.retain(|t| !(t.provider == provider && t.id == track_id));
        let removed = p.tracks.len() < before;
        if removed {
            self.save();
        }
        removed
    }

    fn save(&self) {
        save_or_log(&self.path, "плейлисты", &serialize(&self.items, self.next_id));
    }
}

fn serialize(items: &[Playlist], next_id: u64) -> String {
    let mut text = format!("next\t{next_id}\n");
    for p in items {
        text.push_str(&format!("playlist\t{}\t{}\n", p.id, escape(&p.name)));
        for t in &p.tracks {
            text.push_str(&format!("track\t{}\n", encode_track(t)));
        }
    }
    text
}

/// Плейлисты и следующий свободный id (не меньше записанного и не меньше `max id + 1`).
fn parse(text: &str) -> (Vec<Playlist>, u64) {
    let mut items: Vec<Playlist> = Vec::new();
    let mut next_id = 1;
    // Пока идут строки плейлиста с повторным id или битой шапкой, его треки отбрасываются.
    let mut current_ok = false;
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("next\t") {
            next_id = n.parse().unwrap_or(next_id);
        } else if let Some(rest) = line.strip_prefix("playlist\t") {
            current_ok = false;
            let Some((id, name)) = rest.split_once('\t') else { continue };
            let Ok(id) = id.parse::<u64>() else { continue };
            if items.iter().any(|p| p.id == id) {
                continue;
            }
            items.push(Playlist { id, name: unescape(name), tracks: Vec::new() });
            current_ok = true;
        } else if let Some(rest) = line.strip_prefix("track\t")
            && current_ok
            && let (Some(track), Some(p)) = (decode_track(rest), items.last_mut())
        {
            p.tracks.push(track);
        }
    }
    let min_next = items.iter().map(|p| p.id + 1).max().unwrap_or(1);
    (items, next_id.max(min_next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track_codec::tests::{cleanup, temp_file, track};

    fn local(id: &str) -> Track {
        track(id, ProviderKind::Local)
    }

    #[test]
    fn create_trims_name_and_rejects_empty() {
        let mut pl = Playlists::open(None);
        assert_eq!(pl.create("   "), None);
        let id = pl.create("  Дорога  ").unwrap();
        assert_eq!(pl.get(id).unwrap().name, "Дорога");
        assert_eq!(pl.list().len(), 1);
    }

    #[test]
    fn rename_and_delete() {
        let mut pl = Playlists::open(None);
        let a = pl.create("А").unwrap();
        let b = pl.create("Б").unwrap();
        assert!(pl.rename(a, "Новое"));
        assert!(!pl.rename(a, "  "), "пустое имя не принимается");
        assert!(!pl.rename(99, "x"));
        assert_eq!(pl.get(a).unwrap().name, "Новое");
        assert!(pl.delete(a));
        assert!(!pl.delete(a));
        assert!(pl.get(a).is_none() && pl.get(b).is_some());
    }

    #[test]
    fn add_track_appends_without_duplicates() {
        let mut pl = Playlists::open(None);
        let id = pl.create("П").unwrap();
        assert!(pl.add_track(id, &local("a")));
        assert!(pl.add_track(id, &local("b")));
        assert!(!pl.add_track(id, &local("a")), "повтор не добавляется");
        assert!(pl.add_track(id, &track("a", ProviderKind::SoundCloud)), "тот же id другого сервиса это другой трек");
        assert!(!pl.add_track(99, &local("a")));
        let ids: Vec<&str> = pl.get(id).unwrap().tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "a"]);
    }

    #[test]
    fn remove_track_only_matching_service() {
        let mut pl = Playlists::open(None);
        let id = pl.create("П").unwrap();
        pl.add_track(id, &local("a"));
        pl.add_track(id, &track("a", ProviderKind::SoundCloud));
        assert!(pl.remove_track(id, ProviderKind::Local, "a"));
        assert!(!pl.remove_track(id, ProviderKind::Local, "a"));
        assert!(!pl.remove_track(99, ProviderKind::Local, "a"));
        let left = &pl.get(id).unwrap().tracks;
        assert_eq!((left.len(), left[0].provider), (1, ProviderKind::SoundCloud));
    }

    #[test]
    fn deleted_id_is_never_reused_even_after_restart() {
        let path = temp_file("playlists", "ids");
        let mut pl = Playlists::open(Some(path.clone()));
        let a = pl.create("А").unwrap();
        let b = pl.create("Б").unwrap();
        pl.delete(b); // удалён плейлист с наибольшим id
        let mut reopened = Playlists::open(Some(path.clone()));
        let c = reopened.create("В").unwrap();
        assert!(c > b && c != a, "новый id {c} не должен совпасть с удалённым {b}");
        cleanup(&path);
    }

    #[test]
    fn survives_restart_with_tracks_and_special_names() {
        let path = temp_file("playlists", "restart");
        let mut pl = Playlists::open(Some(path.clone()));
        let id = pl.create("Имя\tс табом\nи строкой\\").unwrap();
        pl.add_track(id, &local("1"));
        pl.add_track(id, &track("2", ProviderKind::SoundCloud));
        let other = pl.create("Пустой").unwrap();
        let re = Playlists::open(Some(path.clone()));
        assert_eq!(re.list().len(), 2);
        let p = re.get(id).unwrap();
        assert_eq!(p.name, "Имя\tс табом\nи строкой\\");
        assert_eq!(p.tracks.len(), 2);
        assert_eq!((p.tracks[1].id.as_str(), p.tracks[1].provider), ("2", ProviderKind::SoundCloud));
        assert_eq!(p.tracks[0].artists, ["А", "Б"]);
        assert!(re.get(other).unwrap().tracks.is_empty());
        cleanup(&path);
    }

    #[test]
    fn broken_lines_are_skipped_not_fatal() {
        let good = encode_track(&local("ok"));
        let text = format!(
            "next\tчепуха\nмусор\nplaylist\tне число\tИмя\ntrack\t{good}\nplaylist\t7\tЖивой\ntrack\tсломан\ntrack\t{good}\nplaylist\t7\tДубль\ntrack\t{good}\n"
        );
        let (items, next) = parse(&text);
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].id, items[0].name.as_str(), items[0].tracks.len()), (7, "Живой", 1));
        assert_eq!(next, 8);
    }

    #[test]
    fn no_file_means_no_playlists() {
        assert!(Playlists::open(Some(temp_file("playlists", "missing"))).list().is_empty());
    }
}
