//! Хранилища онлайн-треков: временный кэш и постоянная папка загрузок.
//!
//! Имена файлов одинаковы в обоих местах: `<сервис>-<id трека>.<расширение>`. При
//! воспроизведении сначала ищем постоянную загрузку, затем кэш; кэш ограничен по размеру,
//! постоянные загрузки не удаляются автоматически.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use mcrate_core::{Downloads, ProviderKind, StreamSource, Track};

/// Сводка по кэшу: сколько в нём файлов и сколько они занимают.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    /// Число файлов в папке (недокачанные `.part` тоже считаются).
    pub files: usize,
    /// Их общий размер в байтах.
    pub bytes: u64,
}

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
        Self::stem_of(track.provider, &track.id)
    }

    /// То же по сервису и id (когда самого `Track` под рукой нет, например у ключа предзагрузки).
    pub(crate) fn stem_of(provider: ProviderKind, id: &str) -> String {
        format!("{}-{}", provider_name(provider), sanitize_id(id))
    }

    /// Сколько файлов в кэше и сколько они занимают. Нет папки = кэш пуст.
    pub(crate) fn stats(&self) -> CacheStats {
        let mut stats = CacheStats::default();
        let Ok(entries) = fs::read_dir(&self.dir) else { return stats };
        for meta in entries.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()) {
            stats.bytes += meta.len();
            stats.files += 1;
        }
        stats
    }

    /// Удаляет скачанные треки, кроме тех, чьё имя без расширения (`stem`) есть в `keep`
    /// (играющий и подготовленный следующий). Недокачанные `.part` не трогаем: их ещё пишет загрузка.
    /// Ошибки удаления игнорируем (в Windows открытый плеером файл удалить нельзя). Возвращает, сколько удалено.
    pub(crate) fn clear(&self, keep: &[String]) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else { return 0 };
        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_file = entry.metadata().map(|m| m.is_file()).unwrap_or(false);
            let is_part = path.extension().is_some_and(|e| e == "part");
            let kept = path.file_stem().is_some_and(|s| keep.iter().any(|k| s.to_string_lossy() == k.as_str()));
            if is_file && !is_part && !kept && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Ищет уже скачанный трек (с любым расширением). Найденный файл «освежаем»:
    /// ставим время изменения «сейчас», чтобы он считался недавним и не удалился первым.
    pub(crate) fn find(&self, stem: &str) -> Option<PathBuf> {
        find_track(&self.dir, stem, true)
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

/// Постоянная папка треков, сохранённых пользователем. В отличие от кэша, файлы здесь
/// не удаляются автоматически и используются при каждом следующем воспроизведении.
#[derive(Debug, Clone)]
pub(crate) struct DownloadStore {
    dir: PathBuf,
    /// Метаданные лежат рядом с папкой аудиофайлов и обновляются потоком движка.
    index: Arc<Mutex<Downloads>>,
}

impl DownloadStore {
    pub(crate) fn new(dir: PathBuf) -> Self {
        let index_path = if dir == Self::default_dir() {
            mcrate_core::paths::downloads_index_file().or_else(|| dir.parent().map(|parent| parent.join("downloads.txt")))
        } else {
            Some(dir.join("downloads.txt"))
        };
        Self { dir, index: Arc::new(Mutex::new(Downloads::open(index_path))) }
    }

    /// Папка `%APPDATA%\Mcrate\downloads`.
    pub(crate) fn default_dir() -> PathBuf {
        mcrate_core::paths::downloads_dir().unwrap_or_else(|| std::env::temp_dir().join("mcrate").join("downloads"))
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.dir)
    }

    /// Сохраняет метаданные после успешной записи аудиофайла. Если индекс временно недоступен,
    /// сам аудиофайл не теряем: `Downloads` оставляет метаданные в памяти и пишет ошибку в stderr.
    pub(crate) fn remember(&self, track: &Track) {
        if let Some(path) = find_track(&self.dir, &StreamCache::stem(track), false) {
            touch(&path); // перенос из кэша тоже становится свежей загрузкой в списке.
        }
        let mut index = self.index.lock().unwrap_or_else(|error| error.into_inner());
        index.add(track);
    }

    /// Удаляет постоянный аудиофайл трека и его запись из общего индекса. Временный потоковый кэш не трогает.
    pub(crate) fn remove(&self, track: &Track) -> Result<(), String> {
        let stem = StreamCache::stem(track);
        match fs::read_dir(&self.dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|error| format!("не удалось прочитать папку загрузок: {error}"))?;
                    let path = entry.path();
                    if path.extension().is_some_and(|extension| extension == "part")
                        || !path.file_stem().is_some_and(|name| name.to_string_lossy() == stem.as_str())
                        || !entry.file_type().map_err(|error| format!("не удалось проверить {}: {error}", path.display()))?.is_file()
                    {
                        continue;
                    }
                    match fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(format!("не удалось удалить {}: {error}", path.display())),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("не удалось открыть папку загрузок {}: {error}", self.dir.display())),
        }
        let mut index = self.index.lock().unwrap_or_else(|error| error.into_inner());
        index.remove(track.provider, &track.id);
        Ok(())
    }

    /// Возвращает существующие загрузки, новые первыми по времени файла. Старые файлы без индекса
    /// тоже показываем: имя содержит сервис и id, поэтому создаём минимальную запись, пока метаданные
    /// не удастся восстановить из истории или плейлиста на стороне Controller.
    pub(crate) fn tracks(&self) -> Vec<Track> {
        let indexed = self.index.lock().unwrap_or_else(|error| error.into_inner()).items().to_vec();
        let mut found = Vec::<(SystemTime, Track)>::new();
        let mut known_stems = HashSet::new();

        for track in indexed {
            let stem = StreamCache::stem(&track);
            let Some(path) = find_track(&self.dir, &stem, false) else { continue };
            known_stems.insert(stem);
            let modified = fs::metadata(path).and_then(|meta| meta.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((modified, track));
        }

        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(metadata) = entry.metadata() else { continue };
                if !metadata.is_file() || metadata.len() == 0 || path.extension().is_some_and(|ext| ext == "part") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else { continue };
                if !known_stems.insert(stem.to_string()) {
                    continue;
                }
                let Some((service, id)) = stem.split_once('-') else { continue };
                let Some(provider) = ProviderKind::from_key(service) else { continue };
                if provider == ProviderKind::Local {
                    continue;
                }
                let track = Track {
                    id: id.to_string(),
                    provider,
                    title: format!("Трек {id}"),
                    artists: Vec::new(),
                    duration_secs: 0,
                };
                found.push((metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), track));
            }
        }

        found.sort_by(|a, b| b.0.cmp(&a.0));
        found.into_iter().map(|(_, track)| track).collect()
    }

    /// Проверяет, что постоянная копия трека уже существует. Время файла не меняем:
    /// постоянная загрузка не участвует в очистке и должна сохранять дату загрузки.
    pub(crate) fn find(&self, stem: &str) -> Option<PathBuf> {
        find_track(&self.dir, stem, false)
    }

    pub(crate) fn target(&self, stem: &str, source: &StreamSource) -> PathBuf {
        self.dir.join(format!("{stem}.{}", extension_for(source)))
    }

    /// Переносит завершённый файл из кэша в постоянное хранилище. Сначала пробуем rename;
    /// если Windows не разрешает переименовать открытый плеером файл, копируем во временный
    /// файл и атомарно публикуем его, оставляя исходник в кэше, если удалить его нельзя.
    pub(crate) fn adopt_cache_file(&self, source: &Path, stem: &str) -> Result<PathBuf, String> {
        let file_name = source.file_name().ok_or_else(|| format!("неверный путь к файлу кэша: {}", source.display()))?;
        fs::create_dir_all(&self.dir).map_err(|e| format!("папка загрузок {}: {e}", self.dir.display()))?;
        let dest = self.dir.join(file_name);
        if dest.exists() {
            let valid = fs::metadata(&dest).map(|m| m.is_file() && m.len() > 0).unwrap_or(false);
            if valid && dest.file_stem().is_some_and(|s| s.to_string_lossy() == stem) {
                return Ok(dest);
            }
            let _ = fs::remove_file(&dest);
        }
        match fs::rename(source, &dest) {
            Ok(()) => Ok(dest),
            Err(rename_error) => {
                let part = part_path(&dest);
                if let Err(copy_error) = fs::copy(source, &part) {
                    let _ = fs::remove_file(&part);
                    return Err(format!("не удалось переместить файл из кэша ({rename_error}) или скопировать его ({copy_error})"));
                }
                if let Err(error) = fs::rename(&part, &dest) {
                    let _ = fs::remove_file(&part);
                    return Err(format!("не удалось сохранить файл в загрузки {}: {error}", dest.display()));
                }
                let _ = fs::remove_file(source);
                Ok(dest)
            }
        }
    }

    /// Куда сохранить готовый файл из сетевого источника. Сначала создаём папку,
    /// затем вызывающий код пишет в `.part` и переименовывает его после успешной загрузки.
    pub(crate) fn prepare_target(&self, stem: &str, source: &StreamSource) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("папка загрузок {}: {e}", self.dir.display()))?;
        let dest = self.target(stem, source);
        if fs::metadata(&dest).is_ok_and(|metadata| metadata.is_file() && metadata.len() == 0) {
            let _ = fs::remove_file(&dest);
        }
        Ok(dest)
    }

    /// Копирует источник `LocalFile` в постоянную папку (полезно для провайдеров,
    /// которые могут возвращать файл вместо сетевой ссылки).
    pub(crate) fn import_file(&self, source: &Path, stem: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("папка загрузок {}: {e}", self.dir.display()))?;
        let extension = source.extension().and_then(|e| e.to_str()).filter(|e| (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("bin");
        let dest = self.dir.join(format!("{stem}.{}", extension.to_ascii_lowercase()));
        if fs::metadata(&dest).is_ok_and(|metadata| metadata.is_file() && metadata.len() == 0) {
            let _ = fs::remove_file(&dest);
        }
        let part = part_path(&dest);
        if let Err(error) = fs::copy(source, &part) {
            let _ = fs::remove_file(&part);
            return Err(format!("не удалось скопировать {}: {error}", source.display()));
        }
        fs::rename(&part, &dest).map_err(|e| {
            let _ = fs::remove_file(&part);
            format!("не удалось сохранить {}: {e}", dest.display())
        })?;
        Ok(dest)
    }
}

