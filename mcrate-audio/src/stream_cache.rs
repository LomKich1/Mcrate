//! Кэш скачанных онлайн-треков: папка во временных файлах Windows (%TEMP%\mcrate\stream\).
//!
//! Зачем: движок скачивает трек целиком в файл, а плеер играет файл. Если пользователь
//! вернётся к уже скачанному треку, второй раз качать не нужно (на медленном интернете
//! это экономит минуту). Старые файлы удаляем, чтобы папка не росла бесконечно.
//!
//! Имя файла: `<сервис>-<id трека>.<расширение>`, например `soundcloud-123456.m4a`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mcrate_core::{ProviderKind, StreamSource, Track};

/// Папка кэша и предел количества файлов. `Clone` нужен, чтобы отдать копию фоновой задаче.
#[derive(Debug, Clone)]
pub(crate) struct StreamCache {
    dir: PathBuf,
    keep: usize, // сколько последних файлов хранить
}

impl StreamCache {
    pub(crate) fn new(dir: PathBuf, keep: usize) -> Self {
        Self { dir, keep }
    }

    /// Папка по умолчанию: `%TEMP%\mcrate\stream` (`temp_dir()` сам знает, где временная папка).
    pub(crate) fn default_dir() -> PathBuf {
        std::env::temp_dir().join("mcrate").join("stream")
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Лежит ли файл внутри папки кэша (чтобы не удалить чужой файл, например локальный трек).
    pub(crate) fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.dir)
    }

    /// Имя файла без расширения: `<сервис>-<id>`. Расширение до запроса ссылки неизвестно,
    /// поэтому кэш ищет файл по этой основе (`find`).
    pub(crate) fn stem(track: &Track) -> String {
        format!("{}-{}", provider_name(track.provider), sanitize_id(&track.id))
    }

    /// Ищет уже скачанный трек (с любым расширением). Найденный файл «освежаем»:
    /// ставим время изменения «сейчас», чтобы он считался недавним и не удалился первым.
    pub(crate) fn find(&self, stem: &str) -> Option<PathBuf> {
        let entries = fs::read_dir(&self.dir).ok()?; // нет папки = в кэше пусто
        for entry in entries.flatten() {
            let path = entry.path();
            let is_part = path.extension().is_some_and(|e| e == "part"); // недокачанный файл не берём
            let same_stem = path.file_stem().is_some_and(|s| s.to_string_lossy() == stem);
            let not_empty = entry.metadata().map(|m| m.is_file() && m.len() > 0).unwrap_or(false);
            if !is_part && same_stem && not_empty {
                touch(&path);
                return Some(path);
            }
        }
        None
    }

    /// Куда сохранить скачанный трек.
    pub(crate) fn target(&self, stem: &str, source: &StreamSource) -> PathBuf {
        self.dir.join(format!("{stem}.{}", extension_for(source)))
    }

    /// Удаляет самые старые файлы, оставляя не больше `keep`. Файл `protect` не трогаем
    /// (он только что скачан или сейчас играет). Ошибки удаления игнорируем: на Windows
    /// файл, открытый плеером, удалить нельзя, и это нормально.
    pub(crate) fn prune(&self, protect: &Path) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
            })
            .collect();
        if files.len() <= self.keep {
            return;
        }
        files.sort(); // сортировка по времени (старые первыми), при равенстве по пути
        let mut to_remove = files.len() - self.keep;
        for (_, path) in files {
            if to_remove == 0 {
                break;
            }
            if path == protect {
                continue;
            }
            if fs::remove_file(&path).is_ok() {
                to_remove -= 1;
            }
        }
    }
}

/// Путь временного файла для докачки: `x.m4a` -> `x.m4a.part`. Скачиваем сюда,
/// а потом переименовываем: недокачанный файл никогда не выглядит как готовый.
pub(crate) fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

fn touch(path: &Path) {
    // Открываем на запись и ставим время изменения «сейчас». Ошибки не важны.
    let _ = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now()));
}

fn provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Yandex => "yandex",
        ProviderKind::Vk => "vk",
        ProviderKind::SoundCloud => "soundcloud",
        ProviderKind::Spotify => "spotify",
        ProviderKind::Local => "local",
    }
}

/// Делает id безопасным для имени файла: оставляет латиницу, цифры, `-` и `_`.
/// Если что-то пришлось заменить, добавляет хэш исходного id, чтобы разные id
/// (`a/b` и `a_b`) не получили одно и то же имя и не подменили друг друга.
fn sanitize_id(id: &str) -> String {
    let clean: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(80)
        .collect();
    if clean == id {
        clean
    } else {
        format!("{clean}-{:08x}", fnv1a(id))
    }
}

