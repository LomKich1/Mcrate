//! Загрузка звука по ссылке. Умеет прямые ссылки на файл и HLS-плейлисты (звук, нарезанный на куски).
//!
//! Два режима:
//!   - `download_bytes` / `download_to_file`: скачать трек целиком, потом играть. Просто и надёжно,
//!     но перед стартом приходится ждать (на медленном интернете минуту и больше).
//!   - `download_progressive`: качать в файл кусками и сообщать о прогрессе (`DownloadProgress`),
//!     чтобы плеер начал играть, пока файл ещё растёт.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use aes::Aes128;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
use mcrate_core::{ProviderError, ProviderResult, StreamSource};
use reqwest::Url; // Url = разобранный веб-адрес

use crate::http::{build_client, net_err, short};
use crate::progress::DownloadProgress;
use crate::ts;

/// Верхний предел размера одного трека, чтобы по ошибке не забить всю память (300 МБ).
const MAX_BYTES: usize = 300 * 1024 * 1024;

/// Ключ шифрования куска (HLS, метод AES-128).
#[derive(Debug, Clone, PartialEq)]
pub struct HlsKey {
    /// Откуда скачать 16 байт ключа.
    pub uri: Url,
    /// «Вектор инициализации» (IV, 16 байт), если плейлист его задал.
    /// Если нет, по стандарту IV = номер куска (см. `iv_from_sequence`).
    pub iv: Option<[u8; 16]>,
}

/// Один кусок звука.
#[derive(Debug, Clone, PartialEq)]
pub struct HlsSegment {
    pub url: Url,
    /// Ключ, которым зашифрован именно этот кусок (None = кусок не зашифрован).
    /// У некоторых сервисов шифруется только часть кусков.
    pub key: Option<HlsKey>,
    /// Порядковый номер куска в потоке (EXT-X-MEDIA-SEQUENCE плюс позиция): из него считается IV по умолчанию.
    pub sequence: u64,
    /// Длительность куска в секундах из `#EXTINF` (0.0, если в плейлисте её нет).
    /// Нужна, чтобы считать долю скачанного трека.
    pub duration: f64,
}

/// «План» HLS-загрузки: что скачать и в каком порядке.
#[derive(Debug, PartialEq)]
pub struct HlsPlan {
    /// Общий «заголовок» (EXT-X-MAP), нужен для fMP4. Его кладём в начало файла.
    pub init: Option<Url>,
    /// Куски звука по порядку.
    pub segments: Vec<HlsSegment>,
}

/// Разбирает строку атрибутов HLS вида `A=1,B="текст, с запятой",C=x` в пары (имя, значение).
/// Кавычки у значений снимаются.
fn parse_attrs(attrs: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let mut rest = attrs.trim();
    while !rest.is_empty() {
        let Some((name, after)) = rest.split_once('=') else { break };
        let (value, remainder) = if let Some(quoted) = after.strip_prefix('"') {
            // значение в кавычках: до закрывающей кавычки (внутри могут быть запятые)
            quoted.split_once('"').unwrap_or((quoted, ""))
        } else {
            after.split_once(',').unwrap_or((after, ""))
        };
        result.push((name.trim().to_string(), value.to_string()));
        rest = remainder.trim_start_matches(',').trim_start();
    }
    result
}

/// Достаёт значение атрибута (с кавычками или без) из строки атрибутов HLS.
fn attr_value(attrs: &str, name: &str) -> Option<String> {
    parse_attrs(attrs).into_iter().find(|(key, _)| key == name).map(|(_, value)| value)
}

/// `0x00112233445566778899AABBCCDDEEFF` -> 16 байт.
fn parse_iv(text: &str) -> Result<[u8; 16], String> {
    let hex = text.trim_start_matches("0x").trim_start_matches("0X");
    if hex.is_empty() || hex.len() > 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("плохой IV в #EXT-X-KEY: {}", short(text, 40)));
    }
    let padded = format!("{hex:0>32}"); // короче 32 цифр: дополняем нулями слева
    let mut iv = [0u8; 16];
    for (i, byte) in iv.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(iv)
}

