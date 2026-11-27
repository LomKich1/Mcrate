//! Обложки треков: достаёт картинку (из сети или из самого аудиофайла), разбирает её и отдаёт готовые пиксели.
//!
//! Откуда берётся картинка:
//! - онлайн-трек: по `Track.cover_url`. Скачанные байты кладутся в папку кэша (`%APPDATA%\Mcrate\covers`),
//!   поэтому повторный показ идёт с диска, без сети;
//! - свой файл: из тегов файла (встроенная обложка), а если её нет, из картинки рядом (`cover.jpg`, `folder.png`...).
//!
//! Что получает вызывающий: квадрат RGBA нужного размера (`CoverSize`). Неквадратная картинка обрезается
//! по центру, маленькая не растягивается. Два размера нужны затем, что мелкую обложку (строка списка, 40-50 px)
//! из картинки 500 px видно плохо: после грубого уменьшения на лету края мерцают.
//!
//! Тяжёлое (чтение тегов, разбор картинки) идёт в `spawn_blocking`, чтобы не занимать потоки tokio, на которых ходит сеть.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use image::imageops::{self, FilterType};
use image::{DynamicImage, GenericImageView, ImageReader, Limits};
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use mcrate_core::{ProviderError, ProviderKind, ProviderResult, Track};
use tokio::sync::Semaphore;

/// Сторона маленькой обложки в пикселях: строки списков, плитки коллажа, карточки полок.
pub const SMALL_SIDE: u32 = 160;
/// Сторона большой обложки в пикселях: экран «Сейчас играет».
pub const LARGE_SIDE: u32 = 512;
/// Сколько обложек качается одновременно: поиск возвращает до 30 треков, а десятки соединений разом ни к чему.
const MAX_PARALLEL_DOWNLOADS: usize = 6;
/// Больше этого размера (в байтах) картинку не берём: обложка весит десятки КБ, а гигантский файл это ошибка или подвох.
const MAX_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024;
/// Картинку шире или выше этого (в пикселях) не разбираем: защита от «бомбы» (маленький файл, огромная распаковка).
const MAX_DECODE_SIDE: u32 = 8192;
/// Имена картинок рядом с аудиофайлом (без расширения, в нижнем регистре), в порядке предпочтения.
const FOLDER_COVER_NAMES: [&str; 5] = ["cover", "folder", "front", "album", "albumart"];
/// Расширения таких картинок (в нижнем регистре).
const FOLDER_COVER_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

/// Какого размера нужна обложка.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverSize {
    Small,
    Large,
}

impl CoverSize {
    /// Сторона квадрата в пикселях.
    pub fn side(self) -> u32 {
        match self {
            Self::Small => SMALL_SIDE,
            Self::Large => LARGE_SIDE,
        }
    }

    /// Какой размер взять для места, которому на экране нужно `pixels` физических пикселей по стороне.
    pub fn for_pixels(pixels: f32) -> Self {
        if pixels <= SMALL_SIDE as f32 { Self::Small } else { Self::Large }
    }
}

