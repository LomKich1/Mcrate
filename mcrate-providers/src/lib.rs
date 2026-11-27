//! mcrate-providers: реализации MusicProvider для конкретных сервисов.

pub mod covers; // картинки обложек: из сети и из файлов
pub mod local;
pub mod soundcloud;

pub use covers::{CoverImage, CoverSize, CoverStore, cover_key};
pub use local::LocalProvider; // чтобы писать mcrate_providers::LocalProvider
pub use mcrate_net::StreamDownloader;
pub use soundcloud::{SoundCloudProvider, StreamFormat}; // загрузчик переехал в mcrate-net; старый путь продолжает работать
