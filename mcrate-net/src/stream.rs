//! Загрузка звука по ссылке. Умеет прямые ссылки на файл и HLS-плейлисты (звук, нарезанный на куски).
//!
//! Идея: скачать трек целиком в память или в файл, а дальше его играет обычный плеер.
//! Минус: при длинных треках (часовые миксы) перед стартом придётся подождать.
//! Плюс: простая перемотка и никаких «докачек» на ходу.

use std::path::Path;

use mcrate_core::{ProviderError, ProviderResult, StreamSource};
use reqwest::Url; // Url = разобранный веб-адрес

use crate::http::{build_client, net_err, short};

/// Верхний предел размера одного трека, чтобы по ошибке не забить всю память (300 МБ).
const MAX_BYTES: usize = 300 * 1024 * 1024;

/// «План» HLS-загрузки: что скачать и в каком порядке.
#[derive(Debug, PartialEq)]
pub struct HlsPlan {
    /// Общий «заголовок» (EXT-X-MAP), нужен для fMP4. Его кладём в начало файла.
    pub init: Option<Url>,
    /// Куски звука по порядку.
    pub segments: Vec<Url>,
}

/// Достаёт значение атрибута вида NAME="значение" из строки атрибутов HLS.
fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\""); // например URI="
    let start = attrs.find(&key)? + key.len(); // `?` = если не нашли, сразу вернуть None
    let end = attrs[start..].find('"')?;
    Some(attrs[start..start + end].to_string())
}

/// Разбирает текст HLS-плейлиста. `base` = адрес самого плейлиста: от него считаются
/// относительные ссылки. Всё, что мы пока не умеем (шифрование, эфиры), сразу отклоняем с понятным текстом.
pub fn parse_hls(text: &str, base: &Url) -> Result<HlsPlan, String> {
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err("это не HLS-плейлист (нет #EXTM3U в начале)".to_string());
    }
    if text.contains("#EXT-X-STREAM-INF") {
        return Err("это мастер-плейлист (выбор качества), а нужен плейлист с самими кусками".to_string());
    }
    if !text.contains("#EXT-X-ENDLIST") {
        return Err("плейлист без #EXT-X-ENDLIST (живой эфир), такое не поддержано".to_string());
    }

    let mut init = None;
    let mut segments = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            // METHOD=NONE значит «этот участок не зашифрован»; всё остальное шифрование
            if !attrs.contains("METHOD=NONE") {
                return Err("шифрованный HLS пока не поддержан".to_string());
            }
        } else if line.starts_with("#EXT-X-BYTERANGE") {
            return Err("HLS с #EXT-X-BYTERANGE (куски внутри одного файла) пока не поддержан".to_string());
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if attrs.contains("BYTERANGE") {
                return Err("#EXT-X-MAP с BYTERANGE пока не поддержан".to_string());
            }
            let uri = attr_value(attrs, "URI").ok_or("в #EXT-X-MAP нет URI")?;
            init = Some(base.join(&uri).map_err(|e| e.to_string())?);
        } else if line.starts_with('#') {
            continue; // остальные служебные строки нам не нужны
        } else {
            // строка без # = адрес куска звука; join склеивает адрес плейлиста и (возможно относительный) путь
            segments.push(base.join(line).map_err(|e| e.to_string())?);
        }
    }
    if segments.is_empty() {
        return Err("в плейлисте нет ни одного куска звука".to_string());
    }
    Ok(HlsPlan { init, segments })
}

/// Скачивает звук из любого `StreamSource`.
pub struct StreamDownloader {
    client: reqwest::Client,
}

impl StreamDownloader {
    pub fn new() -> ProviderResult<Self> {
        Ok(Self { client: build_client()? })
    }

    /// Скачать звук целиком в память.
    pub async fn download_bytes(&self, source: &StreamSource) -> ProviderResult<Vec<u8>> {
        match source {
            StreamSource::DirectUrl(url) => self.get_bytes(url).await,
            StreamSource::HlsPlaylist(url) => self.download_hls(url).await,
            StreamSource::LocalFile(path) => {
                std::fs::read(path).map_err(|e| ProviderError::LocalFile(format!("{}: {e}", path.display())))
            }
        }
    }

    /// Скачать звук и сохранить в файл `dest` (его потом можно отдать плееру).
    pub async fn download_to_file(&self, source: &StreamSource, dest: &Path) -> ProviderResult<()> {
        let bytes = self.download_bytes(source).await?;
        std::fs::write(dest, bytes).map_err(|e| ProviderError::LocalFile(format!("{}: {e}", dest.display())))
    }