/// IV по умолчанию: номер куска как 16-байтовое число (старший байт первым). Так требует стандарт HLS.
pub fn iv_from_sequence(sequence: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

/// Разбирает строку `#EXT-X-KEY`: Ok(None) = «дальше без шифрования», Ok(Some) = «дальше шифруем этим ключом».
fn parse_key(attrs: &str, base: &Url) -> Result<Option<HlsKey>, String> {
    let method = attr_value(attrs, "METHOD").ok_or("в #EXT-X-KEY нет METHOD")?;
    match method.as_str() {
        "NONE" => Ok(None),
        "AES-128" => {
            if let Some(format) = attr_value(attrs, "KEYFORMAT")
                && format != "identity"
            {
                return Err(format!("шифрование с KEYFORMAT={format} не поддержано"));
            }
            let uri = attr_value(attrs, "URI").ok_or("в #EXT-X-KEY нет URI")?;
            let uri = base.join(&uri).map_err(|e| e.to_string())?;
            let iv = attr_value(attrs, "IV").map(|v| parse_iv(&v)).transpose()?;
            Ok(Some(HlsKey { uri, iv }))
        }
        other => Err(format!("шифрование {other} не поддержано (умеем только AES-128)")),
    }
}

/// Разбирает текст HLS-плейлиста. `base` = адрес самого плейлиста: от него считаются
/// относительные ссылки. Всё, что мы пока не умеем (другое шифрование, эфиры), сразу отклоняем с понятным текстом.
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
    let mut segments: Vec<HlsSegment> = Vec::new();
    let mut media_sequence: u64 = 0; // номер первого куска
    let mut next_duration: f64 = 0.0; // длительность из последнего #EXTINF: относится к следующему куску
    let mut key: Option<HlsKey> = None; // действующий ключ: действует на все следующие куски, пока не сменится
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = value.trim().parse().map_err(|_| format!("плохой #EXT-X-MEDIA-SEQUENCE: {value}"))?;
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            key = parse_key(attrs, base)?;
        } else if line.starts_with("#EXT-X-BYTERANGE") {
            return Err("HLS с #EXT-X-BYTERANGE (куски внутри одного файла) пока не поддержан".to_string());
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if attr_value(attrs, "BYTERANGE").is_some() {
                return Err("#EXT-X-MAP с BYTERANGE пока не поддержан".to_string());
            }
            if key.is_some() {
                return Err("зашифрованный #EXT-X-MAP пока не поддержан".to_string());
            }
            let uri = attr_value(attrs, "URI").ok_or("в #EXT-X-MAP нет URI")?;
            init = Some(base.join(&uri).map_err(|e| e.to_string())?);
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            // формат: `#EXTINF:10.0078,необязательное название`; нужно число до запятой
            let number = value.split(',').next().unwrap_or("").trim();
            next_duration = number.parse().unwrap_or(0.0);
        } else if line.starts_with('#') {
            continue; // остальные служебные строки нам не нужны
        } else {
            // строка без # = адрес куска звука; join склеивает адрес плейлиста и (возможно относительный) путь
            let url = base.join(line).map_err(|e| e.to_string())?;
            let sequence = media_sequence + segments.len() as u64;
            segments.push(HlsSegment { url, key: key.clone(), sequence, duration: next_duration });
            next_duration = 0.0;
        }
    }
    if segments.is_empty() {
        return Err("в плейлисте нет ни одного куска звука".to_string());
    }
    Ok(HlsPlan { init, segments })
}

/// Расшифровывает один кусок: AES-128 в режиме CBC, добавка PKCS7 (так делает стандарт HLS).
pub fn decrypt_segment(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(format!("размер зашифрованного куска ({} байт) не кратен 16", data.len()));
    }
    let mut buffer = data.to_vec(); // расшифровываем «на месте», поэтому нужна своя копия
    let plain = cbc::Decryptor::<Aes128>::new(key.into(), iv.into())
        .decrypt_padded::<Pkcs7>(&mut buffer)
        .map_err(|_| "не удалось расшифровать кусок (неверный ключ или повреждённые данные)".to_string())?;
    Ok(plain.to_vec())
}

/// Скачивает звук из любого `StreamSource`.
pub struct StreamDownloader {
    client: reqwest::Client,
}

impl StreamDownloader {
    pub fn new() -> ProviderResult<Self> {
        Ok(Self { client: build_client()? })
    }

    /// Загрузчик со своим HTTP-клиентом (например, с куками или заголовками сервиса).
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Скачать звук целиком в память.
    pub async fn download_bytes(&self, source: &StreamSource) -> ProviderResult<Vec<u8>> {
        match source {
            StreamSource::DirectUrl(url) => self.get_bytes(url).await,
            StreamSource::HlsPlaylist(url) => self.download_hls(url).await,
            StreamSource::LocalFile(path) => std::fs::read(path).map_err(|e| ProviderError::LocalFile(format!("{}: {e}", path.display()))),
        }
    }

