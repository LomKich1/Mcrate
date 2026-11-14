//! SoundCloud без аккаунта и без платной подписки.
//!
//! ВАЖНО: это НЕОФИЦИАЛЬНЫЙ доступ. Мы ходим в тот же `api-v2.soundcloud.com`, что и сайт,
//! с публичным `client_id`, который берём из скриптов сайта. Официальный API требует
//! платную подписку Artist Pro, поэтому для личного плеера он не подходит.
//! SoundCloud может в любой момент изменить или закрыть этот доступ.
//!
//! Что берём из поиска: только треки с policy=ALLOW (полное воспроизведение).
//! Треки с policy=SNIP (30-секундные превью) не показываем.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex; // Mutex = «замок»: данные одновременно меняет только один поток

use async_trait::async_trait;
use mcrate_core::{MusicProvider, ProviderError, ProviderKind, ProviderResult, StreamSource, Track};
use reqwest::Url;
use serde_json::Value; // Value = «любой JSON»

use crate::http::{build_client, net_err, short};

/// В каком формате предпочитаем получать звук.
/// Если предпочтительного нет у конкретного трека, берётся следующий по списку.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamFormat {
    /// AAC 160 кбит/с по HLS (куски fMP4). Лучшее качество, тут и тянется основной интерес. По умолчанию.
    #[default]
    HlsAac,
    /// Обычный MP3 128 кбит/с одной ссылкой. Проще, но качество ниже, и SoundCloud собирался его убрать.
    ProgressiveMp3,
}

/// Один вариант потока трека (то, что SoundCloud называет transcoding).
#[derive(Debug, Clone, PartialEq)]
struct Transcoding {
    url: String,      // адрес, по которому надо ещё раз спросить настоящую ссылку
    preset: String,   // например aac_160k или mp3_1_0
    protocol: String, // hls или progressive
}

/// Что запоминаем про трек после поиска, чтобы потом получить ссылку на звук.
#[derive(Debug, Clone)]
struct CachedTrack {
    transcodings: Vec<Transcoding>,
    authorization: String, // «пропуск» на этот трек, его выдаёт поиск
}

pub struct SoundCloudProvider {
    client: reqwest::Client,
    format: StreamFormat,
    client_id: Mutex<Option<String>>,         // добытый client_id (None = ещё не добывали)
    client_id_file: Option<PathBuf>,          // где хранить client_id между запусками (None = не хранить)
    cache: Mutex<HashMap<String, CachedTrack>>, // id трека -> данные о потоках из последнего поиска
}

impl SoundCloudProvider {
    pub fn new() -> ProviderResult<Self> {
        Self::with_format(StreamFormat::default())
    }

