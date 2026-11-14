//! mcrate-providers: реализации MusicProvider для конкретных сервисов.

pub mod local;
pub mod soundcloud;

pub use local::LocalProvider; // чтобы писать mcrate_providers::LocalProvider
pub use soundcloud::{SoundCloudProvider, StreamFormat};
pub use mcrate_net::StreamDownloader; // загрузчик переехал в mcrate-net; старый путь продолжает работать