    /// Скачать звук и сохранить в файл `dest` (его потом можно отдать плееру).
    pub async fn download_to_file(&self, source: &StreamSource, dest: &Path) -> ProviderResult<()> {
        let bytes = self.download_bytes(source).await?;
        std::fs::write(dest, bytes).map_err(|e| ProviderError::LocalFile(format!("{}: {e}", dest.display())))
    }

    /// Скачивает плейлист и разбирает его в план.
    async fn load_plan(&self, url: &str) -> ProviderResult<HlsPlan> {
        let base = Url::parse(url).map_err(|e| ProviderError::UnexpectedResponse(format!("плохой адрес плейлиста: {e}")))?;
        let playlist = self.get_bytes(url).await?;
        let text = String::from_utf8_lossy(&playlist); // байты -> текст (нечитаемые байты заменятся на �)
        parse_hls(&text, &base).map_err(ProviderError::UnexpectedResponse)
    }

    /// Скачивает один кусок и, если он зашифрован, расшифровывает.
    /// `keys` = уже скачанные ключи (один ключ обычно на много кусков), `i` и `total` нужны для понятного текста ошибки.
    async fn fetch_segment(
        &self,
        seg: &HlsSegment,
        i: usize,
        total: usize,
        keys: &mut HashMap<String, [u8; 16]>,
    ) -> ProviderResult<Vec<u8>> {
        let mut bytes =
            self.get_bytes(seg.url.as_str()).await.map_err(|e| ProviderError::Network(format!("кусок {} из {total}: {e}", i + 1)))?;
        if let Some(key) = &seg.key {
            let key_bytes = match keys.get(key.uri.as_str()) {
                Some(known) => *known,
                None => {
                    let fresh = self.get_key(&key.uri).await?;
                    keys.insert(key.uri.to_string(), fresh);
                    fresh
                }
            };
            // IV из плейлиста или, если его нет, номер куска
            let iv = key.iv.unwrap_or_else(|| iv_from_sequence(seg.sequence));
            bytes = decrypt_segment(&bytes, &key_bytes, &iv)
                .map_err(|e| ProviderError::UnexpectedResponse(format!("кусок {} из {total}: {e}", i + 1)))?;
        }
        Ok(bytes)
    }

    async fn download_hls(&self, url: &str) -> ProviderResult<Vec<u8>> {
        let plan = self.load_plan(url).await?;

        let mut out: Vec<u8> = Vec::new();
        if let Some(init) = &plan.init {
            out.extend_from_slice(&self.get_bytes(init.as_str()).await?);
        }
        let total = plan.segments.len();
        let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
        // enumerate даёт пары (номер, элемент): номер нужен для понятного текста ошибки
        for (i, seg) in plan.segments.iter().enumerate() {
            let bytes = self.fetch_segment(seg, i, total, &mut keys).await?;
            out.extend_from_slice(&bytes);
            if out.len() > MAX_BYTES {
                return Err(ProviderError::UnexpectedResponse("трек больше 300 МБ, останавливаемся".to_string()));
            }
        }

        // Звук в MPEG-TS плеер открыть не умеет: достаём из него «голый» поток MP3 или AAC
        if ts::looks_like_ts(&out) {
            let (audio, _kind) = ts::extract_audio(&out).map_err(ProviderError::UnexpectedResponse)?;
            return Ok(audio);
        }
        Ok(out)
    }

    // ---------- загрузка с прогрессом (играть, пока качается) ----------

    /// Качает звук в файл `dest` по кускам и сообщает о ходе дел через `progress`.
    /// Данные видны читателю файла сразу после записи каждого куска (файл не буферизуется).
    ///
    /// Что в `progress` к концу: `finish()` при успехе или `fail(текст)` при ошибке. Отмена:
    /// `progress.cancel()`, загрузка остановится на ближайшем куске с ошибкой «загрузка отменена».
    ///
    /// Особый случай: HLS с кусками в MPEG-TS (VK). Из TS звук вынимается только целиком
    /// (потоковый разбор пока не написан), поэтому такой трек копится в памяти и записывается
    /// в файл в самом конце, как раньше: игра по мере скачивания для него пока не работает.
    pub async fn download_progressive(&self, source: &StreamSource, dest: &Path, progress: &DownloadProgress) -> ProviderResult<()> {
        let result = match source {
            StreamSource::DirectUrl(url) => self.progressive_direct(url, dest, progress).await,
            StreamSource::HlsPlaylist(url) => self.progressive_hls(url, dest, progress).await,
            StreamSource::LocalFile(path) => std::fs::copy(path, dest)
                .map(|n| progress.add_bytes(n))
                .map_err(|e| ProviderError::LocalFile(format!("{}: {e}", path.display()))),
        };
        match &result {
            Ok(()) => progress.finish(),
            Err(e) => progress.fail(e.to_string()),
        }
        result
    }

