//! Хаб обложек: единое место, куда интерфейс приходит за обложкой трека.
//!
//! Что делает: по ключу обложки (`mcrate_providers::cover_key`) и размеру запускает загрузку в фоне (её ведёт
//! `CoverStore`: сеть, кэш на диске, теги файлов, разбор картинки), помнит результат и сообщает окну, что пора перерисоваться.
//! Хаб ничего не знает про egui и текстуры: он отдаёт готовые пиксели (`CoverImage`), а в текстуры их превращает
//! `ui/widgets/cover_images.rs`.
//!
//! Правила:
//! - один и тот же ключ грузится один раз, сколько бы строк его ни просило (треки одного альбома делят картинку);
//! - у трека нет обложки (в файле и рядом пусто): запоминаем до конца работы и больше не проверяем;
//! - сеть не ответила: пробуем снова не раньше, чем через `RETRY_AFTER`, иначе каждый кадр был бы новый запрос;
//! - готовые картинки хранятся в пределах `MAX_READY_BYTES`, дальше вытесняются самые давние по показу.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use mcrate_core::{ProviderError, ProviderResult, Track};
use mcrate_providers::{CoverImage, CoverSize, CoverStore, cover_key};

/// Через сколько секунд после неудачной загрузки пробуем ту же обложку снова.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Сколько байт пикселей хаб держит готовыми. Маленькая обложка около 100 КБ, большая около 1 МБ.
const MAX_READY_BYTES: usize = 48 * 1024 * 1024;
/// Сколько обложек оставляем на диске (см. `CoverStore::prune_cache`): около 50 КБ каждая, то есть до 100 МБ.
pub(crate) const MAX_CACHED_FILES: usize = 2000;

/// Что хаб знает про обложку трека прямо сейчас.
pub(crate) enum CoverState {
    /// Обложки у трека нет (нет адреса, в файле и рядом с ним пусто): рисуем заглушку.
    Missing,
    /// Загрузка идёт.
    Pending,
    /// Загрузить не удалось (сеть, битая картинка): заглушка, повтор позже.
    Failed,
    /// Готова.
    Ready(Arc<CoverImage>),
}

/// Ключ записи: что за картинка и какого размера.
type SlotKey = (String, CoverSize);

enum Slot {
    Pending,
    /// Обложки нет вообще (`ProviderError::Unsupported`): не перепроверяем.
    Absent,
    /// Не вышло в этот момент.
    Failed(Instant),
    /// Готова; `used` это номер последнего показа (по нему вытесняем).
    Ready {
        image: Arc<CoverImage>,
        used: u64,
    },
}

#[derive(Default)]
struct Inner {
    slots: HashMap<SlotKey, Slot>,
    /// Счётчик обращений: растёт на каждое `get`, нужен вытеснению.
    tick: u64,
    /// Сколько байт пикселей лежит в `Slot::Ready`.
    ready_bytes: usize,
}

pub(crate) struct CoverHub {
    store: Arc<CoverStore>,
    runtime: tokio::runtime::Handle,
    inner: Mutex<Inner>,
    /// Просьба окну перерисоваться; ставится один раз при запуске (окно создаётся позже хаба).
    repaint: OnceLock<Box<dyn Fn() + Send + Sync>>,
    retry_after: Duration,
    max_ready_bytes: usize,
}

impl CoverHub {
    /// `runtime` это среда tokio контроллера: загрузки идут на ней (там же живёт HTTP-клиент, см. `Controller`).
    pub(crate) fn new(store: Arc<CoverStore>, runtime: tokio::runtime::Handle) -> Arc<Self> {
        Self::with_limits(store, runtime, RETRY_AFTER, MAX_READY_BYTES)
    }

    fn with_limits(store: Arc<CoverStore>, runtime: tokio::runtime::Handle, retry_after: Duration, max_ready_bytes: usize) -> Arc<Self> {
        Arc::new(Self { store, runtime, inner: Mutex::default(), repaint: OnceLock::new(), retry_after, max_ready_bytes })
    }

    /// Что вызвать, когда обложка догрузилась (обычно `ctx.request_repaint()`). Второй вызов игнорируется.
    pub(crate) fn set_repaint(&self, repaint: impl Fn() + Send + Sync + 'static) {
        let _ = self.repaint.set(Box::new(repaint));
    }

