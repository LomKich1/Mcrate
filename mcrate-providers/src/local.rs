//! Провайдер своих файлов: читает теги (название, исполнитель) и длину из аудиофайлов.

use std::path::{Path, PathBuf};
use std::sync::RwLock; // замок «много читают или один пишет»

use async_trait::async_trait;
use lofty::file::{AudioFile, TaggedFileExt}; // AudioFile даёт properties() (длина), TaggedFileExt даёт primary_tag() и first_tag()
use lofty::tag::{Accessor, ItemKey}; // Accessor даёт title(); ItemKey = «какое поле тега» (например, TrackArtist)
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
    // Куда сохранять список путей между запусками. None = не сохраняем (так работает new()).
    store_path: Option<PathBuf>,
}

impl LocalProvider {
    /// Библиотека только в памяти: при закрытии программы пропадает (для тестов и примеров).
    pub fn new() -> Self {
        Self { entries: RwLock::new(Vec::new()), store_path: None }
    }

    /// Библиотека с сохранением: список путей лежит в файле `store_path`.
    /// Сам файл не читается, чтобы загрузить сохранённое, вызови после этого `load()`.
    pub fn with_storage(store_path: PathBuf) -> Self {
        Self { entries: RwLock::new(Vec::new()), store_path: Some(store_path) }
    }