    /// Прямая ссылка: читаем тело ответа порциями и сразу пишем в файл.
    async fn progressive_direct(&self, url: &str, dest: &Path, progress: &DownloadProgress) -> ProviderResult<()> {
        let mut resp = self.client.get(url).send().await.map_err(net_err)?;
        check_status(resp.status(), url)?;
        let total = resp.content_length().filter(|n| *n > 0); // общий размер, если сервер его сообщил
        let mut file = File::create(dest).map_err(|e| local_err(dest, e))?;
        let mut written: u64 = 0;
        // chunk() отдаёт тело ответа по частям, по мере прихода из сети (None = тело кончилось)
        while let Some(chunk) = resp.chunk().await.map_err(net_err)? {
            if progress.is_cancelled() {
                return Err(cancelled());
            }
            written += chunk.len() as u64;
            if written > MAX_BYTES as u64 {
                return Err(ProviderError::UnexpectedResponse("файл больше 300 МБ, останавливаемся".to_string()));
            }
            file.write_all(&chunk).map_err(|e| local_err(dest, e))?;
            progress.add_bytes(chunk.len() as u64);
            if let Some(total) = total {
                progress.set_fraction(written as f32 / total as f32);
            }
        }
        Ok(())
    }

    /// HLS: после каждого куска дописываем его в файл. Для fMP4 это init + куски подряд.
    async fn progressive_hls(&self, url: &str, dest: &Path, progress: &DownloadProgress) -> ProviderResult<()> {
        let plan = self.load_plan(url).await?;
        let mut file = File::create(dest).map_err(|e| local_err(dest, e))?;
        let mut total_bytes: usize = 0;

        if let Some(init) = &plan.init {
            let bytes = self.get_bytes(init.as_str()).await?;
            total_bytes += bytes.len();
            file.write_all(&bytes).map_err(|e| local_err(dest, e))?;
            progress.add_bytes(bytes.len() as u64);
        }

        let total = plan.segments.len();
        let total_secs: f64 = plan.segments.iter().map(|s| s.duration).sum();
        let mut done_secs = 0.0;
        let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
        // Some(...) = куски оказались MPEG-TS: копим всё в памяти (см. описание функции выше)
        let mut ts_buffer: Option<Vec<u8>> = None;

        for (i, seg) in plan.segments.iter().enumerate() {
            if progress.is_cancelled() {
                return Err(cancelled());
            }
            let bytes = self.fetch_segment(seg, i, total, &mut keys).await?;
            total_bytes += bytes.len();
            if total_bytes > MAX_BYTES {
                return Err(ProviderError::UnexpectedResponse("трек больше 300 МБ, останавливаемся".to_string()));
            }
            if i == 0 && plan.init.is_none() && ts::looks_like_ts(&bytes) {
                ts_buffer = Some(Vec::new());
            }
            match &mut ts_buffer {
                Some(buffer) => buffer.extend_from_slice(&bytes),
                None => {
                    file.write_all(&bytes).map_err(|e| local_err(dest, e))?;
                    progress.add_bytes(bytes.len() as u64);
                }
            }
            // Доля: по длительности кусков, а если в плейлисте нет EXTINF, по числу кусков
            done_secs += seg.duration;
            let fraction = if total_secs > 0.0 { done_secs / total_secs } else { (i + 1) as f64 / total as f64 };
            progress.set_fraction(fraction as f32);
        }

        if let Some(buffer) = ts_buffer {
            let (audio, _kind) = ts::extract_audio(&buffer).map_err(ProviderError::UnexpectedResponse)?;
            file.write_all(&audio).map_err(|e| local_err(dest, e))?;
            progress.add_bytes(audio.len() as u64);
        }
        Ok(())
    }