    pub fn with_format(format: StreamFormat) -> ProviderResult<Self> {
        Ok(Self {
            client: build_client()?,
            format,
            client_id: Mutex::new(None),
            client_id_file: None,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Включает хранение client_id в файле. Добыча client_id с сайта долгая (качаются большие скрипты:
    /// на медленном интернете это полминуты), поэтому делаем её один раз, а не при каждом запуске.
    /// client_id публичный, секретов в файле нет. Вызывать сразу после создания: `new()?.with_client_id_file(путь)`.
    pub fn with_client_id_file(mut self, path: PathBuf) -> Self {
        self.client_id_file = Some(path);
        self
    }

    /// Заранее добывает client_id (из файла или с сайта), чтобы первый поиск не ждал.
    /// Удобно вызвать в фоне при запуске программы.
    pub async fn prepare(&self) -> ProviderResult<()> {
        self.client_id().await.map(|_| ())
    }

    /// Читает сохранённый client_id из файла. None = файла нет или в нём не то, что нужно.
    fn read_saved_client_id(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.client_id_file.as_ref()?).ok()?; // `?` = нет файла/пути, значит None
        let id = text.trim();
        // настоящий client_id это ровно 32 буквы и цифры; всё остальное считаем мусором
        (id.len() == 32 && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| id.to_string())
    }

    /// Сохраняет client_id в файл. Ошибку только печатаем: без файла всё работает, просто медленнее.
    fn save_client_id(&self, id: &str) {
        let Some(path) = &self.client_id_file else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir); // `let _ =` = результат нарочно игнорируем
        }
        if let Err(e) = std::fs::write(path, id) {
            eprintln!("не удалось сохранить client_id: {e}");
        }
    }

    /// Забывает client_id и в памяти, и на диске (когда SoundCloud сказал, что он устарел).
    fn forget_client_id(&self) {
        *self.client_id.lock().unwrap() = None;
        if let Some(path) = &self.client_id_file {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Возвращает client_id: из памяти, а если его ещё нет, добывает с сайта.
    async fn client_id(&self) -> ProviderResult<String> {
        // lock() берёт замок, unwrap() = «если замок сломан, падаем». Держим его только на миг, без .await внутри.
        if let Some(id) = self.client_id.lock().unwrap().clone() {
            return Ok(id);
        }
        // Потом из файла (быстро), и только если там нет, с сайта (долго)
        if let Some(id) = self.read_saved_client_id() {
            *self.client_id.lock().unwrap() = Some(id.clone());
            return Ok(id);
        }
        let id = fetch_client_id(&self.client).await?;
        self.save_client_id(&id);
        *self.client_id.lock().unwrap() = Some(id.clone());
        Ok(id)
    }

    /// GET-запрос с client_id, ответ как JSON. Если SoundCloud ответил 401 или 403, client_id
    /// мог устареть (он периодически меняется): добываем новый и пробуем ещё раз.
    async fn get_json(&self, base: &str, params: &[(&str, &str)]) -> ProviderResult<Value> {
        for attempt in 0..2 {
            let id = self.client_id().await?;
            let mut all: Vec<(&str, &str)> = params.to_vec(); // копия списка параметров
            all.push(("client_id", id.as_str()));
            // parse_with_params сам кодирует русские буквы и пробелы
            let url = Url::parse_with_params(base, &all)
                .map_err(|e| ProviderError::UnexpectedResponse(format!("плохой адрес: {e}")))?;

            let resp = self.client.get(url).send().await.map_err(net_err)?;
            let status = resp.status();
            if (status.as_u16() == 401 || status.as_u16() == 403) && attempt == 0 {
                self.forget_client_id(); // забываем старый (и в файле), на втором круге добудем новый
                continue;
            }
            let body = resp.text().await.map_err(net_err)?;
            if !status.is_success() {
                return Err(ProviderError::Network(format!("код {status}: {}", short(&body, 150))));
            }
            return serde_json::from_str(&body)
                .map_err(|e| ProviderError::UnexpectedResponse(format!("ответ не похож на JSON: {e}")));
        }
        Err(ProviderError::NotAuthorized) // сюда не доходим, но Rust требует «выход» после цикла
    }
}

#[async_trait]
impl MusicProvider for SoundCloudProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::SoundCloud
    }

    async fn search(&self, query: &str) -> ProviderResult<Vec<Track>> {
        let json = self.get_json("https://api-v2.soundcloud.com/search/tracks", &[("q", query), ("limit", "30")]).await?;
        let items = json["collection"]
            .as_array()
            .ok_or_else(|| ProviderError::UnexpectedResponse("в ответе поиска нет списка collection".to_string()))?;

        let mut tracks = Vec::new();
        let mut found = Vec::new();
        for item in items {
            if let Some((track, cached)) = parse_item(item) {
                found.push((track.id.clone(), cached));
                tracks.push(track);
            }
        }
        // Замок берём ПОСЛЕ последнего .await и сразу отпускаем
        let mut cache = self.cache.lock().unwrap();
        if cache.len() > 500 {
            cache.clear(); // чтобы память не росла бесконечно
        }
        cache.extend(found);
        Ok(tracks)
    }

    async fn get_stream(&self, track: &Track) -> ProviderResult<StreamSource> {
        // Данные о потоках берём из запомненного при поиске. Если трека там нет (например, он из старой
        // сессии), нужен новый поиск. Запрос трека по id мы пока не делаем: он не проверен.
        let cached = self.cache.lock().unwrap().get(&track.id).cloned().ok_or_else(|| {
            ProviderError::UnexpectedResponse("трека нет в памяти провайдера: найди его поиском ещё раз".to_string())
        })?;
        let chosen = pick_transcoding(&cached.transcodings, self.format).ok_or_else(|| {
            ProviderError::UnexpectedResponse("у трека нет подходящего потока (aac_160k, aac_96k, mp3)".to_string())
        })?;

        // Спрашиваем настоящую (подписанную, временную) ссылку на звук
        let json = self.get_json(&chosen.url, &[("track_authorization", cached.authorization.as_str())]).await?;
        let url = json["url"]
            .as_str()
            .ok_or_else(|| ProviderError::UnexpectedResponse("в ответе нет поля url".to_string()))?
            .to_string();
        Ok(if chosen.protocol == "hls" { StreamSource::HlsPlaylist(url) } else { StreamSource::DirectUrl(url) })
    }

    async fn set_like(&self, _track: &Track, _liked: bool) -> ProviderResult<()> {
        Err(ProviderError::Unsupported) // без входа в аккаунт лайкать нельзя
    }
}

/// Превращает один найденный трек из JSON в наш Track. None = трек нам не подходит.
fn parse_item(item: &Value) -> Option<(Track, CachedTrack)> {
    // ALLOW = полное воспроизведение. SNIP = только 30-секундное превью, BLOCK = нельзя
    if item["policy"].as_str() != Some("ALLOW") || item["streamable"].as_bool() == Some(false) {
        return None;
    }
    let id = item["id"].as_u64()?.to_string(); // `?` = если поля нет, сразу вернуть None
    let title = item["title"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("(без названия)");
    // Исполнитель: сначала поле «artist» из метаданных, иначе имя того, кто залил трек
    let artist = item["publisher_metadata"]["artist"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| item["user"]["username"].as_str().map(str::trim).filter(|s| !s.is_empty()));
    let transcodings = parse_transcodings(item);
    if transcodings.is_empty() {
        return None;
    }
    let track = Track {
        id,
        provider: ProviderKind::SoundCloud,
        title: title.to_string(),
        artists: artist.map(|a| vec![a.to_string()]).unwrap_or_default(),
        duration_secs: (item["duration"].as_u64().unwrap_or(0) / 1000) as u32, // в JSON миллисекунды
    };
    let cached = CachedTrack { transcodings, authorization: item["track_authorization"].as_str().unwrap_or("").to_string() };
    Some((track, cached))
}

fn parse_transcodings(item: &Value) -> Vec<Transcoding> {
    item["media"]["transcodings"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|tc| {
                    Some(Transcoding {
                        url: tc["url"].as_str()?.to_string(),
                        preset: tc["preset"].as_str()?.to_string(),
                        protocol: tc["format"]["protocol"].as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Выбирает поток по порядку предпочтений. Мастер-плейлист abr_sq и HLS-MP3 не берём: они нам не нужны.
fn pick_transcoding(list: &[Transcoding], format: StreamFormat) -> Option<&Transcoding> {
    // пары (preset, protocol) от самого желаемого к запасным
    let order: [(&str, &str); 3] = match format {
        StreamFormat::HlsAac => [("aac_160k", "hls"), ("aac_96k", "hls"), ("mp3_1_0", "progressive")],
        StreamFormat::ProgressiveMp3 => [("mp3_1_0", "progressive"), ("aac_160k", "hls"), ("aac_96k", "hls")],
    };
    order.iter().find_map(|(preset, protocol)| list.iter().find(|t| t.preset == *preset && t.protocol == *protocol))
}

/// Скачивает текст по адресу. При сбое пробует ещё раз: на плохом интернете соединение иногда обрывается.
/// None = не получилось ни разу.
async fn get_text_retry(client: &reqwest::Client, url: &str) -> Option<String> {
    for _ in 0..2 {
        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    return Some(text);
                }
            }
        }
    }
    None
}

/// Добывает client_id: открывает главную страницу, находит её скрипты и ищет в них client_id.
async fn fetch_client_id(client: &reqwest::Client) -> ProviderResult<String> {
    let html = client
        .get("https://soundcloud.com/")
        .send()
        .await
        .map_err(net_err)?
        .text()
        .await
        .map_err(net_err)?;
    let scripts = find_script_urls(&html);
    let mut failed = 0; // сколько скриптов не скачалось
    // rev() = с конца: client_id обычно лежит в последних скриптах
    for url in scripts.iter().rev() {
        let Some(js) = get_text_retry(client, url).await else {
            failed += 1;
            continue; // этот скрипт не скачался, пробуем следующий
        };
        if let Some(id) = find_client_id(&js) {
            return Ok(id);
        }
    }
    Err(ProviderError::UnexpectedResponse(format!(
        "client_id не найден в скриптах soundcloud.com (не скачалось скриптов: {failed} из {}; сайт мог измениться или соединение нестабильно)",
        scripts.len()
    )))
}

/// Ищет в HTML адреса скриптов (.js) с a-v2.sndcdn.com.
fn find_script_urls(html: &str) -> Vec<String> {
    let prefix = "https://a-v2.sndcdn.com/assets/";
    let mut urls: Vec<String> = Vec::new();
    for (start, _) in html.match_indices(prefix) {
        let rest = &html[start..];
        let end = rest.find(|c: char| matches!(c, '"' | '\'' | ' ' | '<' | '>')).unwrap_or(rest.len());
        let url = &rest[..end];
        if url.ends_with(".js") && !urls.iter().any(|u| u == url) {
            urls.push(url.to_string());
        }
    }
    urls
}

/// Ищет в тексте скрипта client_id: 32 символа (буквы и цифры) после слова client_id.
fn find_client_id(js: &str) -> Option<String> {
    for needle in ["client_id:\"", "client_id: \"", "client_id=", "\"client_id\":\""] {
        for (start, _) in js.match_indices(needle) {
            let id: String = js[start + needle.len()..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if id.len() == 32 {
                return Some(id);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json; // json! = удобно писать JSON прямо в коде

    /// Трек по образцу того, что отдаёт поиск (поля и пресеты взяты из настоящего ответа).
    fn sample(policy: &str) -> Value {
        json!({
            "id": 123456u64,
            "title": "  Группа крови ",
            "duration": 247_000u64,
            "policy": policy,
            "streamable": true,
            "track_authorization": "auth-token",
            "user": { "username": "Кино (залил Вася)" },
            "publisher_metadata": { "artist": "Кино" },
            "media": { "transcodings": [
                { "url": "https://api-v2.soundcloud.com/media/1/progressive", "preset": "mp3_1_0",
                  "format": { "protocol": "progressive", "mime_type": "audio/mpeg" } },
                { "url": "https://api-v2.soundcloud.com/media/1/hls96", "preset": "aac_96k",
                  "format": { "protocol": "hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\"" } },
                { "url": "https://api-v2.soundcloud.com/media/1/hls160", "preset": "aac_160k",
                  "format": { "protocol": "hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\"" } },
                { "url": "https://api-v2.soundcloud.com/media/1/abr", "preset": "abr_sq",
                  "format": { "protocol": "hls", "mime_type": "audio/mpegurl" } }
            ] }
        })
    }

    #[test]
    fn parses_allowed_track() {
        let (track, cached) = parse_item(&sample("ALLOW")).unwrap();
        assert_eq!(track.id, "123456");
        assert_eq!(track.title, "Группа крови"); // пробелы по краям убраны
        assert_eq!(track.artists, vec!["Кино".to_string()]); // artist из метаданных важнее имени загрузившего
        assert_eq!(track.duration_secs, 247);
        assert_eq!(track.provider, ProviderKind::SoundCloud);
        assert_eq!(cached.authorization, "auth-token");
        assert_eq!(cached.transcodings.len(), 4);
    }

    #[test]
    fn skips_previews_and_blocked() {
        assert!(parse_item(&sample("SNIP")).is_none()); // 30-секундное превью
        assert!(parse_item(&sample("BLOCK")).is_none());
    }

    #[test]
    fn artist_falls_back_to_username() {
        let mut item = sample("ALLOW");
        item["publisher_metadata"] = Value::Null;
        let (track, _) = parse_item(&item).unwrap();
        assert_eq!(track.artists, vec!["Кино (залил Вася)".to_string()]);
    }

    #[test]
    fn track_without_streams_is_skipped() {
        let mut item = sample("ALLOW");
        item["media"]["transcodings"] = json!([]);
        assert!(parse_item(&item).is_none());
    }

    #[test]
    fn picks_stream_by_preference() {
        let (_, cached) = parse_item(&sample("ALLOW")).unwrap();
        let aac = pick_transcoding(&cached.transcodings, StreamFormat::HlsAac).unwrap();
        assert_eq!(aac.preset, "aac_160k");
        let mp3 = pick_transcoding(&cached.transcodings, StreamFormat::ProgressiveMp3).unwrap();
        assert_eq!(mp3.preset, "mp3_1_0");

        // нет 160k: берём 96k; нет и его: берём mp3; ничего подходящего: None
        let no160: Vec<Transcoding> = cached.transcodings.iter().filter(|t| t.preset != "aac_160k").cloned().collect();
        assert_eq!(pick_transcoding(&no160, StreamFormat::HlsAac).unwrap().preset, "aac_96k");
        let only_mp3: Vec<Transcoding> = cached.transcodings.iter().filter(|t| t.preset == "mp3_1_0").cloned().collect();
        assert_eq!(pick_transcoding(&only_mp3, StreamFormat::HlsAac).unwrap().preset, "mp3_1_0");
        let only_abr: Vec<Transcoding> = cached.transcodings.iter().filter(|t| t.preset == "abr_sq").cloned().collect();
        assert!(pick_transcoding(&only_abr, StreamFormat::HlsAac).is_none());
    }

    #[test]
    fn client_id_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mcrate-test-cid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("data").join("cid.txt"); // папки data ещё нет: она должна создаться
        let id = "aBcD1234eFgH5678iJkL9012mNoP3456";

        let p = SoundCloudProvider::new().unwrap().with_client_id_file(file.clone());
        assert_eq!(p.read_saved_client_id(), None); // файла ещё нет
        p.save_client_id(id);
        // «новый запуск»: другой провайдер читает то, что сохранил первый
        let p2 = SoundCloudProvider::new().unwrap().with_client_id_file(file.clone());
        assert_eq!(p2.read_saved_client_id(), Some(id.to_string()));

        std::fs::write(&file, "мусор вместо client_id").unwrap();
        assert_eq!(p2.read_saved_client_id(), None); // не 32 буквы-цифры

        p2.save_client_id(id);
        p2.forget_client_id();
        assert!(!file.exists()); // забыли и на диске
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_file_nothing_is_saved() {
        let p = SoundCloudProvider::new().unwrap();
        p.save_client_id("aBcD1234eFgH5678iJkL9012mNoP3456"); // не должно ни падать, ни писать
        assert_eq!(p.read_saved_client_id(), None);
    }

    #[test]
    fn finds_script_urls() {
        let html = r#"<script crossorigin src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>
            <link href="https://a-v2.sndcdn.com/assets/style.css">
            <script src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>"#;
        assert_eq!(find_script_urls(html), vec!["https://a-v2.sndcdn.com/assets/0-abc.js"]);
    }

    #[test]
    fn finds_client_id() {
        let id = "aBcD1234eFgH5678iJkL9012mNoP3456";
        assert_eq!(find_client_id(&format!("x={{client_id:\"{id}\",a:1}}")), Some(id.to_string()));
        assert_eq!(find_client_id("client_id:\"tooShort\""), None);
    }
}
