//! Проверка PlaybackEngine с SoundCloud: поиск -> очередь из 3 треков -> движок сам качает и играет.
//! Главное, что проверяем: пока трек скачивается, движок НЕ замирает.
//!
//! Что делает пример (всё само, ничего нажимать не нужно):
//!   1. Ставит очередь и во время загрузки 1-го трека жмёт паузу и меняет громкость
//!      (пауза должна отозваться сразу, а трек после загрузки стартует на паузе).
//!      Потом снимает паузу.
//!   2. Когда трек пошёл, слушает 6 с, перематывает на 30-ю секунду.
//!   3. Жмёт «следующий» (2-й трек грузится) и через 2 с ещё раз «следующий» (3-й трек).
//!      Результат загрузки 2-го трека устарел и должен быть выброшен: заиграть обязан 3-й.
//!   4. Играет 3-й трек 6 с, останавливается, показывает папку кэша.
//!
//! Запуск:
//!   cargo run -p mcrate-providers --example engine_sc
//!   cargo run -p mcrate-providers --example engine_sc -- "Кино Группа крови"
//! Второй запуск с тем же запросом быстрее: треки уже в кэше (%TEMP%\mcrate\stream).

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mcrate_audio::{PlaybackEngine, PlaybackEvent};
use mcrate_core::{MusicProvider, Track};

// current_thread = async-код примера идёт в одном потоке; у движка своя среда с потоками внутри
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\nОШИБКА: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let query = std::env::args().nth(1).unwrap_or_else(|| "Кино Группа крови".to_string());

    // ПРОВАЙДЕР-НАЧАЛО: подготовка SoundCloud и поиск
    let dir = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Mcrate");
    let sc = Arc::new(
        mcrate_providers::SoundCloudProvider::new()?.with_client_id_file(dir.join("soundcloud_client_id.txt")),
    );
    println!("== Подготовка (client_id; первый раз ~30 с на медленном интернете)");
    sc.prepare().await?;
    println!("== Поиск «{query}»");
    let found: Vec<Track> = sc.search(&query).await?;
    let provider: Arc<dyn MusicProvider> = sc; // движку нужен общий тип «любой провайдер»
    // ПРОВАЙДЕР-КОНЕЦ

    let tracks: Vec<Track> = found.into_iter().take(3).collect();
    if tracks.len() < 3 {
        return Err("нашлось меньше 3 треков, нужно минимум 3 (попробуй другой запрос)".into());
    }
    for (i, t) in tracks.iter().enumerate() {
        println!("  #{}: {} — {} | {} с", i + 1, t.artists.join(", "), t.title, t.duration_secs);
    }

    let engine = PlaybackEngine::new(vec![provider])?;
    let mut w = Watcher::new(&engine);
    println!("\n== Старт. Папка кэша: {}", PlaybackEngine::stream_cache_dir().display());
    let started_at = Instant::now();
    engine.play_queue(tracks.clone(), 0);
    if !w.wait(Duration::from_secs(10), loading(&tracks[0].id))? {
        return Err("движок не прислал событие Loading (очередь не запустилась?)".into());
    }

    // ---- Шаг 1: пауза и громкость во время загрузки ----
    println!("\n== Шаг 1: пауза и громкость во время загрузки 1-го трека");
    w.sleep(Duration::from_secs(2))?;
    if engine.state().is_loading {
        let sent = Instant::now();
        engine.set_volume(0.5);
        engine.toggle_pause();
        if w.wait(Duration::from_secs(2), |e| matches!(e, PlaybackEvent::PauseChanged(true)))? {
            println!("   ОК: пауза отозвалась за {} мс, трек всё ещё грузится", sent.elapsed().as_millis());
        } else {
            println!("   ОШИБКА: пауза не отозвалась за 2 с, движок завис на загрузке?");
        }
        w.sleep(Duration::from_secs(2))?;
        let st = engine.state();
        println!("   в снимке: is_loading={} громкость={:.2}", st.is_loading, st.volume);
        engine.toggle_pause(); // снимаем паузу: трек после загрузки должен играть
        if !w.wait(Duration::from_secs(2), |e| matches!(e, PlaybackEvent::PauseChanged(false)))? {
            println!("   ОШИБКА: снятие паузы не отозвалось");
        }
    } else {
        println!("   шаг пропущен: трек уже играет (загрузка короче 2 с, скорее всего он был в кэше)");
    }

    // ---- Шаг 2: старт и перемотка ----
    println!("\n== Шаг 2: ждём, пока 1-й трек пойдёт");
    if !w.wait(Duration::from_secs(600), started(&tracks[0].id))? {
        return Err("1-й трек так и не начал играть за 10 минут (смотри события выше)".into());
    }
    println!("   1-й трек пошёл через {:.1} с после старта. СЛУШАЙ: звук есть, громкость примерно вдвое тише", started_at.elapsed().as_secs_f32());
    w.sleep(Duration::from_secs(6))?;
    if tracks[0].duration_secs > 60 {
        engine.seek(Duration::from_secs(30));
        w.sleep(Duration::from_secs(3))?;
        println!("   после перемотки на 30 с позиция = {:.1} с (ожидаем около 33)", engine.state().position.as_secs_f32());
    }

    // ---- Шаг 3: «следующий» дважды, устаревший результат ----
    println!("\n== Шаг 3: «следующий» дважды подряд");
    engine.next();
    if !w.wait(Duration::from_secs(10), loading(&tracks[1].id))? {
        return Err("после next() нет события Loading для 2-го трека".into());
    }
    println!("   старый трек должен замолчать сразу (СЛУШАЙ), 2-й грузится");
    w.sleep(Duration::from_secs(2))?;
    if engine.state().is_loading {
        engine.next(); // 2-й трек не дождался загрузки, просим 3-й
        if !w.wait(Duration::from_secs(10), loading(&tracks[2].id))? {
            return Err("после второго next() нет события Loading для 3-го трека".into());
        }
        if !w.wait(Duration::from_secs(600), |e| matches!(e, PlaybackEvent::TrackChanged(Some(_))))? {
            return Err("после этого ни один трек не начал играть".into());
        }
        let first_started = w.last_started_id();
        if first_started.as_deref() == Some(tracks[2].id.as_str()) {
            println!("   ОК: заиграл 3-й трек, результат загрузки 2-го выброшен");
        } else {
            println!("   ОШИБКА: заиграл трек {first_started:?}, а должен был 3-й ({})", tracks[2].id);
        }
    } else {
        println!("   проверка пропущена: 2-й трек загрузился быстрее 2 с (кэш), устаревший результат не проверить");
        w.wait(Duration::from_secs(10), started(&tracks[1].id))?;
    }

    // ---- Шаг 4: доигрываем и выходим ----
    println!("\n== Шаг 4: слушаем 6 с и останавливаемся");
    w.sleep(Duration::from_secs(6))?;
    engine.stop();
    w.sleep(Duration::from_millis(500))?;
    let late_second = w.seen_started(&tracks[1].id) && w.last_started_id().as_deref() != Some(tracks[1].id.as_str());
    if late_second {
        println!("   ЗАМЕЧАНИЕ: 2-й трек всё же начинал играть (если проверка шага 3 была пропущена, это нормально)");
    }

    println!("\n== Кэш скачанных треков:");
    match std::fs::read_dir(PlaybackEngine::stream_cache_dir()) {
        Ok(entries) => {
            for e in entries.flatten() {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                println!("   {} ({} КБ)", e.file_name().to_string_lossy(), size / 1024);
            }
        }
        Err(e) => println!("   папка не читается: {e}"),
    }
    println!("\nГотово. Пришли весь вывод и скажи, что СЛЫШАЛ (звук чистый? громкость в шаге 1? старый трек замолчал сразу? перемотка?).");
    Ok(())
}