/// Готовая обложка: квадрат из пикселей RGBA (по 4 байта на пиксель, цвет не умножен на прозрачность).
#[derive(Debug, Clone, PartialEq)]
pub struct CoverImage {
    /// Ширина и высота в пикселях (равны: обложка всегда квадрат).
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// Ключ обложки: у разных треков с одной картинкой (альбом) он одинаковый, и картинка грузится один раз.
/// `None`, если обложки у трека взять неоткуда (онлайн-трек без адреса).
pub fn cover_key(track: &Track) -> Option<String> {
    match track.provider {
        ProviderKind::Local => Some(format!("file:{}", track.id)), // id своего трека это путь к файлу
        _ => track.cover_url.as_deref().filter(|url| !url.is_empty()).map(|url| format!("url:{url}")),
    }
}

/// Откуда взялись байты картинки (от этого зависит, что с кэшем делать после разбора).
enum Origin {
    /// Свой файл: кэша нет.
    Local,
    /// Из файла кэша (путь внутри): если там мусор, файл выбрасывается.
    Cache(PathBuf),
    /// Только что скачано; путь внутри, куда сохранить после успешного разбора (`None`: кэша на диске нет).
    Fresh(Option<PathBuf>),
}

/// Загрузчик обложек. Дёшево клонируется через `Arc` у владельца; сам ничего не помнит между вызовами, кроме папки кэша.
pub struct CoverStore {
    client: reqwest::Client,
    /// Папка кэша скачанных картинок; `None` = без кэша на диске (тесты, нет `%APPDATA%`).
    dir: Option<PathBuf>,
    downloads: Arc<Semaphore>,
}

impl CoverStore {
    /// Создаёт загрузчик. Вызывай внутри среды tokio, как и провайдеров: клиент держит соединения там, где создан.
    pub fn new(dir: Option<PathBuf>) -> ProviderResult<Self> {
        Ok(Self { client: mcrate_net::http::build_client()?, dir, downloads: Arc::new(Semaphore::new(MAX_PARALLEL_DOWNLOADS)) })
    }

    /// Обложка трека в нужном размере. Ошибка значит «обложки нет или не вышло»: вызывающий оставляет заглушку.
    pub async fn load(&self, track: &Track, size: CoverSize) -> ProviderResult<CoverImage> {
        let (bytes, origin) = match track.provider {
            ProviderKind::Local => (read_local_cover(PathBuf::from(&track.id)).await?, Origin::Local),
            _ => {
                let url = track.cover_url.as_deref().filter(|u| u.starts_with("https://") || u.starts_with("http://"));
                self.remote_bytes(url.ok_or(ProviderError::Unsupported)?).await?
            }
        };
        let side = size.side();
        let joined = tokio::task::spawn_blocking(move || (decode_cover(&bytes, side), bytes)).await;
        let (decoded, bytes) = joined.map_err(|e| ProviderError::UnexpectedResponse(format!("разбор обложки прервался: {e}")))?;
        match (&decoded, origin) {
            // Скачано и разобралось: теперь можно и в кэш (раньше нельзя, иначе туда попадала бы страница-заглушка вместо картинки).
            (Ok(_), Origin::Fresh(Some(file))) => {
                let _ = tokio::task::spawn_blocking(move || store_atomic(&file, &bytes)).await; // не записалось: не страшно, картинка уже есть
            }
            // В кэше лежит мусор: выбрасываем, пусть в следующий раз скачается заново.
            (Err(_), Origin::Cache(file)) => {
                let _ = std::fs::remove_file(file);
            }
            _ => {}
        }
        decoded
    }

    /// Байты картинки по адресу: из кэша на диске, а если там нет, из сети. Кэш сам здесь не пишется: см. `load`.
    async fn remote_bytes(&self, url: &str) -> ProviderResult<(Vec<u8>, Origin)> {
        let file = self.dir.as_ref().map(|dir| dir.join(cache_file_name(url)));
        if let Some(file) = &file {
            let path = file.clone();
            if let Ok(Ok(bytes)) = tokio::task::spawn_blocking(move || std::fs::read(path)).await
                && !bytes.is_empty()
            {
                return Ok((bytes, Origin::Cache(file.clone())));
            }
        }
        Ok((self.download(url).await?, Origin::Fresh(file)))
    }

    /// Скачивает картинку. Размер ограничен: см. `MAX_DOWNLOAD_BYTES`.
    async fn download(&self, url: &str) -> ProviderResult<Vec<u8>> {
        let _slot = self.downloads.acquire().await.map_err(|_| ProviderError::Network("загрузчик обложек остановлен".into()))?;
        let response = self.client.get(url).send().await.map_err(mcrate_net::http::net_err)?;
        if !response.status().is_success() {
            return Err(ProviderError::Network(format!("обложка: сервер ответил {}", response.status())));
        }
        if response.content_length().is_some_and(|len| len > MAX_DOWNLOAD_BYTES) {
            return Err(ProviderError::UnexpectedResponse("обложка слишком большая".into()));
        }
        let bytes = response.bytes().await.map_err(mcrate_net::http::net_err)?;
        if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
            return Err(ProviderError::UnexpectedResponse("обложка слишком большая".into()));
        }
        Ok(bytes.to_vec())
    }

