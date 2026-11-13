use mcrate_audio::AudioPlayer;
use mcrate_core::{MusicProvider, StreamSource};
use mcrate_providers::LocalProvider;
use std::path::Path;
use std::time::Duration;

// #[tokio::main] делает main асинхронной: внутри можно писать .await
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("Укажи путь к аудиофайлу");

    let provider = LocalProvider::new();
    let track = provider.add_file(Path::new(&path))?;
    println!("Добавлен: {} | {:?} | {} с", track.title, track.artists, track.duration_secs);

    // .await = «дождаться результата асинхронной функции»
    let all = provider.search("").await?;
    println!("Треков в библиотеке: {}", all.len());

    match provider.get_stream(&all[0]).await? {
        StreamSource::LocalFile(p) => {
            let player = AudioPlayer::new()?;
            player.play_file(&p)?;
            std::thread::sleep(Duration::from_secs(5));
        }
        other => println!("Неожиданный источник: {other:?}"),
    }
    Ok(())
}