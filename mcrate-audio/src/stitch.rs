//! Бесшовный стык: следующий трек дописывается в очередь той же деки, что играет текущий.
//!
//! Очередь `rodio::Player` переходит к следующему источнику внутри одного вызова `next()`, поэтому
//! стык точный до сэмпла (проверено пробником `examples/gapless_probe.rs`). Платим за это учётом:
//! `Player` не умеет убрать один дописанный источник, не тронув играющий, и не сообщает о смене
//! трека. Здесь живёт этот учёт: список дописанных треков, отмена (отменённый источник сразу
//! возвращает конец, и очередь идёт дальше) и определение стыка по числу источников в очереди
//! (`Player::len()`). Логика чистая, без звука, поэтому тестируется напрямую.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::{ChannelCount, SampleRate, Source};

use crate::gain::TrackGain;

/// Один дописанный трек: его усиление и флаг отмены (общие с источником в очереди).
struct Entry {
    gain: TrackGain,
    cancel: Arc<AtomicBool>,
}

impl Entry {
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Дописанные в очередь деки треки, которые ещё не начали играть (текущий трек сюда не входит),
/// в порядке очереди. Отменённые остаются в списке, пока очередь до них не дойдёт: в `Player`
/// они ещё числятся, и без них счёт `len()` не сойдётся.
#[derive(Default)]
pub(crate) struct Stitch {
    entries: VecDeque<Entry>,
    /// Был стык на следующий трек, а движок о нём ещё не узнал (см. `take_transition`).
    transitioned: bool,
}

impl Stitch {
    /// Записать, что в очередь дописан трек. Звать вместе с `Player::append`.
    pub(crate) fn push(&mut self, gain: TrackGain, cancel: Arc<AtomicBool>) {
        self.entries.push_back(Entry { gain, cancel });
    }

    /// Есть ли дописанный трек, который ещё сыграет (не отменённый)?
    pub(crate) fn has_alive(&self) -> bool {
        self.entries.iter().any(|e| !e.is_cancelled())
    }

    /// Усиление для дописанного (живого) трека: он начнёт играть сразу с ним.
    pub(crate) fn set_alive_gain(&self, value: f32) {
        for entry in self.entries.iter().filter(|e| !e.is_cancelled()) {
            entry.gain.set(value);
        }
    }

