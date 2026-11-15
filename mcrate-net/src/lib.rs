//! mcrate-net: всё, что ходит в сеть и не привязано к одному сервису:
//! общий HTTP-клиент и скачивание звука (прямые ссылки и HLS, в том числе
//! с шифрованием AES-128 и кусками в MPEG-TS).
//! Этим пользуются и провайдеры (mcrate-providers), и движок воспроизведения (mcrate-audio).

pub mod http;
pub mod stream;
pub mod ts; // разбор MPEG-TS (звук внутри HLS-кусков)

pub use stream::StreamDownloader; // чтобы писать mcrate_net::StreamDownloader
