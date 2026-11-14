//! Пробник SoundCloud. Ничего не воспроизводит и ничего не сохраняет: только ходит в сеть
//! и печатает, что именно сейчас отдаёт SoundCloud. Нужен, чтобы увидеть настоящие данные
//! ДО того, как мы напишем провайдер (формат потока, контейнер сегментов, ограничения).
//!
//! Запуск:  cargo run -p mcrate-providers --example sc_probe -- "Кино Группа крови"
//! (слова после `--` это поисковый запрос; без них используется запрос по умолчанию)
//!
//! Это НЕОФИЦИАЛЬНЫЙ доступ: SoundCloud может его изменить или закрыть в любой момент.

use std::collections::{BTreeMap, BTreeSet}; // BTreeMap и BTreeSet = словарь и набор, хранят записи по порядку
use std::time::Duration;

use serde_json::Value; // Value = «любой JSON»: смотрим данные, не описывая заранее их форму

// Притворяемся обычным браузером: сайт отдаёт страницу так же, как человеку.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// Ищет в тексте HTML-страницы адреса скриптов (.js) с сервера a-v2.sndcdn.com.
/// Именно в этих скриптах сайт хранит свой публичный client_id.
fn find_script_urls(html: &str) -> Vec<String> {
    let prefix = "https://a-v2.sndcdn.com/assets/";
    let mut urls: Vec<String> = Vec::new();
    // match_indices отдаёт позиции всех мест, где встретился prefix
    for (start, _) in html.match_indices(prefix) {
        let rest = &html[start..]; // кусок текста от начала адреса до конца страницы
        // адрес заканчивается на первом символе из этого набора (кавычка, пробел, скобка)
        let end = rest.find(|c: char| matches!(c, '"' | '\'' | ' ' | '<' | '>')).unwrap_or(rest.len());
        let url = &rest[..end];
        if url.ends_with(".js") && !urls.iter().any(|u| u == url) {
            urls.push(url.to_string()); // to_string делает копию, чтобы хранить её отдельно
        }
    }
    urls
}

/// Ищет в тексте скрипта client_id: 32 символа (буквы и цифры) после слова client_id.
fn find_client_id(js: &str) -> Option<String> {
    // возможные способы записи в коде сайта
    for needle in ["client_id:\"", "client_id: \"", "client_id=", "\"client_id\":\""] {
        for (start, _) in js.match_indices(needle) {
            let after = &js[start + needle.len()..];
            // берём подряд идущие буквы и цифры
            let id: String = after.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if id.len() == 32 {
                return Some(id);
            }
        }
    }
    None
}

/// По первым байтам сегмента угадывает контейнер (формат «упаковки» звука).
fn classify_segment(b: &[u8]) -> &'static str {
    if b.len() >= 2 && b[0] == 0xFF && (b[1] & 0xF0) == 0xF0 {
        // 0xFFF в начале = «синхрослово» аудиокадра. Биты слоя (маска 0x06) отличают AAC от MP3.
        return if (b[1] & 0x06) == 0 { "ADTS (сырой AAC, кадры подряд)" } else { "MP3 (кадры подряд)" };
    }
    if !b.is_empty() && b[0] == 0x47 {
        return "MPEG-TS (пакеты по 188 байт)";
    }
    if b.len() >= 8 && matches!(&b[4..8], b"ftyp" | b"styp" | b"moof" | b"moov") {
        return "fMP4 (ISO BMFF, куски MP4)";
    }
    if b.len() >= 3 && &b[..3] == b"ID3" {
        return "MP3 с тегом ID3 в начале";
    }
    "не распознан"
}

/// Первые `max` символов строки (по символам, а не по байтам: иначе можно разрезать кириллицу пополам).
fn short(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Адрес без хвоста после `?`. В хвосте у ссылок на потоки лежат временные подписи,
/// в вывод их печатать не нужно.
fn strip_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// Байты в виде «ff f1 50 ...»
fn hex_head(b: &[u8], n: usize) -> String {
    b.iter().take(n).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

type AnyError = Box<dyn std::error::Error>; // «любая ошибка»: удобно, чтобы `?` работал со всем подряд

/// Скачать страницу как текст. Возвращает код ответа (200 = ок) и сам текст.
async fn get_text(client: &reqwest::Client, url: &str) -> Result<(u16, String), AnyError> {
    let resp = client.get(url).send().await?; // отправили GET-запрос и дождались ответа
    let status = resp.status().as_u16();
    let body = resp.text().await?;
    Ok((status, body))
}

/// Скачать как есть, байтами (для кусков звука).
async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<(u16, Vec<u8>), AnyError> {
    let resp = client.get(url).send().await?;
    let status = resp.status().as_u16();
    let body = resp.bytes().await?.to_vec();
    Ok((status, body))
}

// #[tokio::main] превращает async fn main в обычную точку входа: сам запускает асинхронный движок
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\nОШИБКА: {e}");
        std::process::exit(1); // код 1 = программа завершилась с ошибкой
    }
}

