use mcrate_audio::PlaybackEngine;
use mcrate_core::MusicProvider;
use mcrate_providers::LocalProvider;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("Укажи пути к аудиофайлам");
        return Ok(());
    }

    let local = LocalProvider::new();
    let mut tracks = Vec::new();
    for p in &paths {
        tracks.push(local.add_file(Path::new(p))?);
    }

    // Arc<dyn MusicProvider> = общий «умный указатель» на любой провайдер
    let provider: Arc<dyn MusicProvider> = Arc::new(local);
    let engine = PlaybackEngine::new(vec![provider])?;
    engine.play_queue(tracks, 0);

    // Команды с клавиатуры, как в прошлом примере
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            if stdin.read_line(&mut line).map(|n| n == 0).unwrap_or(true) {
                break;
            }
            if tx.send(line.trim().to_string()).is_err() {
                break;
            }
        }
    });
    println!("n = следующий, p = предыдущий, t = пауза, s = shuffle, r = повтор, q = выход");

    let mut ticks = 0u32;
    loop {
        if let Ok(cmd) = rx.try_recv() {
            match cmd.as_str() {
                "n" => engine.next(),
                "p" => engine.prev(),
                "t" => engine.toggle_pause(),
                "s" => {
                    let on = engine.state().shuffle;
                    engine.set_shuffle(!on);
                }
                "r" => engine.cycle_repeat(),
                "q" => break,
                _ => {}
            }
        }

        // Так UI будет читать события каждый кадр
        for event in engine.poll_events() {
            println!("событие: {event:?}");
        }

        // Раз в 2 секунды печатаем позицию из снимка
        ticks += 1;
        if ticks % 20 == 0 {
            let st = engine.state();
            println!("позиция {:?} из {:?}", st.position, st.duration);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}