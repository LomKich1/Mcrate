use mcrate_audio::{AudioPlayer, PlayQueue};
use std::path::PathBuf;
use std::sync::mpsc; // mpsc = канал «много отправителей, один получатель»
use std::thread;
use std::time::Duration;

/// Запустить трек, который сейчас выбран в очереди.
fn play_current(
    player: &AudioPlayer,
    queue: &PlayQueue<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = queue.current() {
        let total = player.play_file(path)?;
        println!("> {} ({:?})", path.display(), total);
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // skip(1) пропускает имя программы; остальное это пути к файлам
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if files.is_empty() {
        eprintln!("Укажи пути к аудиофайлам (можно один и тот же несколько раз)");
        return Ok(());
    }

    let player = AudioPlayer::new()?;
    let mut queue = PlayQueue::new();
    queue.set(files, 0);

    // Отдельный поток читает команды с клавиатуры и шлёт их в главный цикл через канал.
    // Иначе чтение ввода заблокировало бы воспроизведение.
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            // read_line вернёт 0 байт, когда ввод закрыт
            if stdin.read_line(&mut line).map(|n| n == 0).unwrap_or(true) {
                break;
            }
            if tx.send(line.trim().to_string()).is_err() {
                break;
            }
        }
    });

    println!("Команды (набери букву и Enter): n = следующий, p = предыдущий,");
    println!("t = пауза/продолжить, s = перемешивание вкл/выкл, r = повтор (выкл/все/один), q = выход");
    play_current(&player, &queue)?;

    loop {
        // try_recv не ждёт: если команды нет, идём дальше
        if let Ok(cmd) = rx.try_recv() {
            match cmd.as_str() {
                "n" => {
                    if queue.next().is_some() {
                        play_current(&player, &queue)?;
                    } else {
                        println!("Конец очереди");
                    }
                }
                "p" => {
                    queue.prev();
                    play_current(&player, &queue)?;
                }
                "t" => {
                    if player.is_paused() { player.resume(); } else { player.pause(); }
                }
                "s" => {
                    queue.set_shuffle(!queue.shuffle());
                    println!("Перемешивание: {}", queue.shuffle());
                }
                "r" => {
                    queue.set_repeat(queue.repeat().cycle());
                    println!("Повтор: {:?}", queue.repeat());
                }
                "q" => break,
                _ => {}
            }
        }

        // Трек доиграл сам? Берём следующий по правилам очереди.
        if player.is_finished() {
            if queue.track_finished().is_some() {
                play_current(&player, &queue)?;
            } else {
                println!("Очередь закончилась");
                break;
            }
        }

        thread::sleep(Duration::from_millis(100)); // не крутим цикл вхолостую
    }
    Ok(())
}