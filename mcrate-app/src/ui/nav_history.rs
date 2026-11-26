//! История переходов между экранами, как в браузере: «назад» и «вперёд».
//!
//! Это чистая логика без egui: история хранит места (`Place`), а `McrateApp` (ui/shortcuts.rs) каждый кадр
//! сообщает ей, где пользователь сейчас, и применяет результат `back`/`forward` к `view`. Благодаря этому
//! не нужно трогать десятки мест, где экраны переключаются присваиванием `self.view = ...`: любой переход
//! (клик по меню, кнопка «назад» в заголовке, перетаскивание файлов) попадает в историю сам.
//!
//! Правила те же, что в браузере: новый переход после шага «назад» стирает «вперёд», повтор того же места
//! историю не растит, длина ограничена.

use crate::app::View;

/// Сколько мест «назад» помним. Больше смысла нет: так глубоко по кнопке «назад» не ходят.
const MAX_BACK: usize = 64;

/// Место в приложении: экран и, для экрана плейлиста, какой именно плейлист открыт.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Place {
    pub view: View,
    /// Номер плейлиста. Только у `View::CollectionPlaylist`, у остальных экранов всегда `None`.
    pub playlist: Option<u64>,
}

impl Place {
    /// Собирает место из текущего экрана и открытого плейлиста. Номер плейлиста нужен только его экрану:
    /// иначе один и тот же экран «Библиотека» считался бы разными местами из-за забытого номера.
    pub(crate) fn new(view: View, open_playlist: Option<u64>) -> Self {
        Self { view, playlist: if view == View::CollectionPlaylist { open_playlist } else { None } }
    }
}

/// Стопки «назад» и «вперёд» вокруг текущего места.
pub(crate) struct NavHistory {
    back: Vec<Place>,
    forward: Vec<Place>,
    current: Place,
}

impl NavHistory {
    /// История, в которой пользователь уже стоит на `start`.
    pub(crate) fn new(start: Place) -> Self {
        Self { back: Vec::new(), forward: Vec::new(), current: start }
    }

    /// Сообщает, где пользователь сейчас. Если место новое, прежнее уходит в «назад», а «вперёд» стирается.
    /// Вызывать каждый кадр: одинаковое место ничего не меняет.
    pub(crate) fn observe(&mut self, place: Place) {
        if place == self.current {
            return;
        }
        self.back.push(self.current);
        if self.back.len() > MAX_BACK {
            self.back.remove(0);
        }
        self.forward.clear();
        self.current = place;
    }

    /// Шаг «назад»: возвращает место, на которое надо перейти. `is_alive` отбрасывает места, которых больше нет
    /// (удалённый плейлист): они пропускаются, а не открываются пустыми. `None`, если идти некуда.
    pub(crate) fn back(&mut self, is_alive: impl Fn(Place) -> bool) -> Option<Place> {
        let target = Self::pop_alive(&mut self.back, &is_alive)?;
        self.forward.push(self.current);
        self.current = target;
        Some(target)
    }

    /// Шаг «вперёд»: то же, что `back`, но в другую сторону.
    pub(crate) fn forward(&mut self, is_alive: impl Fn(Place) -> bool) -> Option<Place> {
        let target = Self::pop_alive(&mut self.forward, &is_alive)?;
        self.back.push(self.current);
        self.current = target;
        Some(target)
    }

    /// Место, на которое вернёт «назад» (без перехода). Нужно «Сейчас играет»: пока он открыт, в меню подсвечен
    /// экран под ним.
    pub(crate) fn previous(&self) -> Option<Place> {
        self.back.last().copied()
    }

    /// Достаёт из стопки ближайшее живое место, выкидывая мёртвые по пути.
    fn pop_alive(stack: &mut Vec<Place>, is_alive: &impl Fn(Place) -> bool) -> Option<Place> {
        while let Some(place) = stack.pop() {
            if is_alive(place) {
                return Some(place);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(view: View) -> Place {
        Place::new(view, None)
    }

    fn playlist(id: u64) -> Place {
        Place::new(View::CollectionPlaylist, Some(id))
    }

    fn alive(_: Place) -> bool {
        true
    }

    #[test]
    fn empty_history_has_nowhere_to_go() {
        let mut h = NavHistory::new(at(View::Home));
        assert!(h.back(alive).is_none());
        assert!(h.forward(alive).is_none());
        assert!(h.previous().is_none());
    }

    #[test]
    fn back_and_forward_walk_the_path() {
        let mut h = NavHistory::new(at(View::Home));
        h.observe(at(View::Collection));
        h.observe(at(View::CollectionLiked));
        assert!(h.back(alive) == Some(at(View::Collection)));
        assert!(h.back(alive) == Some(at(View::Home)));
        assert!(h.back(alive).is_none());
        assert!(h.forward(alive) == Some(at(View::Collection)));
        assert!(h.forward(alive) == Some(at(View::CollectionLiked)));
        assert!(h.forward(alive).is_none());
    }

    #[test]
    fn same_place_is_not_recorded_twice() {
        let mut h = NavHistory::new(at(View::Home));
        h.observe(at(View::Home));
        h.observe(at(View::Home));
        assert!(h.back(alive).is_none());
    }

    #[test]
    fn new_navigation_clears_forward() {
        let mut h = NavHistory::new(at(View::Home));
        h.observe(at(View::Collection));
        assert!(h.back(alive) == Some(at(View::Home)));
        h.observe(at(View::SoundCloud));
        assert!(h.forward(alive).is_none());
        assert!(h.back(alive) == Some(at(View::Home)));
    }

    #[test]
    fn going_back_is_not_recorded_as_new_navigation() {
        // После `back` приложение ставит `view` в найденное место, и следующий `observe` видит то же место.
        let mut h = NavHistory::new(at(View::Home));
        h.observe(at(View::Collection));
        let target = h.back(alive).unwrap();
        h.observe(target);
        assert!(h.forward(alive) == Some(at(View::Collection)), "«вперёд» не должен пропасть");
    }

    #[test]
    fn dead_places_are_skipped() {
        let mut h = NavHistory::new(at(View::Home));
        h.observe(playlist(7));
        h.observe(at(View::CollectionPlaylists));
        let not_seven = |p: Place| p.playlist != Some(7);
        assert!(h.back(not_seven) == Some(at(View::Home)), "удалённый плейлист пропущен");
        assert!(h.forward(not_seven) == Some(at(View::CollectionPlaylists)));
    }

    #[test]
    fn playlist_number_matters_only_on_playlist_screen() {
        assert!(Place::new(View::Library, Some(3)) == at(View::Library));
        assert!(playlist(1) != playlist(2));
    }

    #[test]
    fn different_playlists_are_different_places() {
        let mut h = NavHistory::new(playlist(1));
        h.observe(playlist(2));
        assert!(h.back(alive) == Some(playlist(1)));
    }

    #[test]
    fn previous_shows_where_back_leads() {
        let mut h = NavHistory::new(at(View::Home));
        h.observe(at(View::NowPlaying));
        assert!(h.previous() == Some(at(View::Home)));
    }

    #[test]
    fn back_stack_is_bounded() {
        let mut h = NavHistory::new(at(View::Home));
        for i in 0..(MAX_BACK as u64 * 2) {
            h.observe(playlist(i));
        }
        let mut steps = 0;
        while h.back(alive).is_some() {
            steps += 1;
        }
        assert_eq!(steps, MAX_BACK);
    }
}