    /// Обложка трека в нужном размере. Если её ещё не просили (или пора повторить), запускает загрузку в фоне.
    pub(crate) fn get(self: &Arc<Self>, track: &Track, size: CoverSize) -> CoverState {
        let Some(key) = cover_key(track) else { return CoverState::Missing };
        let slot_key = (key, size);
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        match inner.slots.get_mut(&slot_key) {
            Some(Slot::Ready { image, used }) => {
                *used = tick;
                return CoverState::Ready(Arc::clone(image));
            }
            Some(Slot::Pending) => return CoverState::Pending,
            Some(Slot::Absent) => return CoverState::Missing,
            Some(Slot::Failed(at)) if at.elapsed() < self.retry_after => return CoverState::Failed,
            _ => {} // записи нет или пора повторить
        }
        inner.slots.insert(slot_key.clone(), Slot::Pending);
        drop(inner);
        let hub = Arc::clone(self);
        let track = track.clone();
        self.runtime.spawn(async move {
            let result = hub.store.load(&track, size).await;
            hub.finish(slot_key, result);
        });
        CoverState::Pending
    }

    /// Готовая обложка без запроса: `None`, если её сейчас нет. Нужна, чтобы пока грузится большая обложка,
    /// показывать уже готовую маленькую вместо пустой заглушки.
    pub(crate) fn peek(&self, track: &Track, size: CoverSize) -> Option<Arc<CoverImage>> {
        let key = (cover_key(track)?, size);
        match self.lock().slots.get(&key) {
            Some(Slot::Ready { image, .. }) => Some(Arc::clone(image)),
            _ => None,
        }
    }

    /// Загрузка закончилась: записываем итог, вытесняем лишнее, просим окно перерисоваться.
    fn finish(&self, key: SlotKey, result: ProviderResult<CoverImage>) {
        {
            let mut inner = self.lock();
            let tick = inner.tick;
            let slot = match result {
                Ok(image) => {
                    inner.ready_bytes += image.rgba.len();
                    Slot::Ready { image: Arc::new(image), used: tick }
                }
                Err(ProviderError::Unsupported) => Slot::Absent,
                Err(_) => Slot::Failed(Instant::now()),
            };
            inner.slots.insert(key.clone(), slot);
            self.evict_over_budget(&mut inner, &key);
        }
        if let Some(repaint) = self.repaint.get() {
            repaint();
        }
    }

    /// Пока готовых картинок больше бюджета, выбрасывает самую давно показанную (кроме только что пришедшей `keep`).
    fn evict_over_budget(&self, inner: &mut Inner, keep: &SlotKey) {
        while inner.ready_bytes > self.max_ready_bytes {
            let oldest = inner
                .slots
                .iter()
                .filter(|(key, _)| *key != keep)
                .filter_map(|(key, slot)| match slot {
                    Slot::Ready { used, .. } => Some((*used, key.clone())),
                    _ => None,
                })
                .min_by_key(|(used, _)| *used);
            let Some((_, victim)) = oldest else { break };
            if let Some(Slot::Ready { image, .. }) = inner.slots.remove(&victim) {
                inner.ready_bytes -= image.rgba.len();
            }
        }
    }

