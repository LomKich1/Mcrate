//! mcrate-core: общие типы и интерфейс музыкальных сервисов.
//! (`//!` это комментарий-документация для всего файла)

use async_trait::async_trait; // макрос, который разрешает async fn в трейте
use std::path::PathBuf;
use thiserror::Error; // макрос для удобного описания ошибок // PathBuf = путь к файлу (владеющий, как String для текста)
pub mod downloads; // метаданные скачанных онлайн-треков
pub mod history; // история прослушивания на диске
pub mod likes; // понравившиеся треки на диске
pub mod paths; // где лежат файлы приложения
pub mod playlists; // плейлисты на диске
pub mod secrets; // секреты (токены) в хранилище системы
mod track_codec; // запись трека строкой и атомарная запись файла (общее для history/likes/playlists)
pub use downloads::Downloads;
pub use history::{HISTORY_MAX, History};
pub use likes::Likes;
pub use playlists::{Playlist, Playlists};

/// Какой это сервис.
/// `derive` автоматически даёт типу набор способностей:
/// Debug = можно печатать для отладки, Clone/Copy = можно копировать,
/// PartialEq/Eq = можно сравнивать через ==, Hash = можно использовать как ключ в HashMap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Yandex,
    Vk,
    SoundCloud,
    Spotify,
    Local,
}

impl ProviderKind {
    /// Короткое стабильное имя сервиса для имён файлов и сохранённых данных (не для показа пользователю).
    pub fn key(self) -> &'static str {
        match self {
            Self::Yandex => "yandex",
            Self::Vk => "vk",
            Self::SoundCloud => "soundcloud",
            Self::Spotify => "spotify",
            Self::Local => "local",
        }
    }

    /// Обратно: сервис по короткому имени (`None`, если такого нет).
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Yandex, Self::Vk, Self::SoundCloud, Self::Spotify, Self::Local].into_iter().find(|k| k.key() == key)
    }
}

/// Один трек. `pub` значит «виден из других крейтов».
#[derive(Debug, Clone)]
pub struct Track {
    pub id: String,             // id трека внутри сервиса
    pub provider: ProviderKind, // откуда трек
    pub title: String,
    pub artists: Vec<String>, // Vec = список (динамический массив)
    pub duration_secs: u32,   // длительность в секундах, u32 = целое без знака
    /// Адрес картинки обложки (http/https) у онлайн-треков; `None`, если сервис её не дал.
    /// У своих файлов всегда `None`: их обложка лежит внутри файла (или рядом с ним), см. `mcrate_providers::covers`.
    pub cover_url: Option<String>,
}

/// Откуда брать звук. У сервисов это отличается:
/// Yandex отдаёт подписанную ссылку на файл, VK отдаёт HLS (нарезанный на куски поток).
#[derive(Debug, Clone)]
pub enum StreamSource {
    DirectUrl(String),   // прямая ссылка на аудиофайл
    HlsPlaylist(String), // ссылка на HLS-плейлист (.m3u8)
    LocalFile(PathBuf),  // локальный файл, играем прямо с диска
}

/// Ошибки провайдеров. `#[error("...")]` задаёт текст ошибки.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("сервис не подключён или токен недействителен")]
    NotAuthorized,
    #[error("ошибка сети: {0}")] // {0} подставит строку из скобок ниже
    Network(String),
    #[error("неожиданный ответ сервиса (возможно, API изменился): {0}")]
    UnexpectedResponse(String),
    #[error("этот сервис так не умеет")]
    Unsupported,
    #[error("ошибка с локальным файлом: {0}")]
    LocalFile(String),
}

/// Сокращение: вместо Result<T, ProviderError> пишем ProviderResult<T>.
/// Result = «либо успех со значением T, либо ошибка».
pub type ProviderResult<T> = Result<T, ProviderError>;

/// Интерфейс, который должен реализовать каждый сервис (Yandex, VK и т.д.).
/// Аналогия из UE5: Blueprint Interface.
/// `Send + Sync` значит «безопасно использовать из разных потоков».
#[async_trait]
pub trait MusicProvider: Send + Sync {
    /// Какой это сервис.
    fn kind(&self) -> ProviderKind;

    /// Поиск треков по строке. `&self` = «читаем объект, не меняя»; `&str` = ссылка на строку.
    async fn search(&self, query: &str) -> ProviderResult<Vec<Track>>;

    /// Получить источник звука для трека.
    async fn get_stream(&self, track: &Track) -> ProviderResult<StreamSource>;

    /// Поставить или снять лайк.
    async fn set_like(&self, track: &Track, liked: bool) -> ProviderResult<()>;
}