    /// Оставляет в папке кэша не больше `max_files` картинок, удаляя самые старые.
    /// Без этого кэш рос бы бесконечно: каждый поиск приносит десятки новых обложек.
    pub fn prune_cache(&self, max_files: usize) {
        let Some(dir) = &self.dir else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                meta.is_file().then(|| (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
            })
            .collect();
        if files.len() <= max_files {
            return;
        }
        files.sort_by_key(|(modified, _)| *modified); // старые первыми
        let extra = files.len() - max_files;
        for (_, path) in files.into_iter().take(extra) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Имя файла в кэше для адреса: хэш FNV-1a (64 бита), записанный шестнадцатерично.
/// Свой хэш, а не `DefaultHasher`: тот не обещает давать одно и то же в разных версиях Rust, и кэш бы «терялся» после обновления.
fn cache_file_name(url: &str) -> String {
    let hash = url.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{hash:016x}.img")
}

/// Запись во временный файл и переименование: оборванная запись не оставит в кэше половину картинки.
fn store_atomic(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file)
}

/// Разбирает байты картинки: обрезает до квадрата по центру и уменьшает до `side` (больше исходника не растягивает).
fn decode_cover(bytes: &[u8], side: u32) -> ProviderResult<CoverImage> {
    let bad = |e: &dyn std::fmt::Display| ProviderError::UnexpectedResponse(format!("не удалось разобрать обложку: {e}"));
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| bad(&e))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| bad(&e))?;
    let square = crop_square(image);
    let target = side.min(square.width());
    let resized = if target < square.width() { imageops::resize(&square, target, target, FilterType::Lanczos3) } else { square.to_rgba8() };
    Ok(CoverImage { size: [resized.width() as usize, resized.height() as usize], rgba: resized.into_raw() })
}

/// Вырезает из картинки квадрат по центру (как `object-fit: cover` в вебе).
fn crop_square(image: DynamicImage) -> DynamicImage {
    let (w, h) = image.dimensions();
    let side = w.min(h);
    image.crop_imm((w - side) / 2, (h - side) / 2, side, side)
}

/// Байты обложки своего файла: встроенная картинка из тегов, иначе картинка рядом с файлом.
async fn read_local_cover(path: PathBuf) -> ProviderResult<Vec<u8>> {
    let found = tokio::task::spawn_blocking(move || embedded_picture(&path).or_else(|| folder_picture(&path)))
        .await
        .map_err(|e| ProviderError::LocalFile(format!("чтение обложки прервалось: {e}")))?;
    found.ok_or(ProviderError::Unsupported)
}

/// Картинка, вшитая в теги файла. Предпочитается «лицевая обложка», иначе любая первая.
fn embedded_picture(path: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(path).ok()?;
    let pictures = || tagged.tags().iter().flat_map(|tag| tag.pictures());
    let best = pictures().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pictures().next())?;
    Some(best.data().to_vec())
}