    /// Замок состояния. Отравленный замок (другой поток упал, держа его) не повод ронять интерфейс: данные хаба это только кэш.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};
    use mcrate_core::ProviderKind;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn png(side: u32) -> Vec<u8> {
        let mut out = Vec::new();
        RgbaImage::from_pixel(side, side, Rgba([10, 200, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mcrate-hub-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Свой трек с картинкой `cover.png` рядом (если `side` задан).
    fn local_track(dir: &Path, name: &str, side: Option<u32>) -> Track {
        let sub = dir.join(name);
        std::fs::create_dir_all(&sub).unwrap();
        let song = sub.join("song.mp3");
        std::fs::write(&song, b"not audio").unwrap();
        if let Some(side) = side {
            std::fs::write(sub.join("cover.png"), png(side)).unwrap();
        }
        Track {
            id: song.to_string_lossy().into_owned(),
            provider: ProviderKind::Local,
            title: name.into(),
            artists: vec![],
            duration_secs: 1,
            cover_url: None,
        }
    }

    struct Fixture {
        rt: tokio::runtime::Runtime,
        hub: Arc<CoverHub>,
    }

    fn fixture(retry_after: Duration, max_bytes: usize) -> Fixture {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let store = {
            let _guard = rt.enter();
            Arc::new(CoverStore::new(None).unwrap())
        };
        let hub = CoverHub::with_limits(store, rt.handle().clone(), retry_after, max_bytes);
        Fixture { rt, hub }
    }

    /// Ждёт (до 5 секунд), пока обложка перестанет быть `Pending`.
    fn settle(hub: &Arc<CoverHub>, track: &Track, size: CoverSize) -> CoverState {
        for _ in 0..500 {
            match hub.get(track, size) {
                CoverState::Pending => std::thread::sleep(Duration::from_millis(10)),
                other => return other,
            }
        }
        panic!("обложка так и не загрузилась");
    }

    #[test]
    fn cover_loads_in_background_and_is_shared() {
        let dir = temp_dir("ready");
        let fx = fixture(RETRY_AFTER, MAX_READY_BYTES);
        let track = local_track(&dir, "a", Some(300));
        assert!(matches!(fx.hub.get(&track, CoverSize::Small), CoverState::Pending), "первый запрос запускает загрузку");
        let CoverState::Ready(first) = settle(&fx.hub, &track, CoverSize::Small) else { panic!("должна быть готова") };
        assert_eq!(first.size, [160, 160]);
        let CoverState::Ready(second) = fx.hub.get(&track, CoverSize::Small) else { panic!("должна быть готова") };
        assert!(Arc::ptr_eq(&first, &second), "повторный запрос отдаёт ту же картинку, а не грузит заново");
        assert!(fx.hub.peek(&track, CoverSize::Small).is_some());
        assert!(fx.hub.peek(&track, CoverSize::Large).is_none(), "другой размер это другая запись");
        drop(fx.rt);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn track_without_any_source_is_missing_at_once() {
        let fx = fixture(RETRY_AFTER, MAX_READY_BYTES);
        let online = Track { cover_url: None, ..local_track(&temp_dir("nosrc"), "x", None) };
        let online = Track { provider: ProviderKind::SoundCloud, ..online };
        assert!(matches!(fx.hub.get(&online, CoverSize::Small), CoverState::Missing));
        assert!(fx.hub.lock().slots.is_empty(), "для трека без адреса записи не заводим");
    }

    #[test]
    fn absent_cover_is_remembered_and_not_rechecked() {
        let dir = temp_dir("absent");
        let fx = fixture(Duration::ZERO, MAX_READY_BYTES);
        let track = local_track(&dir, "bare", None);
        assert!(matches!(settle(&fx.hub, &track, CoverSize::Small), CoverState::Missing));
        // Картинка появилась уже после проверки: в этом сеансе её не ждём (иначе каждая строка списка читала бы теги на каждый кадр).
        std::fs::write(dir.join("bare").join("cover.png"), png(50)).unwrap();
        assert!(matches!(fx.hub.get(&track, CoverSize::Small), CoverState::Missing));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failure_is_retried_only_after_cooldown() {
        let dead = Track {
            id: "1".into(),
            provider: ProviderKind::SoundCloud,
            title: "t".into(),
            artists: vec![],
            duration_secs: 1,
            cover_url: Some("http://127.0.0.1:1/none.png".into()), // порт 1: соединение сразу отклоняется
        };
        let fx = fixture(Duration::from_millis(300), MAX_READY_BYTES);
        assert!(matches!(settle(&fx.hub, &dead, CoverSize::Small), CoverState::Failed));
        assert!(matches!(fx.hub.get(&dead, CoverSize::Small), CoverState::Failed), "до конца паузы повторного запроса нет");
        std::thread::sleep(Duration::from_millis(400));
        assert!(matches!(fx.hub.get(&dead, CoverSize::Small), CoverState::Pending), "пауза прошла: идёт повторная попытка");
    }

    #[test]
    fn oldest_shown_cover_is_evicted_over_budget() {
        let dir = temp_dir("evict");
        // Бюджет на две маленькие обложки (160x160x4 = 102 400 байт каждая).
        let fx = fixture(RETRY_AFTER, 2 * 102_400);
        let tracks: Vec<Track> = (0..3).map(|i| local_track(&dir, &format!("t{i}"), Some(300))).collect();
        for t in &tracks[..2] {
            assert!(matches!(settle(&fx.hub, t, CoverSize::Small), CoverState::Ready(_)));
        }
        assert!(matches!(fx.hub.get(&tracks[0], CoverSize::Small), CoverState::Ready(_))); // t0 показана позже t1
        assert!(matches!(settle(&fx.hub, &tracks[2], CoverSize::Small), CoverState::Ready(_)));
        assert!(fx.hub.peek(&tracks[1], CoverSize::Small).is_none(), "давно не показанная t1 вытеснена");
        assert!(fx.hub.peek(&tracks[0], CoverSize::Small).is_some());
        assert!(fx.hub.peek(&tracks[2], CoverSize::Small).is_some(), "только что пришедшую не вытесняем");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn repaint_is_requested_when_cover_arrives() {
        let dir = temp_dir("repaint");
        let fx = fixture(RETRY_AFTER, MAX_READY_BYTES);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        fx.hub.set_repaint(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        let track = local_track(&dir, "r", Some(100));
        let _ = settle(&fx.hub, &track, CoverSize::Small);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