async fn run() -> Result<(), AnyError> {
    // Поисковый запрос = первый аргумент командной строки
    let query = std::env::args().nth(1).unwrap_or_else(|| "Кино Группа крови".to_string());
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(25)) // если сервер молчит дольше, сдаёмся
        .build()?;

    // ---------- Шаг 1: достаём client_id из скриптов сайта ----------
    println!("== Шаг 1: ищем client_id на soundcloud.com");
    let (status, html) = get_text(&client, "https://soundcloud.com/").await?;
    println!("главная страница: код {status}, {} символов", html.len());
    let scripts = find_script_urls(&html);
    println!("найдено скриптов a-v2.sndcdn.com: {}", scripts.len());

    let mut client_id: Option<String> = None;
    // rev() = с конца: client_id обычно лежит в последних скриптах
    for url in scripts.iter().rev() {
        let (st, js) = get_text(&client, url).await?;
        if st != 200 {
            continue; // этот скрипт не скачался, пробуем следующий
        }
        if let Some(id) = find_client_id(&js) {
            println!("client_id найден в {}", short(url, 80));
            client_id = Some(id);
            break;
        }
    }
    let client_id = client_id.ok_or("client_id не найден ни в одном скрипте (сайт мог измениться)")?;
    println!("client_id: {client_id} (он публичный, виден всем в браузере)");

    // ---------- Шаг 2: поиск треков ----------
    println!("\n== Шаг 2: поиск «{query}»");
    // parse_with_params сам аккуратно кодирует русские буквы и пробелы в адресе
    let url = reqwest::Url::parse_with_params(
        "https://api-v2.soundcloud.com/search/tracks",
        [("q", query.as_str()), ("client_id", client_id.as_str()), ("limit", "20")],
    )?;
    let (status, body) = get_text(&client, url.as_str()).await?;
    println!("поиск: код {status}");
    if status != 200 {
        return Err(format!("поиск не удался: {}", short(&body, 200)).into());
    }
    let json: Value = serde_json::from_str(&body)?;
    let tracks = json["collection"].as_array().ok_or("в ответе нет списка collection")?;
    println!("треков в ответе: {}", tracks.len());

    let mut policies: BTreeMap<String, usize> = BTreeMap::new(); // сколько треков с каждым значением policy
    let mut stream_kinds: BTreeSet<String> = BTreeSet::new(); // какие виды потоков вообще встречаются
    for (i, t) in tracks.iter().enumerate() {
        let policy = t["policy"].as_str().unwrap_or("?");
        *policies.entry(policy.to_string()).or_insert(0) += 1;
        let transcodings = t["media"]["transcodings"].as_array().cloned().unwrap_or_default();
        for tc in &transcodings {
            stream_kinds.insert(format!(
                "preset={} protocol={} mime={}",
                tc["preset"].as_str().unwrap_or("?"),
                tc["format"]["protocol"].as_str().unwrap_or("?"),
                tc["format"]["mime_type"].as_str().unwrap_or("?"),
            ));
        }
        if i < 5 {
            // подробно печатаем только первые пять
            println!(
                "  #{i}: «{}» — {} | {} с | policy={policy} streamable={} | потоков: {}",
                short(t["title"].as_str().unwrap_or("?"), 50),
                short(t["user"]["username"].as_str().unwrap_or("?"), 30),
                t["duration"].as_u64().unwrap_or(0) / 1000, // duration в миллисекундах
                t["streamable"],
                transcodings.len()
            );
        }
    }
    println!("значения policy по всем трекам: {policies:?}");
    println!("виды потоков по всем трекам:");
    for k in &stream_kinds {
        println!("  - {k}");
    }

    // ---------- Шаг 3: просим ссылку на поток у первого подходящего трека ----------
    println!("\n== Шаг 3: ссылка на поток");
    // ищем первый трек с policy=ALLOW (полное воспроизведение); если нет, берём первый
    let track = tracks.iter().find(|t| t["policy"] == "ALLOW").or(tracks.first()).ok_or("поиск вернул 0 треков")?;
    println!("берём: «{}»", short(track["title"].as_str().unwrap_or("?"), 60));
    let transcodings = track["media"]["transcodings"].as_array().cloned().unwrap_or_default();
    // предпочитаем HLS + AAC, иначе любой HLS, иначе самый первый
    let pick = transcodings
        .iter()
        .find(|tc| tc["format"]["protocol"] == "hls" && tc["preset"].as_str().unwrap_or("").contains("aac"))
        .or_else(|| transcodings.iter().find(|tc| tc["format"]["protocol"] == "hls"))
        .or(transcodings.first())
        .ok_or("у трека нет ни одного потока (transcodings пуст)")?;
    println!(
        "выбран поток: preset={} protocol={} mime={}",
        pick["preset"], pick["format"]["protocol"], pick["format"]["mime_type"]
    );
    let tc_url = pick["url"].as_str().ok_or("у потока нет url")?;
    let auth = track["track_authorization"].as_str().unwrap_or(""); // пропуск на этот трек, его выдал поиск
    let url = reqwest::Url::parse_with_params(tc_url, [("client_id", client_id.as_str()), ("track_authorization", auth)])?;
    let (status, body) = get_text(&client, url.as_str()).await?;
    println!("запрос ссылки: код {status}");
    if status != 200 {
        return Err(format!("ссылку не выдали: {}", short(&body, 200)).into());
    }
    let json: Value = serde_json::from_str(&body)?;
    let stream_url = json["url"].as_str().ok_or("в ответе нет поля url")?.to_string();
    println!("ссылка на поток: {}?<подпись скрыта>", strip_query(&stream_url));

    // ---------- Шаг 4: смотрим плейлист и первый кусок звука ----------
    println!("\n== Шаг 4: плейлист и сегменты");
    let (status, playlist) = get_text(&client, &stream_url).await?;
    println!("плейлист: код {status}, {} символов", playlist.len());
    println!("--- начало плейлиста (адреса укорочены) ---");
    for line in playlist.lines().take(14) {
        println!("{}", short(strip_query(line), 110));
    }
    println!("--- конец начала ---");
    let segments: Vec<&str> = playlist.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    println!(
        "сегментов: {} | шифрование (EXT-X-KEY): {} | общий кусок-заголовок (EXT-X-MAP): {} | целиком, не живой эфир (ENDLIST): {}",
        segments.len(),
        playlist.contains("#EXT-X-KEY"),
        playlist.contains("#EXT-X-MAP"),
        playlist.contains("#EXT-X-ENDLIST"),
    );

    let base = reqwest::Url::parse(&stream_url)?; // базовый адрес: от него считаются относительные ссылки
    // Если есть #EXT-X-MAP:URI="...", это «заголовок» fMP4, его тоже стоит посмотреть
    if let Some(map_line) = playlist.lines().find(|l| l.starts_with("#EXT-X-MAP")) {
        if let Some(uri) = map_line.split("URI=\"").nth(1).and_then(|s| s.split('"').next()) {
            let init = base.join(uri)?; // join склеивает базовый адрес и относительный
            let (st, bytes) = get_bytes(&client, init.as_str()).await?;
            println!("init-сегмент: код {st}, {} байт, начало: {} → {}", bytes.len(), hex_head(&bytes, 16), classify_segment(&bytes));
        }
    }
    if let Some(first) = segments.first() {
        let seg = base.join(first)?;
        let (st, bytes) = get_bytes(&client, seg.as_str()).await?;
        println!("первый сегмент: код {st}, {} байт, начало: {} → {}", bytes.len(), hex_head(&bytes, 16), classify_segment(&bytes));
    } else {
        println!("в плейлисте нет сегментов (возможно, это не обычный HLS)");
    }

    println!("\nГотово. Пришли весь этот вывод целиком.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_script_urls() {
        let html = r#"<script crossorigin src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>
            <script src="https://a-v2.sndcdn.com/assets/1-def.js"></script>
            <link href="https://a-v2.sndcdn.com/assets/style.css">
            <script src="https://a-v2.sndcdn.com/assets/0-abc.js"></script>"#;
        // .css отбрасывается, дубль не повторяется
        assert_eq!(
            find_script_urls(html),
            vec!["https://a-v2.sndcdn.com/assets/0-abc.js", "https://a-v2.sndcdn.com/assets/1-def.js"]
        );
    }

    #[test]
    fn finds_client_id() {
        let id = "aBcD1234eFgH5678iJkL9012mNoP3456";
        assert_eq!(id.len(), 32);
        assert_eq!(find_client_id(&format!("x={{client_id:\"{id}\",a:1}}")), Some(id.to_string()));
        assert_eq!(find_client_id(&format!("https://x?client_id={id}&y=1")), Some(id.to_string()));
        assert_eq!(find_client_id("client_id:\"tooShort\""), None); // не 32 символа
        assert_eq!(find_client_id("ничего похожего"), None);
    }

    #[test]
    fn classifies_segments() {
        assert!(classify_segment(&[0xFF, 0xF1, 0x50, 0x80]).starts_with("ADTS"));
        assert!(classify_segment(&[0xFF, 0xFB, 0x90, 0x00]).starts_with("MP3"));
        assert!(classify_segment(&[0x47, 0x40, 0x00, 0x10]).starts_with("MPEG-TS"));
        assert!(classify_segment(&[0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'm', b'p', b'4', b'2']).starts_with("fMP4"));
        assert!(classify_segment(b"ID3\x04\x00").starts_with("MP3 с тегом"));
        assert_eq!(classify_segment(&[1, 2, 3]), "не распознан");
    }

    #[test]
    fn strips_and_shortens() {
        assert_eq!(strip_query("https://a/b.m3u8?Policy=secret"), "https://a/b.m3u8");
        assert_eq!(short("Группа крови", 6), "Группа"); // по символам, кириллица не ломается
    }
}