    /// Скачивает ключ шифрования: ровно 16 байт (иначе это, скорее всего, страница с ошибкой).
    async fn get_key(&self, uri: &Url) -> ProviderResult<[u8; 16]> {
        let bytes = self.get_bytes(uri.as_str()).await.map_err(|e| ProviderError::Network(format!("ключ шифрования: {e}")))?;
        <[u8; 16]>::try_from(bytes.as_slice())
            .map_err(|_| ProviderError::UnexpectedResponse(format!("ключ шифрования должен быть 16 байт, пришло {}", bytes.len())))
    }

    /// Один GET-запрос, ответ байтами. Любой код, кроме 2xx, считается ошибкой.
    async fn get_bytes(&self, url: &str) -> ProviderResult<Vec<u8>> {
        let resp = self.client.get(url).send().await.map_err(net_err)?;
        check_status(resp.status(), url)?;
        let bytes = resp.bytes().await.map_err(net_err)?;
        if bytes.len() > MAX_BYTES {
            return Err(ProviderError::UnexpectedResponse("файл больше 300 МБ, останавливаемся".to_string()));
        }
        Ok(bytes.to_vec())
    }
}

/// Любой код ответа, кроме 2xx, считается ошибкой (адрес в тексте без `?...`).
fn check_status(status: reqwest::StatusCode, url: &str) -> ProviderResult<()> {
    if status.is_success() {
        Ok(())
    } else {
        Err(ProviderError::Network(format!("код {status} для {}", short(strip_query(url), 100))))
    }
}

/// Ошибка записи в файл `path`.
fn local_err(path: &Path, e: std::io::Error) -> ProviderError {
    ProviderError::LocalFile(format!("{}: {e}", path.display()))
}

fn cancelled() -> ProviderError {
    ProviderError::Network("загрузка отменена".to_string())
}