/// Картинка в папке рядом с файлом: `cover.jpg`, `folder.png` и подобные (регистр имени не важен).
fn folder_picture(audio: &Path) -> Option<Vec<u8>> {
    let dir = audio.parent()?;
    let mut candidates: Vec<(usize, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let stem = path.file_stem()?.to_str()?.to_lowercase();
            let ext = path.extension()?.to_str()?.to_lowercase();
            let rank = FOLDER_COVER_NAMES.iter().position(|name| *name == stem)?;
            FOLDER_COVER_EXTENSIONS.contains(&ext.as_str()).then_some((rank, path))
        })
        .collect();
    candidates.sort(); // лучшее имя первым
    let (_, best) = candidates.into_iter().next()?;
    let meta = std::fs::metadata(&best).ok()?;
    (meta.len() <= MAX_DOWNLOAD_BYTES).then(|| std::fs::read(best).ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    /// PNG `w` на `h`: левая половина красная, правая синяя.
    fn two_color_png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, _| if x < w / 2 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) });
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).unwrap();
        out
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-covers-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn track(provider: ProviderKind, id: &str, cover_url: Option<&str>) -> Track {
        Track { id: id.into(), provider, title: "т".into(), artists: vec![], duration_secs: 1, cover_url: cover_url.map(str::to_string) }
    }

    #[test]
    fn key_depends_on_source() {
        assert_eq!(cover_key(&track(ProviderKind::Local, "C:\\a.mp3", None)).as_deref(), Some("file:C:\\a.mp3"));
        assert_eq!(cover_key(&track(ProviderKind::SoundCloud, "1", Some("https://x/y.jpg"))).as_deref(), Some("url:https://x/y.jpg"));
        assert_eq!(cover_key(&track(ProviderKind::SoundCloud, "1", None)), None);
        assert_eq!(cover_key(&track(ProviderKind::SoundCloud, "1", Some(""))), None, "пустой адрес это отсутствие обложки");
        // Два трека одного альбома делят ключ, а значит и одну загрузку.
        let a = track(ProviderKind::SoundCloud, "1", Some("https://x/album.jpg"));
        let b = track(ProviderKind::SoundCloud, "2", Some("https://x/album.jpg"));
        assert_eq!(cover_key(&a), cover_key(&b));
    }

    #[test]
    fn size_is_picked_by_pixels() {
        assert_eq!(CoverSize::for_pixels(48.0), CoverSize::Small);
        assert_eq!(CoverSize::for_pixels(SMALL_SIDE as f32), CoverSize::Small);
        assert_eq!(CoverSize::for_pixels(SMALL_SIDE as f32 + 1.0), CoverSize::Large);
    }

    #[test]
    fn cache_name_is_stable_and_distinct() {
        // Значение зафиксировано: если оно изменится, весь кэш пользователей станет недоступным.
        assert_eq!(cache_file_name("https://a/b.jpg"), cache_file_name("https://a/b.jpg"));
        assert_ne!(cache_file_name("https://a/b.jpg"), cache_file_name("https://a/c.jpg"));
        assert_eq!(cache_file_name(""), "cbf29ce484222325.img");
    }

    #[test]
    fn wide_picture_is_cropped_to_centered_square() {
        // 300 на 100, красное слева и синее справа с границей на x = 150. Берётся центральный квадрат 100 на 100
        // (x от 100 до 200): граница оказывается в его середине, края остаются красным и синим.
        let cover = decode_cover(&two_color_png(300, 100), SMALL_SIDE).unwrap();
        assert_eq!(cover.size, [100, 100], "маленькую картинку не растягиваем");
        let px = |x: usize, y: usize| &cover.rgba[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
        assert_eq!(px(10, 50), [255, 0, 0, 255]);
        assert_eq!(px(90, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn big_picture_is_scaled_down_to_requested_side() {
        let bytes = two_color_png(400, 400);
        assert_eq!(decode_cover(&bytes, 160).unwrap().size, [160, 160]);
        let big = decode_cover(&bytes, LARGE_SIDE).unwrap();
        assert_eq!(big.size, [400, 400]);
        assert_eq!(big.rgba.len(), 400 * 400 * 4);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_cover("это не картинка".as_bytes(), SMALL_SIDE).is_err());
        assert!(decode_cover(&[], SMALL_SIDE).is_err());
    }

    #[test]
    fn folder_picture_prefers_better_name_and_ignores_case() {
        let dir = temp_dir("folder");
        let song = dir.join("song.mp3");
        std::fs::write(&song, b"not audio").unwrap();
        assert!(folder_picture(&song).is_none(), "картинок рядом нет");
        std::fs::write(dir.join("ALBUM.png"), b"album").unwrap();
        std::fs::write(dir.join("Cover.JPG"), b"cover").unwrap();
        std::fs::write(dir.join("notes.txt"), b"text").unwrap();
        assert_eq!(folder_picture(&song).unwrap(), b"cover", "cover лучше album, регистр не важен");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Минимальный настоящий MP3: десять одинаковых кадров MPEG-1 Layer III (128 кбит/с, 44,1 кГц, по 417 байт, тишина).
    /// Нужен, чтобы lofty согласился записать в файл теги.
    fn tiny_mp3(path: &Path) {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        std::fs::write(path, frame.repeat(10)).unwrap();
    }

    /// Записывает в MP3 тег ID3v2 с картинками: каждая задана типом и содержимым.
    fn tag_with_pictures(path: &Path, pictures: &[(PictureType, Vec<u8>)]) {
        use lofty::config::WriteOptions;
        use lofty::picture::{MimeType, Picture};
        use lofty::tag::{Tag, TagExt, TagType};
        let mut tag = Tag::new(TagType::Id3v2);
        for (kind, data) in pictures {
            tag.push_picture(Picture::unchecked(data.clone()).pic_type(*kind).mime_type(MimeType::Png).build());
        }
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    #[test]
    fn embedded_picture_prefers_front_cover() {
        let dir = temp_dir("embedded");
        let song = dir.join("song.mp3");
        tiny_mp3(&song);
        assert!(embedded_picture(&song).is_none(), "в файле без тега картинки нет");
        tag_with_pictures(&song, &[(PictureType::Artist, two_color_png(20, 20)), (PictureType::CoverFront, two_color_png(30, 30))]);
        let found = embedded_picture(&song).expect("картинка из тега должна найтись");
        assert_eq!(decode_cover(&found, LARGE_SIDE).unwrap().size, [30, 30], "взята лицевая обложка, а не фото исполнителя");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn embedded_picture_wins_over_folder_picture() {
        let dir = temp_dir("embedded-first");
        let song = dir.join("song.mp3");
        tiny_mp3(&song);
        tag_with_pictures(&song, &[(PictureType::CoverFront, two_color_png(40, 40))]);
        std::fs::write(dir.join("cover.png"), two_color_png(60, 60)).unwrap();
        let store = CoverStore::new(None).unwrap();
        let t = track(ProviderKind::Local, song.to_str().unwrap(), None);
        assert_eq!(store.load(&t, CoverSize::Large).await.unwrap().size, [40, 40]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prune_removes_oldest_files_only() {
        let dir = temp_dir("prune");
        let store = CoverStore { client: reqwest::Client::new(), dir: Some(dir.clone()), downloads: Arc::new(Semaphore::new(1)) };
        for i in 0..5 {
            let file = dir.join(format!("{i}.img"));
            std::fs::write(&file, b"x").unwrap();
            let age = std::time::Duration::from_secs(1000 - i * 100); // 0.img самый старый
            std::fs::File::options().write(true).open(&file).unwrap().set_modified(SystemTime::now() - age).unwrap();
        }
        store.prune_cache(3);
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["2.img", "3.img", "4.img"]);
        store.prune_cache(10); // меньше лимита: ничего не трогаем
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn local_track_gets_cover_from_neighbour_picture() {
        let dir = temp_dir("local");
        let song = dir.join("song.mp3");
        std::fs::write(&song, b"not audio").unwrap(); // теги не читаются: берётся картинка из папки
        std::fs::write(dir.join("folder.png"), two_color_png(200, 200)).unwrap();
        let store = CoverStore::new(None).unwrap();
        let t = track(ProviderKind::Local, song.to_str().unwrap(), None);
        let cover = store.load(&t, CoverSize::Small).await.unwrap();
        assert_eq!(cover.size, [160, 160]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn local_track_without_any_picture_is_unsupported() {
        let dir = temp_dir("nopic");
        let song = dir.join("song.mp3");
        std::fs::write(&song, b"not audio").unwrap();
        let store = CoverStore::new(None).unwrap();
        let t = track(ProviderKind::Local, song.to_str().unwrap(), None);
        assert!(matches!(store.load(&t, CoverSize::Small).await, Err(ProviderError::Unsupported)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn remote_track_reads_from_disk_cache_without_network() {
        let dir = temp_dir("cache");
        let url = "https://cover.invalid/a-t500x500.png"; // такого сервера нет: если бы код пошёл в сеть, тест упал бы
        std::fs::write(dir.join(cache_file_name(url)), two_color_png(300, 300)).unwrap();
        let store = CoverStore::new(Some(dir.clone())).unwrap();
        let cover = store.load(&track(ProviderKind::SoundCloud, "1", Some(url)), CoverSize::Small).await.unwrap();
        assert_eq!(cover.size, [160, 160]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Крошечный HTTP-сервер на этом компьютере: отвечает на ровно один запрос заданным ответом и закрывается.
    /// Возвращает адрес, по которому его можно позвать.
    fn serve_once(status: &'static str, content_type: &'static str, body: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let head =
                format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{addr}/cover-t500x500.png")
    }

    #[tokio::test]
    async fn download_is_cached_and_second_load_needs_no_server() {
        let dir = temp_dir("download");
        let url = serve_once("200 OK", "image/png", two_color_png(300, 300));
        let store = CoverStore::new(Some(dir.clone())).unwrap();
        let t = track(ProviderKind::SoundCloud, "1", Some(&url));
        assert_eq!(store.load(&t, CoverSize::Small).await.unwrap().size, [160, 160]);
        assert!(dir.join(cache_file_name(&url)).exists(), "скачанная картинка должна лечь в кэш");
        // Сервер отвечал один раз и закрыт: второй показ возможен только из кэша.
        assert_eq!(store.load(&t, CoverSize::Large).await.unwrap().size, [300, 300]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn server_error_is_an_error_and_nothing_is_cached() {
        let dir = temp_dir("http404");
        let url = serve_once("404 Not Found", "text/plain", b"no".to_vec());
        let store = CoverStore::new(Some(dir.clone())).unwrap();
        let t = track(ProviderKind::SoundCloud, "1", Some(&url));
        assert!(matches!(store.load(&t, CoverSize::Small).await, Err(ProviderError::Network(_))));
        assert!(!dir.join(cache_file_name(&url)).exists(), "ответ с ошибкой в кэш попадать не должен");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn non_image_answer_is_not_kept_in_cache_as_good() {
        // Сервер вернул 200, но не картинку (например, страницу-заглушку): ошибка, а в кэше мусор не остаётся.
        let dir = temp_dir("notimage");
        let url = serve_once("200 OK", "text/html", b"<html>captcha</html>".to_vec());
        let store = CoverStore::new(Some(dir.clone())).unwrap();
        let t = track(ProviderKind::SoundCloud, "1", Some(&url));
        assert!(store.load(&t, CoverSize::Small).await.is_err());
        assert!(!dir.join(cache_file_name(&url)).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn broken_cache_file_is_dropped() {
        let dir = temp_dir("broken");
        let url = "https://cover.invalid/broken.png";
        let file = dir.join(cache_file_name(url));
        std::fs::write(&file, b"garbage").unwrap();
        let store = CoverStore::new(Some(dir.clone())).unwrap();
        assert!(store.load(&track(ProviderKind::SoundCloud, "1", Some(url)), CoverSize::Small).await.is_err());
        assert!(!file.exists(), "битый файл кэша должен удаляться, чтобы картинка скачалась заново");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn online_track_without_url_is_unsupported() {
        let store = CoverStore::new(None).unwrap();
        let t = track(ProviderKind::SoundCloud, "1", None);
        assert!(matches!(store.load(&t, CoverSize::Small).await, Err(ProviderError::Unsupported)));
        let bad = track(ProviderKind::SoundCloud, "1", Some("file:///etc/passwd"));
        assert!(matches!(store.load(&bad, CoverSize::Small).await, Err(ProviderError::Unsupported)), "только http(s)");
    }
}