    /// Отменить все ещё не начавшиеся дописанные треки. Играющий не трогаем: звать только после
    /// `reconcile`, иначе можно отменить трек, который уже начался.
    pub(crate) fn cancel_all(&mut self) {
        for entry in &self.entries {
            entry.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Очередь деки очищена целиком (или собрана заново): забыть всё.
    pub(crate) fn reset(&mut self) {
        self.cancel_all();
        self.entries.clear();
        self.transitioned = false;
    }

    /// Сверить список с очередью. `len` = `Player::len()`: играющий источник и все за ним.
    /// Если очередь дошла до дописанного трека, его убирают из списка. Возвращает усиление трека,
    /// который играет СЕЙЧАС вместо предыдущего (его и надо сделать усилением деки), либо None,
    /// если стыка не было. Пропущенные отменённые треки просто выбывают из списка.
    pub(crate) fn reconcile(&mut self, len: usize) -> Option<TrackGain> {
        let waiting = len.saturating_sub(1); // сколько источников стоит за играющим
        let consumed = self.entries.len().saturating_sub(waiting);
        let mut now_playing = None;
        for _ in 0..consumed {
            now_playing = self.entries.pop_front();
        }
        // len == 0: доиграло всё, играть нечему, это уже `is_finished`, а не стык
        match now_playing {
            Some(entry) if len >= 1 && !entry.is_cancelled() => {
                self.transitioned = true;
                Some(entry.gain)
            }
            _ => None,
        }
    }

    /// Был ли стык с прошлого вопроса (флаг после вопроса сбрасывается).
    pub(crate) fn take_transition(&mut self) -> bool {
        std::mem::take(&mut self.transitioned)
    }
}

/// Источник, который можно отменить: после флага он сразу заканчивается (и отпускает декодер),
/// очередь переходит к следующему. Метаданные запоминаются заранее: очередь спрашивает их у
/// следующего источника ещё до первого сэмпла, и у отменённого они должны остаться прежними.
pub(crate) struct CancelSource<S> {
    inner: Option<S>,
    cancel: Arc<AtomicBool>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    total: Option<Duration>,
}

impl<S> CancelSource<S>
where
    S: Source<Item = f32>,
{
    pub(crate) fn new(inner: S, cancel: Arc<AtomicBool>) -> Self {
        let (channels, sample_rate, total) = (inner.channels(), inner.sample_rate(), inner.total_duration());
        Self { inner: Some(inner), cancel, channels, sample_rate, total }
    }

    /// Отменён? Тогда сразу бросаем декодер (он держит файл открытым) и больше не играем.
    fn check_cancelled(&mut self) {
        if self.inner.is_some() && self.cancel.load(Ordering::Relaxed) {
            self.inner = None;
        }
    }
}

impl<S> Iterator for CancelSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.check_cancelled();
        self.inner.as_mut()?.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.as_ref().map_or((0, Some(0)), |s| s.size_hint())
    }
}

impl<S> Source for CancelSource<S>
where
    S: Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        match &self.inner {
            Some(_) if self.cancel.load(Ordering::Relaxed) => Some(0), // отменён: очередь сразу идёт дальше
            Some(inner) => inner.current_span_len(),
            None => Some(0),
        }
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        match self.inner.as_mut() {
            Some(inner) => inner.try_seek(position),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gain::GainSource;
    use rodio::buffer::SamplesBuffer;
    use rodio::Player;
    use std::num::NonZero;

    fn entry(stitch: &mut Stitch, value: f32) -> (TrackGain, Arc<AtomicBool>) {
        let (gain, cancel) = (TrackGain::new(value), Arc::new(AtomicBool::new(false)));
        stitch.push(gain.clone(), Arc::clone(&cancel));
        (gain, cancel)
    }

    #[test]
    fn nothing_queued_means_nothing_happens() {
        let mut stitch = Stitch::default();
        assert!(stitch.reconcile(1).is_none()); // играет один трек, за ним никого
        assert!(stitch.reconcile(0).is_none());
        assert!(!stitch.take_transition());
        assert!(!stitch.has_alive());
    }

    #[test]
    fn transition_is_reported_once_with_the_new_tracks_gain() {
        let mut stitch = Stitch::default();
        entry(&mut stitch, 0.4);
        assert!(stitch.reconcile(2).is_none(), "играет первый, второй ждёт");
        assert!(stitch.has_alive());
        let gain = stitch.reconcile(1).expect("очередь дошла до второго");
        assert_eq!(gain.get(), 0.4);
        assert!(!stitch.has_alive(), "дописанный стал играющим и ушёл из списка");
        assert!(stitch.take_transition());
        assert!(!stitch.take_transition(), "флаг сбрасывается после вопроса");
    }

    #[test]
    fn everything_finished_is_not_a_transition() {
        let mut stitch = Stitch::default();
        entry(&mut stitch, 1.0);
        assert!(stitch.reconcile(0).is_none(), "в очереди пусто: это конец, а не стык");
        assert!(!stitch.take_transition());
    }

    #[test]
    fn cancelled_track_is_skipped_and_the_next_alive_one_is_reported() {
        let mut stitch = Stitch::default();
        let (_, cancel_b) = entry(&mut stitch, 0.9);
        entry(&mut stitch, 0.5); // C дописан после отменённого B
        cancel_b.store(true, Ordering::Relaxed);
        assert!(stitch.has_alive(), "C жив");
        // Первый трек кончился, очередь прошла мёртвый B и играет C: в Player осталось 1
        let gain = stitch.reconcile(1).expect("играет C");
        assert_eq!(gain.get(), 0.5, "усиление C, а не отменённого B");
        assert!(stitch.take_transition());
    }

    #[test]
    fn only_a_cancelled_track_reached_is_not_a_transition() {
        let mut stitch = Stitch::default();
        let (_, cancel) = entry(&mut stitch, 0.9);
        cancel.store(true, Ordering::Relaxed);
        assert!(stitch.reconcile(1).is_none(), "дошли до отменённого: звука нового трека нет");
        assert!(!stitch.take_transition());
    }

    #[test]
    fn set_alive_gain_skips_cancelled_tracks() {
        let mut stitch = Stitch::default();
        let (dead, cancel) = entry(&mut stitch, 1.0);
        let (alive, _) = entry(&mut stitch, 1.0);
        cancel.store(true, Ordering::Relaxed);
        stitch.set_alive_gain(0.3);
        assert_eq!(dead.get(), 1.0);
        assert_eq!(alive.get(), 0.3);
    }

    #[test]
    fn reset_cancels_and_forgets() {
        let mut stitch = Stitch::default();
        let (_, cancel) = entry(&mut stitch, 1.0);
        stitch.reset();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!stitch.has_alive());
        assert!(stitch.reconcile(2).is_none());
    }

