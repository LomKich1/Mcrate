//! mcrate-providers: реализации MusicProvider для конкретных сервисов.

mod http; // общие мелочи (HTTP-клиент); `mod` без `pub` = виден только внутри крейта
pub mod local;
pub mod soundcloud;
pub mod stream;

pub use local::LocalProvider; // чтобы писать mcrate_providers::LocalProvider
pub use soundcloud::{SoundCloudProvider, StreamFormat};
pub use stream::StreamDownloader;