/// Условие «пришло Loading для трека с таким id».
fn loading(id: &str) -> impl Fn(&PlaybackEvent) -> bool + '_ {
    move |e| matches!(e, PlaybackEvent::Loading(t) if t.id == id)
}

/// Условие «трек с таким id начал играть».
fn started(id: &str) -> impl Fn(&PlaybackEvent) -> bool + '_ {
    move |e| matches!(e, PlaybackEvent::TrackChanged(Some(t)) if t.id == id)
}

/// Короткое описание события для печати (полный Debug трека слишком длинный).
fn describe(e: &PlaybackEvent) -> String {
    match e {
        PlaybackEvent::Loading(t) => format!("Loading «{}»", t.title),
        PlaybackEvent::TrackChanged(Some(t)) => format!("TrackChanged «{}»", t.title),
        PlaybackEvent::TrackFailed { track, reason } => format!("TrackFailed «{}»: {reason}", track.title),
        other => format!("{other:?}"),
    }
}

/// Помощник: забирает события у движка, печатает их со временем и умеет ждать нужное.
/// Все события копятся в `seen`; `cursor` показывает, до какого мы уже дочитали.
struct Watcher<'a> {
    engine: &'a PlaybackEngine,
    t0: Instant,
    seen: Vec<PlaybackEvent>,
    cursor: usize,
    failure: Option<String>, // первая ошибка трека (TrackFailed): дальше ждать бессмысленно
}

impl<'a> Watcher<'a> {
    fn new(engine: &'a PlaybackEngine) -> Self {
        Self { engine, t0: Instant::now(), seen: Vec::new(), cursor: 0, failure: None }
    }

    /// Забрать новые события у движка и напечатать.
    fn pump(&mut self) {
        for e in self.engine.poll_events() {
            println!("   [{:6.1} с] событие: {}", self.t0.elapsed().as_secs_f32(), describe(&e));
            if let PlaybackEvent::TrackFailed { track, reason } = &e {
                self.failure.get_or_insert(format!("трек «{}» не запустился: {reason}", track.title));
            }
            self.seen.push(e);
        }
    }

    /// Если какой-то трек не запустился, пример дальше не идёт (иначе ждали бы до таймаута).
    fn check(&self) -> Result<(), String> {
        match &self.failure {
            Some(msg) => Err(msg.clone()),
            None => Ok(()),
        }
    }

    /// Ждать событие по условию (среди ещё не прочитанных). Находка сдвигает `cursor` за неё.
    /// Ok(true) = дождались, Ok(false) = вышел таймаут, Err = трек не запустился.
    fn wait(&mut self, timeout: Duration, pred: impl Fn(&PlaybackEvent) -> bool) -> Result<bool, String> {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if let Some(offset) = self.seen[self.cursor..].iter().position(&pred) {
                self.cursor += offset + 1;
                return Ok(true);
            }
            self.check()?;
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Просто ждать, печатая события.
    fn sleep(&mut self, duration: Duration) -> Result<(), String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.pump();
            self.check()?;
            std::thread::sleep(Duration::from_millis(30));
        }
        self.pump();
        self.check()
    }

    /// id трека, который начал играть последним.
    fn last_started_id(&self) -> Option<String> {
        self.seen.iter().rev().find_map(|e| match e {
            PlaybackEvent::TrackChanged(Some(t)) => Some(t.id.clone()),
            _ => None,
        })
    }

    fn seen_started(&self, id: &str) -> bool {
        self.seen.iter().any(|e| started(id)(e))
    }
}