/// Адрес без хвоста после `?` (в хвосте временные подписи, в сообщения об ошибках их не пишем).
fn strip_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockModeEncrypt;

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
        assert_eq!(plan.segments[0].url.as_str(), "https://cdn.example.com/track/d0.m4s?sig=1"); // абсолютная ссылка
        assert_eq!(plan.segments[1].url.as_str(), "https://cdn.example.com/track/aac_160k/uuid/d1.m4s"); // относительная
        assert!(plan.segments.iter().all(|s| s.key.is_none()));
    }

    #[test]
    fn reads_segment_durations() {
        let plan = parse_hls(SAMPLE, &base()).unwrap();
        assert_eq!(plan.segments[0].duration, 10.0078);
        assert_eq!(plan.segments[1].duration, 9.98);
        // нет #EXTINF = длительность 0, плохое число тоже
        let text = "#EXTM3U\n#EXTINF:abc,\na.mp3\nb.mp3\n#EXT-X-ENDLIST\n";
        let plan = parse_hls(text, &base()).unwrap();
        assert_eq!(plan.segments[0].duration, 0.0);
        assert_eq!(plan.segments[1].duration, 0.0);
    }

    #[test]
    fn rejects_unsupported_playlists() {
        let live = SAMPLE.replace("#EXT-X-ENDLIST\n", "");
        assert!(parse_hls(&live, &base()).unwrap_err().contains("ENDLIST"));
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nx.m3u8\n#EXT-X-ENDLIST\n";
        assert!(parse_hls(master, &base()).unwrap_err().contains("мастер"));
        assert!(parse_hls("<html>", &base()).is_err());
        assert!(parse_hls("#EXTM3U\n#EXT-X-ENDLIST\n", &base()).is_err()); // нет кусков
        let range = SAMPLE.replace("#EXTINF:9.98,", "#EXT-X-BYTERANGE:100@0\n#EXTINF:9.98,");
        assert!(parse_hls(&range, &base()).unwrap_err().contains("BYTERANGE"));
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
        // без кавычек и запятая внутри кавычек
        assert_eq!(attr_value("METHOD=AES-128,URI=\"k,1\",IV=0x01", "METHOD"), Some("AES-128".to_string()));
        assert_eq!(attr_value("METHOD=AES-128,URI=\"k,1\",IV=0x01", "URI"), Some("k,1".to_string()));
        assert_eq!(attr_value("METHOD=AES-128,URI=\"k,1\",IV=0x01", "IV"), Some("0x01".to_string()));
    }

    // Плейлист, где шифруется только часть кусков (как у некоторых сервисов): ключ действует
    // до следующей строки #EXT-X-KEY, METHOD=NONE выключает шифрование.
    const PARTLY_ENCRYPTED: &str = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"key.pub?siren=1\"\n#EXTINF:1,\ns0.ts\n\
#EXT-X-KEY:METHOD=NONE\n#EXTINF:1,\ns1.ts\n#EXTINF:1,\ns2.ts\n\
#EXT-X-KEY:METHOD=AES-128,URI=\"https://k.example.com/k2\",IV=0x000102030405060708090A0B0C0D0E0F\n#EXTINF:1,\ns3.ts\n#EXT-X-ENDLIST\n";

    #[test]
    fn parses_per_segment_keys() {
        let plan = parse_hls(PARTLY_ENCRYPTED, &base()).unwrap();
        let keys: Vec<bool> = plan.segments.iter().map(|s| s.key.is_some()).collect();
        assert_eq!(keys, [true, false, false, true]);
        // номера кусков считаются от EXT-X-MEDIA-SEQUENCE
        let seq: Vec<u64> = plan.segments.iter().map(|s| s.sequence).collect();
        assert_eq!(seq, [7, 8, 9, 10]);
        // относительный адрес ключа склеился с адресом плейлиста, IV не задан
        let first = plan.segments[0].key.as_ref().unwrap();
        assert_eq!(first.uri.as_str(), "https://cdn.example.com/track/aac_160k/uuid/key.pub?siren=1");
        assert_eq!(first.iv, None);
        // второй ключ с явным IV
        let last = plan.segments[3].key.as_ref().unwrap();
        assert_eq!(last.uri.as_str(), "https://k.example.com/k2");
        assert_eq!(last.iv, Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]));
    }

    #[test]
    fn rejects_unsupported_encryption() {
        let sample_aes = PARTLY_ENCRYPTED.replace("METHOD=AES-128,URI=\"key.pub?siren=1\"", "METHOD=SAMPLE-AES,URI=\"k\"");
        assert!(parse_hls(&sample_aes, &base()).unwrap_err().contains("SAMPLE-AES"));
        let drm = PARTLY_ENCRYPTED.replace("URI=\"key.pub?siren=1\"", "URI=\"k\",KEYFORMAT=\"com.apple.streamingkeydelivery\"");
        assert!(parse_hls(&drm, &base()).unwrap_err().contains("KEYFORMAT"));
        let no_uri = PARTLY_ENCRYPTED.replace(",URI=\"key.pub?siren=1\"", "");
        assert!(parse_hls(&no_uri, &base()).unwrap_err().contains("URI"));
        let bad_iv = PARTLY_ENCRYPTED.replace("0x000102030405060708090A0B0C0D0E0F", "0xZZ");
        assert!(parse_hls(&bad_iv, &base()).unwrap_err().contains("IV"));
        // зашифрованный EXT-X-MAP
        let enc_map = SAMPLE.replace("#EXT-X-MAP", "#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXT-X-MAP");
        assert!(parse_hls(&enc_map, &base()).unwrap_err().contains("MAP"));
        // METHOD=NONE перед MAP это не шифрование
        let none = SAMPLE.replace("#EXT-X-MAP", "#EXT-X-KEY:METHOD=NONE\n#EXT-X-MAP");
        assert!(parse_hls(&none, &base()).is_ok());
    }

    #[test]
    fn iv_defaults_to_sequence_number() {
        assert_eq!(iv_from_sequence(0), [0u8; 16]);
        assert_eq!(iv_from_sequence(0x0102), [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2]);
        assert_eq!(parse_iv("0x1").unwrap(), iv_from_sequence(1)); // короткий IV дополняется нулями слева
    }

    #[test]
    fn decrypts_what_hls_encrypts() {
        let key = [7u8; 16];
        let iv = iv_from_sequence(42);
        let plain: Vec<u8> = (0..1000u32).map(|i| (i % 253) as u8).collect();
        // Шифруем тем же способом, что делает сервис: AES-128-CBC + PKCS7
        let encrypted = cbc::Encryptor::<Aes128>::new((&key).into(), (&iv).into()).encrypt_padded_vec::<Pkcs7>(&plain);
        assert_ne!(encrypted, plain);
        assert_eq!(decrypt_segment(&encrypted, &key, &iv).unwrap(), plain);
        // Неверный ключ и неверный размер
        assert!(decrypt_segment(&encrypted, &[8u8; 16], &iv).is_err());
        assert!(decrypt_segment(&encrypted[..encrypted.len() - 1], &key, &iv).is_err());
        assert!(decrypt_segment(&[], &key, &iv).is_err());
    }
}