    async fn download_hls(&self, url: &str) -> ProviderResult<Vec<u8>> {
        let base = Url::parse(url).map_err(|e| ProviderError::UnexpectedResponse(format!("плохой адрес плейлиста: {e}")))?;
        let playlist = self.get_bytes(url).await?;
        let text = String::from_utf8_lossy(&playlist); // байты -> текст (нечитаемые байты заменятся на �)
        let plan = parse_hls(&text, &base).map_err(ProviderError::UnexpectedResponse)?;

        let mut out: Vec<u8> = Vec::new();
        if let Some(init) = &plan.init {
            out.extend_from_slice(&self.get_bytes(init.as_str()).await?);
        }
        // enumerate даёт пары (номер, элемент): номер нужен для понятного текста ошибки
        for (i, seg) in plan.segments.iter().enumerate() {
            let bytes = self
                .get_bytes(seg.as_str())
                .await
                .map_err(|e| ProviderError::Network(format!("кусок {} из {}: {e}", i + 1, plan.segments.len())))?;
            out.extend_from_slice(&bytes);
            if out.len() > MAX_BYTES {
                return Err(ProviderError::UnexpectedResponse("трек больше 300 МБ, останавливаемся".to_string()));
            }
        }
        Ok(out)
    }

    /// Один GET-запрос, ответ байтами. Любой код, кроме 2xx, считается ошибкой.
    async fn get_bytes(&self, url: &str) -> ProviderResult<Vec<u8>> {
        let resp = self.client.get(url).send().await.map_err(net_err)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ProviderError::Network(format!("код {status} для {}", short(strip_query(url), 100))));
        }
        let bytes = resp.bytes().await.map_err(net_err)?;
        if bytes.len() > MAX_BYTES {
            return Err(ProviderError::UnexpectedResponse("файл больше 300 МБ, останавливаемся".to_string()));
        }
        Ok(bytes.to_vec())
    }
}

/// Адрес без хвоста после `?` (в хвосте временные подписи, в сообщения об ошибках их не пишем).
fn strip_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example.com/track/aac_160k/uuid/playlist.m3u8?sig=1").unwrap()
    }

    // Плейлист по образцу того, что отдаёт SoundCloud (адреса сокращены)
    const SAMPLE: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-MAP:URI=\"https://cdn.example.com/track/init.mp4?sig=1\"\n\
#EXTINF:10.0078,\nhttps://cdn.example.com/track/d0.m4s?sig=1\n#EXTINF:9.98,\nd1.m4s\n#EXT-X-ENDLIST\n";

    #[test]
    fn parses_fmp4_playlist() {
        let plan = parse_hls(SAMPLE, &base()).unwrap();
        assert_eq!(plan.init.unwrap().as_str(), "https://cdn.example.com/track/init.mp4?sig=1");
        assert_eq!(plan.segments.len(), 2);
        assert_eq!(plan.segments[0].as_str(), "https://cdn.example.com/track/d0.m4s?sig=1"); // абсолютная ссылка
        assert_eq!(plan.segments[1].as_str(), "https://cdn.example.com/track/aac_160k/uuid/d1.m4s"); // относительная
    }

    #[test]
    fn rejects_unsupported_playlists() {
        let enc = SAMPLE.replace("#EXT-X-MAP", "#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXT-X-MAP");
        assert!(parse_hls(&enc, &base()).unwrap_err().contains("шифрованный"));
        let none = SAMPLE.replace("#EXT-X-MAP", "#EXT-X-KEY:METHOD=NONE\n#EXT-X-MAP");
        assert!(parse_hls(&none, &base()).is_ok()); // METHOD=NONE это не шифрование
        let live = SAMPLE.replace("#EXT-X-ENDLIST\n", "");
        assert!(parse_hls(&live, &base()).unwrap_err().contains("ENDLIST"));
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nx.m3u8\n#EXT-X-ENDLIST\n";
        assert!(parse_hls(master, &base()).unwrap_err().contains("мастер"));
        assert!(parse_hls("<html>", &base()).is_err());
        assert!(parse_hls("#EXTM3U\n#EXT-X-ENDLIST\n", &base()).is_err()); // нет кусков
    }

    #[test]
    fn plain_playlist_without_map() {
        let text = "#EXTM3U\n#EXTINF:1,\na.mp3\n#EXTINF:1,\nb.mp3\n#EXT-X-ENDLIST\n";
        let plan = parse_hls(text, &base()).unwrap();
        assert!(plan.init.is_none());
        assert_eq!(plan.segments.len(), 2);
    }

    #[test]
    fn attr_value_works() {
        assert_eq!(attr_value("URI=\"a/b.mp4\",BYTERANGE=\"1\"", "URI"), Some("a/b.mp4".to_string()));
        assert_eq!(attr_value("X=1", "URI"), None);
    }
}
