//! Провайдер своих файлов: читает теги (название, исполнитель) и длину из аудиофайлов.

use std::path::{Path, PathBuf};
use std::sync::RwLock; // замок «много читают или один пишет»

use async_trait::async_trait;
use lofty::file::{AudioFile, TaggedFileExt}; // AudioFile даёт properties() (длина), TaggedFileExt даёт primary_tag() и first_tag()
use lofty::tag::Accessor;       // даёт методы title(), artist()
use mcrate_core::{
    MusicProvider, ProviderError, ProviderKind, ProviderResult, StreamSource, Track,
};

/// Расширения, которые умеет проигрывать наш звуковой слой.
/// Список проверим на практике, opus сейчас не включён.
const AUDIO_EXTENSIONS: [&str; 6] = ["mp3", "flac", "ogg", "wav", "m4a", "aac"];

/// Одна запись библиотеки: трек плюс путь к его файлу.
struct LocalEntry {
    track: Track,
    path: PathBuf,
}

pub struct LocalProvider {
    // Методы трейта принимают &self (только чтение), а добавлять файлы надо.
    // RwLock позволяет менять данные через &self (interior mutability, «внутренняя изменяемость»).
    entries: RwLock<Vec<LocalEntry>>,
}

impl LocalProvider {
    pub fn new() -> Self {
        Self { entries: RwLock::new(Vec::new()) }
    }

    /// Добавить один файл в библиотеку. Тот же файл дважды не добавляется.
    pub fn add_file(&self, path: &Path) -> ProviderResult<Track> {
        let id = path.to_string_lossy().into_owned(); // id трека = путь к файлу

        // Уже есть? Вернём существующий (.read() = взять замок на чтение)
        if let Some(e) = self.entries.read().unwrap().iter().find(|e| e.track.id == id) {
            return Ok(e.track.clone());
        }

        // map_err превращает чужую ошибку lofty в нашу ProviderError::LocalFile
        let tagged = lofty::read_from_path(path)
            .map_err(|e| ProviderError::LocalFile(format!("{}: {e}", path.display())))?;

        let duration_secs = tagged.properties().duration().as_secs() as u32;

        // primary_tag = главный тег формата; first_tag = любой первый, если главного нет
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

        // Название из тега; если тега нет, берём имя файла без расширения
        let title = tag
            .and_then(|t| t.title().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Без названия".to_string())
            });

        let artists: Vec<String> = tag
            .and_then(|t| t.artist().map(|s| s.to_string()))
            .into_iter() // Option превращаем в итератор из 0 или 1 элемента
            .collect();

        let track = Track {
            id,
            provider: ProviderKind::Local,
            title,
            artists,
            duration_secs,
        };

        // .write() = замок на запись
        self.entries
            .write()
            .unwrap() // unwrap = упасть, если замок «отравлен» (другой поток упал, держа его)
            .push(LocalEntry { track: track.clone(), path: path.to_path_buf() });
        Ok(track)
    }

    /// Добавить все аудиофайлы из папки (без вложенных папок).
    /// Возвращает, сколько добавлено. Файлы с ошибками пропускаются.
    pub fn add_folder(&self, dir: &Path) -> ProviderResult<usize> {
        let read_dir = std::fs::read_dir(dir)
            .map_err(|e| ProviderError::LocalFile(format!("{}: {e}", dir.display())))?;

        let mut added = 0;
        for item in read_dir.flatten() { // flatten выбрасывает записи с ошибкой чтения
            let path = item.path();
            let is_audio = path
                .extension() // расширение, например "mp3"
                .and_then(|e| e.to_str())
                .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false);
            if is_audio && self.add_file(&path).is_ok() {
                added += 1;
            }
        }
        Ok(added)
    }
}

impl Default for LocalProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MusicProvider for LocalProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Local
    }

    /// Поиск по названию и исполнителю без учёта регистра. Пустой запрос = все треки.
    async fn search(&self, query: &str) -> ProviderResult<Vec<Track>> {
        let q = query.to_lowercase();
        let entries = self.entries.read().unwrap();
        Ok(entries
            .iter()
            .filter(|e| {
                q.is_empty()
                    || e.track.title.to_lowercase().contains(&q)
                    || e.track.artists.iter().any(|a| a.to_lowercase().contains(&q))
            })
            .map(|e| e.track.clone())
            .collect())
    }

    async fn get_stream(&self, track: &Track) -> ProviderResult<StreamSource> {
        let entries = self.entries.read().unwrap();
        entries
            .iter()
            .find(|e| e.track.id == track.id)
            .map(|e| StreamSource::LocalFile(e.path.clone()))
            .ok_or_else(|| ProviderError::LocalFile("трек не найден в библиотеке".to_string()))
    }

    /// Лайки для своих файлов будут хранить приложение, не провайдер: пока не поддерживается.
    async fn set_like(&self, _track: &Track, _liked: bool) -> ProviderResult<()> {
        Err(ProviderError::Unsupported)
    }
}