    /// Загрузить сохранённые пути и заново прочитать теги каждого файла.
    /// Возвращает пути, которые не удалось загрузить (файл удалён, перенесён или не читается).
    /// Первый запуск (файла сохранения ещё нет) это не ошибка: вернётся пустой список.
    /// Сам файл сохранения тут не меняется: недоступные пути сотрутся из него при следующем сохранении.
    pub fn load(&self) -> ProviderResult<Vec<PathBuf>> {
        let Some(store) = &self.store_path else {
            return Ok(Vec::new()); // сохранение не включено, загружать нечего
        };
        let text = match std::fs::read_to_string(store) {
            Ok(t) => t,
            // NotFound = файла нет, значит это первый запуск
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ProviderError::LocalFile(format!("{}: {e}", store.display())));
            }
        };

        let mut unavailable = Vec::new();
        for line in text.lines() {
            // lines() режет текст по строкам; trim убирает пробелы и невидимый символ конца строки
            let line = line.trim();
            if line.is_empty() {
                continue; // пустые строки пропускаем
            }
            let path = PathBuf::from(line);
            if self.add_file_inner(&path).is_err() {
                unavailable.push(path);
            }
        }
        Ok(unavailable)
    }

    /// Записать список путей в файл сохранения: одна строка = один путь.
    /// Пишем сначала во временный файл, потом переименовываем: если программа упадёт
    /// посреди записи, старый файл останется целым (переименование происходит разом).
    fn save(&self) -> std::io::Result<()> {
        let Some(store) = &self.store_path else {
            return Ok(()); // сохранение не включено
        };
        let text = self
            .entries
            .read()
            .unwrap()
            .iter()
            .filter_map(|e| e.path.to_str()) // путь в текст; пути не в UTF-8 (в Windows почти не бывают) пропускаем
            .collect::<Vec<&str>>()
            .join("\n");
        if let Some(dir) = store.parent() {
            std::fs::create_dir_all(dir)?; // создаём папку сохранения, если её ещё нет
        }
        let tmp = store.with_extension("tmp"); // library.txt -> library.tmp
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, store)?;
        Ok(())
    }

    /// Сохранить и, если не вышло, только сообщить в консоль: трек уже добавлен в память,
    /// и ронять из-за этого добавление было бы неправильно.
    fn save_or_log(&self) {
        if let Err(e) = self.save() {
            eprintln!("не удалось сохранить библиотеку: {e}");
        }
    }

    /// Добавить один файл в библиотеку и сохранить список. Тот же файл дважды не добавляется.
    pub fn add_file(&self, path: &Path) -> ProviderResult<Track> {
        let track = self.add_file_inner(path)?;
        self.save_or_log();
        Ok(track)
    }

    /// Добавление без сохранения. Нужно, чтобы папка из 1000 файлов не писала файл 1000 раз.
    fn add_file_inner(&self, path: &Path) -> ProviderResult<Track> {
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

        // Название из тега; если тега нет или он пустой, берём имя файла без расширения
        let title = tag
            .and_then(|t| t.title().map(|s| s.trim().to_string())) // trim убирает пробелы по краям
            .filter(|s| !s.is_empty()) // пустое название считаем отсутствующим
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Без названия".to_string())
            });

        // artist() отдаёт только первого исполнителя, а get_strings(TrackArtist) всех (у дуэтов их несколько)
        let artists: Vec<String> = tag
            .map(|t| {
                t.get_strings(ItemKey::TrackArtist)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(); // нет тега = пустой список

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
            if is_audio && self.add_file_inner(&path).is_ok() {
                added += 1;
            }
        }
        if added > 0 {
            self.save_or_log(); // один раз на всю папку
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

#[cfg(test)] // этот блок собирается только для `cargo test`
mod tests {
    use super::*; // подтягиваем всё из файла выше (LocalProvider, MusicProvider и т.д.)
    use lofty::config::WriteOptions;
    use lofty::tag::{ItemKey, ItemValue, Tag, TagExt, TagItem, TagType}; // TagExt даёт save_to_path

    /// Чистая временная папка для теста. Имя содержит номер процесса, чтобы тесты не мешали друг другу.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // если осталась с прошлого раза, стираем (ошибку игнорируем)
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Создаёт настоящий, но крошечный WAV: 1 секунда тишины. Тегов в нём нет.
    /// WAV = 44 байта заголовка + сами отсчёты звука.
    fn write_test_wav(path: &Path) {
        let sample_rate: u32 = 8000; // отсчётов в секунду
        let data_len: u32 = sample_rate * 2; // 1 секунда, каждый отсчёт 2 байта (16 бит)
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes()); // размер файла после этих 8 байт
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt "); // блок с описанием формата
        b.extend_from_slice(&16u32.to_le_bytes()); // размер этого блока
        b.extend_from_slice(&1u16.to_le_bytes()); // 1 = обычный PCM (без сжатия)
        b.extend_from_slice(&1u16.to_le_bytes()); // 1 канал (моно)
        b.extend_from_slice(&sample_rate.to_le_bytes());
        b.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // байт в секунду
        b.extend_from_slice(&2u16.to_le_bytes()); // байт на один отсчёт всех каналов
        b.extend_from_slice(&16u16.to_le_bytes()); // бит на отсчёт
        b.extend_from_slice(b"data"); // блок со звуком
        b.extend_from_slice(&data_len.to_le_bytes());
        b.resize(b.len() + data_len as usize, 0); // тишина: нули
        std::fs::write(path, b).unwrap();
    }

    /// Записывает теги прямо в файл (формат ID3v2: его понимают и WAV, и mp3).
    /// None вместо названия = тега названия в файле нет совсем.
    fn write_tags(path: &Path, title: Option<&str>, artists: &[&str]) {
        let mut tag = Tag::new(TagType::Id3v2);
        if let Some(t) = title {
            tag.set_title(t.to_string()); // set_title даёт трейт Accessor (он уже импортирован выше)
        }
        for a in artists {
            // push добавляет ещё одну запись с тем же ключом, не затирая прежние (в отличие от insert)
            tag.push(TagItem::new(ItemKey::TrackArtist, ItemValue::Text(a.to_string())));
        }
        // WriteOptions::default() = настройки записи по умолчанию
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    // #[tokio::test] запускает async-тест (search у нас async)
    #[tokio::test]
    async fn library_survives_restart() {
        let dir = temp_dir("restart");
        let wav = dir.join("song.wav");
        write_test_wav(&wav);
        let store = dir.join("data").join("library.txt"); // папки data ещё нет: проверим, что она создастся

        // «Первый запуск»: добавляем файл
        let first = LocalProvider::with_storage(store.clone());
        assert!(first.load().unwrap().is_empty()); // файла сохранения ещё нет, это не ошибка
        first.add_file(&wav).unwrap();

        // «Второй запуск»: новый провайдер читает то, что сохранил первый
        let second = LocalProvider::with_storage(store);
        assert!(second.load().unwrap().is_empty()); // всё нашлось
        let tracks = second.search("").await.unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "song"); // тегов нет, поэтому название = имя файла
        assert_eq!(tracks[0].duration_secs, 1);

        let _ = std::fs::remove_dir_all(&dir); // прибираем за собой
    }

    #[tokio::test]
    async fn missing_files_are_reported_and_skipped() {
        let dir = temp_dir("missing");
        let wav = dir.join("gone.wav");
        write_test_wav(&wav);
        let store = dir.join("library.txt");

        let first = LocalProvider::with_storage(store.clone());
        first.add_file(&wav).unwrap();
        std::fs::remove_file(&wav).unwrap(); // файл «пропал» между запусками

        let second = LocalProvider::with_storage(store);
        let unavailable = second.load().unwrap();
        assert_eq!(unavailable, vec![wav]); // пропавший путь вернулся списком
        assert!(second.search("").await.unwrap().is_empty()); // а в библиотеку не попал

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn folder_is_added_and_duplicates_ignored() {
        let dir = temp_dir("folder");
        write_test_wav(&dir.join("a.wav"));
        write_test_wav(&dir.join("b.wav"));
        let store = std::env::temp_dir().join(format!("mcrate-test-folder-store-{}.txt", std::process::id()));

        let first = LocalProvider::with_storage(store.clone());
        assert_eq!(first.add_folder(&dir).unwrap(), 2);
        first.add_file(&dir.join("a.wav")).unwrap(); // повторное добавление того же файла

        let second = LocalProvider::with_storage(store.clone());
        assert!(second.load().unwrap().is_empty());
        assert_eq!(second.search("").await.unwrap().len(), 2); // дубля нет

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&store);
    }

    #[tokio::test]
    async fn tags_are_read() {
        let dir = temp_dir("tags");
        let wav = dir.join("file_name.wav");
        write_test_wav(&wav);
        write_tags(&wav, Some("Test Title"), &["Test Artist"]);

        let p = LocalProvider::new();
        let track = p.add_file(&wav).unwrap();
        assert_eq!(track.title, "Test Title"); // название из тега, а не из имени файла
        assert_eq!(track.artists, vec!["Test Artist".to_string()]);
        assert_eq!(track.duration_secs, 1);

        // поиск находит и по названию, и по исполнителю, без учёта регистра
        assert_eq!(p.search("test title").await.unwrap().len(), 1);
        assert_eq!(p.search("ARTIST").await.unwrap().len(), 1);
        assert!(p.search("нет такого").await.unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cyrillic_tags_and_search() {
        let dir = temp_dir("cyrillic");
        let wav = dir.join("track01.wav");
        write_test_wav(&wav);
        write_tags(&wav, Some("Группа крови"), &["Кино"]);

        let p = LocalProvider::new();
        let track = p.add_file(&wav).unwrap();
        assert_eq!(track.title, "Группа крови");
        assert_eq!(track.artists, vec!["Кино".to_string()]);
        // to_lowercase понимает кириллицу: заглавные и строчные считаются одинаковыми
        assert_eq!(p.search("кино").await.unwrap().len(), 1);
        assert_eq!(p.search("ГРУППА").await.unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn missing_title_falls_back_to_file_name() {
        let dir = temp_dir("notitle");
        let wav = dir.join("My Song.wav");
        write_test_wav(&wav);
        write_tags(&wav, None, &["Someone"]); // исполнитель есть, названия нет

        let p = LocalProvider::new();
        let track = p.add_file(&wav).unwrap();
        assert_eq!(track.title, "My Song"); // имя файла без расширения
        assert_eq!(track.artists, vec!["Someone".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn empty_title_falls_back_to_file_name() {
        let dir = temp_dir("emptytitle");
        let wav = dir.join("Other Song.wav");
        write_test_wav(&wav);
        write_tags(&wav, Some(""), &["Someone"]); // тег названия есть, но пустой

        let p = LocalProvider::new();
        let track = p.add_file(&wav).unwrap();
        assert_eq!(track.title, "Other Song");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn several_artists_are_all_kept() {
        let dir = temp_dir("artists");
        let wav = dir.join("duet.wav");
        write_test_wav(&wav);
        write_tags(&wav, Some("Duet"), &["First Artist", "Second Artist"]);

        let p = LocalProvider::new();
        let track = p.add_file(&wav).unwrap();
        assert_eq!(track.artists, vec!["First Artist".to_string(), "Second Artist".to_string()]);
        // и второго исполнителя тоже можно найти поиском
        assert_eq!(p.search("second").await.unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
