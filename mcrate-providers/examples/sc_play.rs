//! Проверка SoundCloud «от поиска до звука»: ищет трек, получает ссылку, скачивает и играет первые секунды.
//! Заодно проверяет перемотку (на 4-й секунде прыгаем на 30-ю).
//!
//! Запуск:
//!   cargo run -p mcrate-providers --example sc_play                      (поиск по умолчанию, формат AAC/HLS)
//!   cargo run -p mcrate-providers --example sc_play -- "Кино Группа крови"
//!   cargo run -p mcrate-providers --example sc_play -- --mp3             (обычный MP3 вместо AAC)
//!   cargo run -p mcrate-providers --example sc_play -- --full            (играть до конца, а не 20 секунд)
//! Слова с `--` это флаги, остальное поисковый запрос.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mcrate_audio::AudioPlayer;
use mcrate_core::{MusicProvider, StreamSource};
use mcrate_providers::{SoundCloudProvider, StreamDownloader, StreamFormat};

// current_thread = весь async-код работает в одном потоке; для примера этого достаточно
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\nОШИБКА: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prefer_mp3 = args.iter().any(|a| a == "--mp3");
    let full = args.iter().any(|a| a == "--full");
    let query = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "Кино Группа крови".to_string());
    let format = if prefer_mp3 { StreamFormat::ProgressiveMp3 } else { StreamFormat::HlsAac };

    // client_id храним в %APPDATA%\Mcrate\soundcloud_client_id.txt, чтобы не добывать его при каждом запуске
    let dir = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Mcrate");
    let provider = SoundCloudProvider::with_format(format)?.with_client_id_file(dir.join("soundcloud_client_id.txt"));
    println!("формат: {format:?}");

    println!("== Подготовка (client_id)");
    let started = Instant::now();
    provider.prepare().await?;
    println!("готово за {:.1} с (первый раз долго: client_id берётся с сайта; потом из файла почти мгновенно)", started.elapsed().as_secs_f32());

    println!("\n== Поиск «{query}»");
    let started = Instant::now();
    let tracks = provider.search(&query).await?;
    println!("найдено полных треков: {} (за {:.1} с)", tracks.len(), started.elapsed().as_secs_f32());
    for (i, t) in tracks.iter().take(5).enumerate() {
        println!("  #{i}: {} — {} | {} с", t.artists.join(", "), t.title, t.duration_secs);
    }
    let track = tracks.first().ok_or("ничего не найдено")?;
    println!("\nберём: {} — {}", track.artists.join(", "), track.title);

    println!("\n== Ссылка на звук");
    let source = provider.get_stream(track).await?;
    let (kind, ext) = match &source {
        StreamSource::HlsPlaylist(_) => ("HLS-плейлист", "m4a"),
        StreamSource::DirectUrl(_) => ("прямая ссылка", "mp3"),
        StreamSource::LocalFile(_) => ("локальный файл", "bin"),
    };
    println!("получили: {kind}");

    println!("\n== Скачивание");
    let path = std::env::temp_dir().join(format!("mcrate-sc-play.{ext}")); // временный файл, в репозиторий не попадает
    let downloader = StreamDownloader::new()?;
    let started = Instant::now();
    downloader.download_to_file(&source, &path).await?;
    let size = std::fs::metadata(&path)?.len();
    println!("скачано за {:.1} с: {} КБ -> {}", started.elapsed().as_secs_f32(), size / 1024, path.display());

    println!("\n== Воспроизведение");
    let player = AudioPlayer::new()?;
    let total = player.play_file(&path)?;
    println!("играет. Длина по файлу: {total:?}; по данным поиска: {} с", track.duration_secs);

    let mut seconds = 0u64;
    loop {
        std::thread::sleep(Duration::from_secs(1)); // блокирующая пауза: в этом примере больше ничего не происходит
        seconds += 1;
        if player.is_finished() {
            println!("трек закончился");
            break;
        }
        if seconds == 4 && track.duration_secs > 60 {
            match player.seek(Duration::from_secs(30)) {
                Ok(()) => println!("перемотка на 30 с: ок"),
                Err(e) => println!("перемотка на 30 с: ОШИБКА ({e})"),
            }
        }
        if seconds % 3 == 0 {
            println!("  позиция: {:.1} с", player.position().as_secs_f32());
        }
        if !full && seconds >= 20 {
            println!("20 секунд прослушано, останавливаюсь (флаг --full играет до конца)");
            break;
        }
    }
    player.stop();
    println!("\nГотово. Пришли весь вывод и скажи, что СЛЫШАЛ (звук был? чистый? перемотка сработала?).");
    Ok(())
}