/// Простой хэш FNV-1a (32 бита): одинаковый на любом запуске, чего нельзя сказать про HashMap.
fn fnv1a(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Расширение файла по типу источника. Плеер определяет формат по содержимому,
/// так что расширение нужно только для человека, который заглянет в папку.
fn extension_for(source: &StreamSource) -> String {
    match source {
        StreamSource::HlsPlaylist(_) => "m4a".to_string(), // куски fMP4 с AAC
        StreamSource::DirectUrl(url) => {
            let path = url.split(['?', '#']).next().unwrap_or(url); // без хвоста после ? и #
            let last = path.rsplit('/').next().unwrap_or("");
            match last.rsplit_once('.') {
                Some((_, ext)) if (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
                    ext.to_ascii_lowercase()
                }
                _ => "bin".to_string(),
            }
        }
        StreamSource::LocalFile(_) => "bin".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn track(id: &str, provider: ProviderKind) -> Track {
        Track { id: id.to_string(), provider, title: "t".into(), artists: vec![], duration_secs: 1 }
    }

    /// Уникальная пустая папка для теста (без внешних крейтов).
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-cache-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Создаёт файл и задаёт ему возраст: `age_secs` секунд назад.
    fn make_file(dir: &Path, name: &str, age_secs: u64) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"x").unwrap();
        let f = fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(age_secs)).unwrap();
        path
    }

    #[test]
    fn stem_uses_provider_and_id() {
        assert_eq!(StreamCache::stem(&track("123", ProviderKind::SoundCloud)), "soundcloud-123");
        assert_eq!(StreamCache::stem(&track("ab-c_9", ProviderKind::Vk)), "vk-ab-c_9");
    }

    #[test]
    fn unsafe_ids_get_unique_safe_names() {
        let a = StreamCache::stem(&track("a/b", ProviderKind::Yandex));
        let b = StreamCache::stem(&track("a_b", ProviderKind::Yandex));
        let c = StreamCache::stem(&track("..\\evil", ProviderKind::Yandex));
        assert_ne!(a, b); // после замены символов имена не должны совпасть
        assert!(!a.contains('/') && !c.contains('\\') && !c.contains(".."));
        assert_eq!(a, StreamCache::stem(&track("a/b", ProviderKind::Yandex))); // имя стабильное
    }

    #[test]
    fn extension_follows_source() {
        let hls = StreamSource::HlsPlaylist("https://x/y/playlist.m3u8?sig=1".into());
        assert_eq!(extension_for(&hls), "m4a");
        let mp3 = StreamSource::DirectUrl("https://x/y/Song.MP3?token=a.b".into());
        assert_eq!(extension_for(&mp3), "mp3");
        let none = StreamSource::DirectUrl("https://x/stream".into());
        assert_eq!(extension_for(&none), "bin");
        let weird = StreamSource::DirectUrl("https://x/a.toolongext".into());
        assert_eq!(extension_for(&weird), "bin");
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(part_path(Path::new("/c/soundcloud-1.m4a")), PathBuf::from("/c/soundcloud-1.m4a.part"));
    }

    #[test]
    fn find_ignores_partial_and_empty_files() {
        let dir = temp_dir("find");
        let cache = StreamCache::new(dir.clone(), 20);
        assert!(cache.find("soundcloud-1").is_none()); // пусто
        fs::write(dir.join("soundcloud-1.m4a.part"), b"half").unwrap();
        fs::write(dir.join("soundcloud-1.m4a"), b"").unwrap(); // пустой файл = брак
        assert!(cache.find("soundcloud-1").is_none());
        fs::write(dir.join("soundcloud-1.m4a"), b"audio").unwrap();
        assert_eq!(cache.find("soundcloud-1"), Some(dir.join("soundcloud-1.m4a")));
        assert!(cache.find("soundcloud-10").is_none()); // другая основа имени, хоть и похожая
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_removes_oldest_and_keeps_protected() {
        let dir = temp_dir("prune");
        let cache = StreamCache::new(dir.clone(), 3);
        let oldest = make_file(&dir, "a.m4a", 500);
        let old = make_file(&dir, "b.m4a", 400);
        let mid = make_file(&dir, "c.m4a", 300);
        let new = make_file(&dir, "d.m4a", 200);
        let newest = make_file(&dir, "e.m4a", 100);
        cache.prune(&newest); // 5 файлов, лимит 3: уйти должны два самых старых
        assert!(!oldest.exists() && !old.exists());
        assert!(mid.exists() && new.exists() && newest.exists());

        // Защищённый файл не удаляется, даже если он самый старый
        let cache2 = StreamCache::new(dir.clone(), 1);
        cache2.prune(&mid); // осталось 3, лимит 1: `mid` защищён, уходят `new` и `newest`
        assert!(mid.exists());
        assert!(!new.exists() && !newest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_refreshes_file_age() {
        let dir = temp_dir("touch");
        let cache = StreamCache::new(dir.clone(), 20);
        let path = make_file(&dir, "soundcloud-7.m4a", 1000);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(cache.find("soundcloud-7"), Some(path.clone()));
        let after = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(after > before); // время изменения сдвинулось к «сейчас»
        let _ = fs::remove_dir_all(&dir);
    }
}
