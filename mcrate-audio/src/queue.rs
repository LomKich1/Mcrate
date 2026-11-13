//! Очередь воспроизведения: что играть дальше, повтор, перемешивание.
//! Чистая логика без звука, поэтому тестируется без звуковой карты.

use std::time::{SystemTime, UNIX_EPOCH};

/// Режим повтора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off, // играть до конца очереди и остановиться
    All, // после последнего трека идти к первому
    One, // повторять один и тот же трек
}

impl RepeatMode {
    /// Переключение по кругу: Off -> All -> One -> Off.
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// Очередь элементов типа T (T может быть путём к файлу, треком и т.д.).
pub struct PlayQueue<T> {
    items: Vec<T>,     // сами элементы, порядок добавления не меняется
    order: Vec<usize>, // порядок проигрывания: номера элементов из items
    pos: usize,        // где мы сейчас внутри order
    repeat: RepeatMode,
    shuffle: bool,
    rng_state: u64,    // состояние генератора случайных чисел
}

impl<T> PlayQueue<T> {
    /// Новая очередь; генератор засеивается текущим временем.
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Self::with_seed(seed)
    }

    /// Очередь с заданным «зерном» случайности (нужно тестам: результат повторяется).
    pub fn with_seed(seed: u64) -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            pos: 0,
            repeat: RepeatMode::Off,
            shuffle: false,
            rng_state: seed | 1, // `| 1` гарантирует, что состояние не равно нулю
        }
    }

    /// Маленький генератор случайных чисел (xorshift).
    fn next_random(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13; // ^ = XOR, << и >> = битовые сдвиги
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    /// Пересобрать порядок проигрывания.
    /// `current_item` = номер элемента в items, который играет сейчас.
    fn rebuild_order(&mut self, current_item: Option<usize>) {
        let n = self.items.len();
        self.order = (0..n).collect(); // 0, 1, 2, ... n-1

        if self.shuffle && n > 1 {
            // Тасование Фишера-Йетса: с конца меняем каждый элемент со случайным из оставшихся
            for i in (1..n).rev() {
                let j = (self.next_random() % (i as u64 + 1)) as usize;
                self.order.swap(i, j);
            }
            // Играющий трек ставим первым, чтобы включение shuffle не прерывало его
            if let Some(cur) = current_item {
                if let Some(p) = self.order.iter().position(|&x| x == cur) {
                    self.order.swap(0, p);
                }
            }
            self.pos = 0;
        } else {
            self.pos = current_item.unwrap_or(0);
        }
    }

    /// Заменить очередь целиком; `start` = с какого элемента начать.
    pub fn set(&mut self, items: Vec<T>, start: usize) {
        self.items = items;
        let start = start.min(self.items.len().saturating_sub(1)); // не выходим за границы
        let current = if self.items.is_empty() { None } else { Some(start) };
        self.rebuild_order(current);
    }

    /// Добавить в конец очереди.
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.order.push(self.items.len() - 1);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.pos = 0;
    }

    pub fn len(&self) -> usize { self.items.len() }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }

    /// Что играет сейчас.
    pub fn current(&self) -> Option<&T> {
        // `&i` в замыкании сразу берёт число из ссылки
        self.order.get(self.pos).and_then(|&i| self.items.get(i))
    }

    /// Пользователь нажал «следующий». В конце очереди при повторе Off возвращает None.
    pub fn next(&mut self) -> Option<&T> {
        if self.order.is_empty() {
            return None;
        }
        if self.pos + 1 < self.order.len() {
            self.pos += 1;
        } else if self.repeat == RepeatMode::All {
            self.pos = 0;
        } else {
            return None; // конец очереди, остаёмся на последнем
        }
        self.current()
    }

    /// Пользователь нажал «предыдущий». В начале очереди остаёмся на первом
    /// (плеер просто перезапустит его).
    pub fn prev(&mut self) -> Option<&T> {
        if self.order.is_empty() {
            return None;
        }
        if self.pos > 0 {
            self.pos -= 1;
        } else if self.repeat == RepeatMode::All {
            self.pos = self.order.len() - 1;
        }
        self.current()
    }

    /// Трек доиграл сам. При повторе One играем его снова, иначе идём дальше.
    pub fn track_finished(&mut self) -> Option<&T> {
        if self.repeat == RepeatMode::One {
            self.current()
        } else {
            self.next()
        }
    }

    pub fn repeat(&self) -> RepeatMode { self.repeat }
    pub fn set_repeat(&mut self, mode: RepeatMode) { self.repeat = mode; }
    pub fn shuffle(&self) -> bool { self.shuffle }

    /// Включить или выключить перемешивание. Играющий сейчас трек не меняется.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        let current = self.order.get(self.pos).copied(); // номер текущего элемента в items
        self.shuffle = on;
        self.rebuild_order(current);
    }
}

impl<T> Default for PlayQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Очередь из чисел 1..=n, стартуем с первого
    fn queue_of(n: i32) -> PlayQueue<i32> {
        let mut q = PlayQueue::with_seed(42);
        q.set((1..=n).collect(), 0);
        q
    }

    #[test]
    fn next_walks_forward_then_stops() {
        let mut q = queue_of(3);
        assert_eq!(q.current(), Some(&1));
        assert_eq!(q.next(), Some(&2));
        assert_eq!(q.next(), Some(&3));
        assert_eq!(q.next(), None);        // конец очереди
        assert_eq!(q.current(), Some(&3)); // остались на последнем
    }

    #[test]
    fn repeat_all_wraps_around() {
        let mut q = queue_of(2);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.next(), Some(&2));
        assert_eq!(q.next(), Some(&1)); // по кругу
        assert_eq!(q.prev(), Some(&2)); // и назад по кругу
    }

    #[test]
    fn repeat_one_only_affects_auto_advance() {
        let mut q = queue_of(3);
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.track_finished(), Some(&1)); // доиграл: снова первый
        assert_eq!(q.next(), Some(&2));           // кнопка «следующий» работает
    }

    #[test]
    fn prev_at_start_stays_on_first() {
        let mut q = queue_of(3);
        assert_eq!(q.prev(), Some(&1));
    }

    #[test]
    fn shuffle_keeps_current_and_plays_every_track_once() {
        let mut q = queue_of(10);
        q.next();
        q.next(); // сейчас играет 3
        q.set_shuffle(true);
        assert_eq!(q.current(), Some(&3)); // текущий не сменился

        let mut seen = vec![*q.current().unwrap()];
        while let Some(&x) = q.next() {
            seen.push(x);
        }
        seen.sort();
        assert_eq!(seen, (1..=10).collect::<Vec<_>>()); // каждый ровно по разу
    }

    #[test]
    fn unshuffle_restores_natural_order_at_current_track() {
        let mut q = queue_of(5);
        q.set_shuffle(true);
        let cur = *q.current().unwrap();
        q.set_shuffle(false);
        assert_eq!(q.current(), Some(&cur));
        assert_eq!(q.next(), Some(&(cur + 1)).filter(|_| cur < 5)); // следом идёт соседний по номеру
    }

    #[test]
    fn empty_queue_is_safe() {
        let mut q: PlayQueue<i32> = PlayQueue::with_seed(1);
        assert_eq!(q.current(), None);
        assert_eq!(q.next(), None);
        assert_eq!(q.prev(), None);
        assert_eq!(q.track_finished(), None);
    }
}