    // ----- на настоящем Player (выход вычитываем сами, звуковая карта не нужна) -----

    const RATE: u32 = 8000;

    fn constant(value: f32, frames: usize) -> SamplesBuffer {
        SamplesBuffer::new(NonZero::new(2u16).unwrap(), NonZero::new(RATE).unwrap(), vec![value; frames * 2])
    }

    /// Дописывает трек так же, как `AudioPlayer::queue_next_file`, но без декодера.
    fn queue(player: &Player, stitch: &mut Stitch, value: f32, frames: usize) -> Arc<AtomicBool> {
        let (gain, cancel) = (TrackGain::new(1.0), Arc::new(AtomicBool::new(false)));
        let source = CancelSource::new(GainSource::new(constant(value, frames), gain.clone()), Arc::clone(&cancel));
        stitch.push(gain, Arc::clone(&cancel));
        player.append(source);
        cancel
    }

    #[test]
    fn real_player_detects_the_boundary_by_len() {
        let (player, mut out) = Player::new();
        let mut stitch = Stitch::default();
        player.append(constant(0.5, 400)); // играющий, в списке его нет
        queue(&player, &mut stitch, 0.25, 400);
        let _: Vec<f32> = out.by_ref().take(400).collect(); // половина первого
        assert!(stitch.reconcile(player.len()).is_none());
        let first: Vec<f32> = out.by_ref().take(400).collect(); // конец первого
        let second: Vec<f32> = out.by_ref().take(10).collect(); // начало второго
        assert!(first.iter().all(|s| *s == 0.5));
        assert!(second.iter().all(|s| *s == 0.25), "стык без тишины: {second:?}");
        assert!(stitch.reconcile(player.len()).is_some(), "len={} после стыка", player.len());
    }

    #[test]
    fn real_player_skips_a_cancelled_track_and_plays_the_next_without_a_gap() {
        let (player, mut out) = Player::new();
        let mut stitch = Stitch::default();
        player.append(constant(0.5, 400));
        let cancel_b = queue(&player, &mut stitch, 0.9, 400); // B: будет отменён
        queue(&player, &mut stitch, 0.25, 400); // C: его-то и надо услышать после первого
        cancel_b.store(true, Ordering::Relaxed);
        let first: Vec<f32> = out.by_ref().take(800).collect();
        let after: Vec<f32> = out.by_ref().take(800).collect();
        assert!(first.iter().all(|s| *s == 0.5));
        assert!(after.iter().all(|s| *s == 0.25), "после первого должен сразу идти C, без звука B и без тишины: {:?}", &after[..4]);
        assert_eq!(stitch.reconcile(player.len()).map(|g| g.get()), Some(1.0));
    }
}