/// Ищет готовый файл с указанной основой имени. `.part` и пустые файлы не считаются загрузками.
fn find_track(dir: &Path, stem: &str, refresh: bool) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_part = path.extension().is_some_and(|e| e == "part");
        let same_stem = path.file_stem().is_some_and(|s| s.to_string_lossy() == stem);
        let not_empty = entry.metadata().map(|m| m.is_file() && m.len() > 0).unwrap_or(false);
        if !is_part && same_stem && not_empty {
            if refresh {
                touch(&path);
            }
            return Some(path);
        }
    }
    None
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
    let _ = fs::OpenOptions::new().write(true).open(path).and_then(|f| f.set_modified(SystemTime::now()));
}

fn provider_name(kind: ProviderKind) -> &'static str {
    kind.key() // то же имя, что в сохранённых данных ядра: имена файлов кэша не меняются
}

/// Делает id безопасным для имени файла: оставляет латиницу, цифры, `-` и `_`.
/// Если что-то пришлось заменить, добавляет хэш исходного id, чтобы разные id
/// (`a/b` и `a_b`) не получили одно и то же имя и не подменили друг друга.
fn sanitize_id(id: &str) -> String {
    let clean: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(80).collect();
    if clean == id { clean } else { format!("{clean}-{:08x}", fnv1a(id)) }
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

    #[test]
    fn stats_count_files_and_bytes() {
        let dir = temp_dir("stats");
        make_file(&dir, "a.m4a", 0);
        make_file(&dir, "b.m4a.part", 0);
        let cache = StreamCache::new(dir.clone(), 20);
        assert_eq!(cache.stats(), CacheStats { files: 2, bytes: 2 });
        assert_eq!(StreamCache::new(dir.join("нет такой"), 20).stats(), CacheStats::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn downloads_find_without_refreshing_file_age() {
        let dir = temp_dir("downloads-find");
        let store = DownloadStore::new(dir.clone());
        let path = make_file(&dir, "soundcloud-7.m4a", 1000);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(store.find("soundcloud-7"), Some(path.clone()));
        let after = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(after, before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_file_can_be_adopted_as_permanent_download() {
        let root = temp_dir("adopt");
        let cache = root.join("cache");
        let downloads = root.join("downloads");
        fs::create_dir_all(&cache).unwrap();
        let source = cache.join("soundcloud-42.m4a");
        fs::write(&source, b"audio").unwrap();
        let store = DownloadStore::new(downloads.clone());
        let saved = store.adopt_cache_file(&source, "soundcloud-42").unwrap();
        assert_eq!(saved, downloads.join("soundcloud-42.m4a"));
        assert_eq!(fs::read(&saved).unwrap(), b"audio");
        assert!(!source.exists());
        assert_eq!(store.find("soundcloud-42"), Some(saved));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_download_deletes_matching_file_and_index_but_keeps_other_service() {
        let dir = temp_dir("downloads-remove");
        let store = DownloadStore::new(dir.clone());
        let soundcloud = track("42", ProviderKind::SoundCloud);
        let vk = track("42", ProviderKind::Vk);
        store.index.lock().unwrap().add(&soundcloud);
        store.index.lock().unwrap().add(&vk);

        let soundcloud_file = make_file(&dir, "soundcloud-42.m4a", 0);
        let vk_file = make_file(&dir, "vk-42.m4a", 0);
        let partial = dir.join("soundcloud-42.m4a.part");
        fs::write(&partial, b"partial").unwrap();

        store.remove(&soundcloud).unwrap();

        assert!(!soundcloud_file.exists());
        assert!(vk_file.exists());
        assert!(partial.exists());
        let remaining = store.tracks();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].provider, ProviderKind::Vk);
        assert_eq!(remaining[0].id, "42");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_store_lists_legacy_files_and_persists_track_metadata() {
        let dir = temp_dir("download-index");
        let store = DownloadStore::new(dir.clone());
        fs::write(dir.join("soundcloud-42.m4a"), b"audio").unwrap();
        fs::write(dir.join("soundcloud-43.m4a.part"), b"partial").unwrap();

        let legacy = store.tracks();
        assert_eq!(legacy.len(), 1);
        assert_eq!((legacy[0].provider, legacy[0].id.as_str()), (ProviderKind::SoundCloud, "42"));

        let full = Track {
            id: "42".into(),
            provider: ProviderKind::SoundCloud,
            title: "Песня".into(),
            artists: vec!["Исполнитель".into()],
            duration_secs: 137,
        };
        store.remember(&full);
        let listed = store.tracks();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Песня");
        assert_eq!(listed[0].artists, ["Исполнитель"]);
        assert_eq!(listed[0].duration_secs, 137);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_file_can_be_imported_into_download_store() {
        let root = temp_dir("import");
        let source = root.join("source.wav");
        fs::write(&source, b"audio").unwrap();
        let store = DownloadStore::new(root.join("downloads"));
        let saved = store.import_file(&source, "soundcloud-13").unwrap();
        assert_eq!(saved, root.join("downloads").join("soundcloud-13.wav"));
        assert_eq!(fs::read(saved).unwrap(), b"audio");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clear_keeps_playing_and_unfinished_files() {
        let dir = temp_dir("clear");
        let playing = make_file(&dir, "soundcloud-1.m4a", 0);
        let old = make_file(&dir, "soundcloud-2.m4a", 100);
        let part = make_file(&dir, "soundcloud-3.m4a.part", 100);
        let cache = StreamCache::new(dir.clone(), 20);
        assert_eq!(cache.clear(&["soundcloud-1".to_string()]), 1);
        assert!(playing.exists(), "играющий файл остался");
        assert!(part.exists(), "недокачанный файл остался");
        assert!(!old.exists(), "лишний файл удалён");
        let _ = fs::remove_dir_all(&dir);
    }
}
