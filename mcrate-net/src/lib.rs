//! mcrate-net: всё, что ходит в сеть и не привязано к одному сервису:
//! общий HTTP-клиент и скачивание звука (прямые ссылки и HLS).
//! Этим пользуются и провайдеры (mcrate-providers), и движок воспроизведения (mcrate-audio).

pub mod http;
pub mod stream;

pub use stream::StreamDownloader; // чтобы писать mcrate_net::StreamDownloader
