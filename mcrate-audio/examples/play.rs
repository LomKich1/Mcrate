use mcrate_audio::AudioPlayer;
use std::path::Path;
use std::time::Duration;
use std::thread::sleep; // sleep = «подождать указанное время»

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // args() = аргументы командной строки; nth(1) = первый после имени программы
    let path = std::env::args().nth(1).expect("Укажи путь к аудиофайлу");

    let player = AudioPlayer::new()?;
    let total = player.play_file(Path::new(&path))?;
    println!("Играю: {path}, длина: {total:?}");

    sleep(Duration::from_secs(3));
    println!("Пауза на 2 секунды");
    player.pause();
    sleep(Duration::from_secs(2));
    player.resume();

    println!("Громкость 50% и перемотка на 10-ю секунду");
    player.set_volume(0.5);
    player.seek(Duration::from_secs(10))?;

    while !player.is_finished() {
        println!("позиция: {:?}", player.position());
        sleep(Duration::from_secs(1));
    }
    Ok(())
}