//! Главная. Раскладка по образцу скриншотов, которые прислал пользователь: сверху «Моя волна» с анимированной
//! волной из точек, ниже горизонтальные полки со стрелками, в конце список «чарта».
//!
//! Блоки сверху вниз:
//!   1. поиск (по Enter открывает «Мою музыку» с этим запросом);
//!   2. «Моя волна»: заголовок, кнопки «запустить» и «перемешать», волна из точек (бежит, пока играет музыка);
//!   3. полка «Быстрый выбор»: недавно добавленные треки;
//!   4. полка «Ваши исполнители»: исполнители из локальной библиотеки, клик включает все их треки;
//!   5. «Мои треки»: пронумерованный список с обложками и длительностью.
//!
//! Блоков «Для вас», «Новые релизы», «Популярные артисты» и настоящего чарта здесь нет: у ядра пока нет ни подборок,
//! ни обложек, ни истории (см. AUDIT_UI_vs_core.md). Когда они появятся, это ещё одна полка через `shelf`.
//! Волна всегда анимирована и реагирует на курсор (притяжение, пузырь, рябь, кольцо от клика). К звуку пока не
//! привязана: когда у движка появится спектр (spectrum_request_for_LomKich.md), амплитуду можно взять оттуда.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use egui::{
    Align, Color32, CornerRadius, CursorIcon, FontId, Frame, Key, Layout, Margin, Painter, Rect, Response, RichText, ScrollArea,
    Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use mcrate_core::Track;

use super::widgets::{ButtonKind, artists_text, cover_tile, icon_button, pill_button, track_row};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

const WAVE_H: f32 = 200.0; // высота волны
const SHELF_GAP: f32 = 12.0; // зазор между карточками на полке
const SHELF_STEP: f32 = 420.0; // на сколько пикселей прокручивает стрелка
const QUICK_W: f32 = 160.0; // карточка «Быстрого выбора» (M3 card: обложка 144 + подпись)
const QUICK_H: f32 = 214.0;
const ARTIST_W: f32 = 144.0; // исполнитель: круглый аватар 128 + подпись по центру
const ARTIST_H: f32 = 190.0;
const QUICK_COUNT: usize = 12; // сколько треков на полке «Быстрый выбор»
const ARTISTS_COUNT: usize = 12; // сколько исполнителей на полке
const CHART_COUNT: usize = 10; // строк в списке «Мои треки»

impl McrateApp {
    pub fn home_view(&mut self, ui: &mut Ui, theme: &Theme) {
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ---------- поиск ----------
            // M3 Search bar: «таблетка» высотой 56, заливка surface_container_high, значок слева, без рамки.
            let search = {
                let p = &theme.palette;
                Frame::new().fill(p.surface_container_high).corner_radius(CornerRadius::same(28)).inner_margin(Margin::symmetric(20, 0)).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), 56.0));
                    // Убираем рамку и заливку самого поля: их рисует контейнер выше.
                    let v = ui.visuals_mut();
                    v.extreme_bg_color = Color32::TRANSPARENT;
                    v.selection.stroke = Stroke::NONE;
                    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                        w.bg_fill = Color32::TRANSPARENT;
                        w.weak_bg_fill = Color32::TRANSPARENT;
                        w.bg_stroke = Stroke::NONE;
                    }
                    ui.horizontal(|ui| {
                        ui.set_min_height(56.0);
                        let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                        icons::paint(ui.painter(), Icon::Search, icon_rect.center(), 24.0, p.on_surface_variant);
                        ui.add_space(8.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("Поиск по названию и исполнителю")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(0, 0)),
                        )
                    })
                    .inner
                })
                .inner
            };
            // Enter в поле: результаты показывает экран «Моя музыка» (там же будут и сервисы).
            if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.search.trim().is_empty() {
                self.view = View::Library;
            }
            ui.add_space(18.0);

            self.home_wave(ui, theme);

            if self.ctl.library.is_empty() {
                ui.add_space(24.0);
                self.home_empty(ui, theme);
            } else {
                ui.add_space(28.0);
                self.home_quick(ui, theme);
                ui.add_space(28.0);
                self.home_artists(ui, theme);
                ui.add_space(28.0);
                self.home_chart(ui, theme);
            }
            ui.add_space(16.0 + self.bottom_inset); // подушка под плавающую панель плеера (Liquid Glass)
        });
    }

    /// «Моя волна»: заголовок, две кнопки и анимированная волна из точек.
    fn home_wave(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let playing = self.ctl.is_playing();
        let has_music = !self.ctl.library.is_empty();
        // Время берём у основного контекста (в закадровом проходе стекла свои часы).
        let now = if self.frame_time > 0.0 { self.frame_time } else { ui.input(|i| i.time) };
        let wave_id = ui.scope_id().with("wave_state");
        let mut st: WaveState = ui.ctx().data_mut(|d| d.get_temp(wave_id)).unwrap_or_default();
        // Анимация идёт ВСЕГДА (и на паузе, и без музыки); музыка только добавляет размах.
        let amp = ui.ctx().animate_value_with_time(wave_id.with("amp"), if playing { 1.0 } else { 0.7 }, 0.6);
        ui.ctx().request_repaint();

        // Шапка: текст слева, кнопки справа.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let text_rect = Rect::from_min_max(head.min, pos2((head.right() - 140.0).max(head.left() + 120.0), head.bottom()));
        ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
            ui.add(egui::Label::new(RichText::new("Моя волна").size(34.0).strong()).truncate());
            ui.add(egui::Label::new(RichText::new("Ваша музыка вперемешку: нажмите ▶ и слушайте").size(14.0).color(p.on_surface_variant)).truncate());
        });
        let btn_rect = Rect::from_min_max(pos2(head.right() - 120.0, head.top()), head.max);
        ui.scope_builder(UiBuilder::new().max_rect(btn_rect).layout(Layout::right_to_left(Align::Center)), |ui| {
            let (icon, tip) = if playing { (Icon::Pause, "Пауза") } else { (Icon::Play, "Запустить волну") };
            if icon_button(ui, theme, icon, 52.0, ButtonKind::Filled, false, tip).clicked() && has_music {
                if self.ctl.now.is_some() {
                    self.ctl.toggle_pause();
                } else {
                    self.shuffle_all();
                }
            }
            if icon_button(ui, theme, Icon::Shuffle, 40.0, ButtonKind::Tonal, false, "Перемешать заново").clicked() && has_music {
                self.shuffle_all();
            }
        });

        // Холст волны слушает курсор (hover) и клик.
        let (canvas, resp) = ui.allocate_exact_size(vec2(ui.available_width(), WAVE_H), Sense::click());
        st.update(now, resp.hover_pos().map(|p| p - canvas.min), resp.clicked());
        ui.ctx().data_mut(|d| d.insert_temp(wave_id, st));
        let look = WaveLook { time: now, amp, base: p.on_surface, accent: p.primary };
        paint_wave(ui.painter(), canvas, &look, &st);
    }

    /// Пустая библиотека: подсказка вместо полок.
    fn home_empty(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.label(RichText::new("Здесь пока пусто").size(22.0).strong());
        ui.add_space(4.0);
        ui.label(RichText::new("Перетащите файлы в окно или добавьте музыку на экране «Моя музыка».").color(p.on_surface_variant));
        ui.add_space(12.0);
        if pill_button(ui, theme, "Добавить музыку", Some(Icon::Plus), ButtonKind::Filled).clicked() {
            self.view = View::Library;
        }
    }

    /// Включить всю библиотеку вперемешку.
    fn shuffle_all(&mut self) {
        // Перемешивание живёт в очереди Controller. Флаг читаем напрямую, пока Controller не переведён
        // на PlaybackEngine (шаг 5c): после этого заменить на `state().shuffle`.
        if !self.ctl.queue.shuffle() {
            self.ctl.toggle_shuffle();
        }
        let items = self.ctl.library.clone();
        self.ctl.play_list(items, 0);
    }

    /// Полка «Быстрый выбор»: последние добавленные треки (LocalProvider хранит их в порядке добавления).
    fn home_quick(&mut self, ui: &mut Ui, theme: &Theme) {
        // Только нужные 12 штук вместе с номерами в библиотеке (номер нужен, чтобы начать с этого трека).
        let recents: Vec<(usize, Track)> = self.ctl.library.iter().cloned().enumerate().rev().take(QUICK_COUNT).collect();
        let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
        let playing = self.ctl.is_playing();
        let time = ui.input(|i| i.time);
        let mut play_index: Option<usize> = None;

        shelf(ui, theme, "quick", "Быстрый выбор", "Недавно добавленные: запустите музыку с любого трека", |ui| {
            for (idx, track) in &recents {
                let is_current = current_id.as_deref() == Some(track.id.as_str());
                let artists = artists_text(track);
                let card = MediaCard { seed: &track.id, title: &track.title, subtitle: &artists, round: false, w: QUICK_W, h: QUICK_H };
                if media_card(ui, theme, &card, is_current, playing, time).clicked() {
                    play_index = Some(*idx);
                }
            }
        });
        if let Some(i) = play_index {
            let items = self.ctl.library.clone();
            self.ctl.play_list(items, i);
        }
    }

    /// Полка «Ваши исполнители»: из локальной библиотеки. Клик включает все треки исполнителя.
    fn home_artists(&mut self, ui: &mut Ui, theme: &Theme) {
        let artists = self.top_artists(ui);
        if artists.is_empty() {
            return;
        }
        let time = ui.input(|i| i.time);
        let mut play_artist: Option<String> = None;

        shelf(ui, theme, "artists", "Ваши исполнители", "Из вашей библиотеки, больше всего треков сверху", |ui| {
            for (name, count) in &artists {
                let sub = format!("{count} {}", tracks_word(*count));
                let card = MediaCard { seed: name, title: name, subtitle: &sub, round: true, w: ARTIST_W, h: ARTIST_H };
                if media_card(ui, theme, &card, false, false, time).clicked() {
                    play_artist = Some(name.clone());
                }
            }
        });
        if let Some(name) = play_artist {
            let items: Vec<Track> = self.ctl.library.iter().filter(|t| t.artists.iter().any(|a| *a == name)).cloned().collect();
            if !items.is_empty() {
                self.ctl.play_list(items, 0);
            }
        }
    }

    /// Исполнители по числу треков. Пересчитываем только когда меняется размер библиотеки.
    fn top_artists(&self, ui: &Ui) -> Vec<(String, usize)> {
        let id = ui.scope_id().with("artists_cache");
        let len = self.ctl.library.len();
        if let Some((cached_len, items)) = ui.ctx().data_mut(|d| d.get_temp::<(usize, Vec<(String, usize)>)>(id)) {
            if cached_len == len {
                return items;
            }
        }
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for track in &self.ctl.library {
            for artist in &track.artists {
                *counts.entry(artist.as_str()).or_default() += 1;
            }
        }
        let mut items: Vec<(String, usize)> = counts.into_iter().map(|(a, n)| (a.to_string(), n)).collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(ARTISTS_COUNT);
        ui.ctx().data_mut(|d| d.insert_temp(id, (len, items.clone())));
        items
    }

    /// Список «Мои треки»: первые 10 треков библиотеки, как список чарта.
    fn home_chart(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let count = self.ctl.library.len();
        let rows: Vec<(usize, Track)> = self.ctl.library.iter().cloned().enumerate().take(CHART_COUNT).collect();
        let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
        let playing = self.ctl.is_playing();
        let time = ui.input(|i| i.time);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Мои треки").size(22.0).strong());
            ui.add_space(12.0);
            let link = ui.add(egui::Label::new(RichText::new("вся музыка →").size(13.0).color(p.on_surface_variant)).sense(Sense::click()));
            if link.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                self.view = View::Library;
            }
        });
        ui.add_space(10.0);

        let mut play_index: Option<usize> = None;
        for (idx, track) in &rows {
            let is_current = current_id.as_deref() == Some(track.id.as_str());
            if track_row(ui, theme, *idx, track, is_current, is_current && playing, time).clicked() {
                play_index = Some(*idx);
            }
        }
        if count > CHART_COUNT {
            ui.add_space(4.0);
            if pill_button(ui, theme, "Показать всё", None, ButtonKind::Tonal).clicked() {
                self.view = View::Library;
            }
        }
        if let Some(i) = play_index {
            let items = self.ctl.library.clone();
            self.ctl.play_list(items, i);
        }
    }
}

/// «трек», «трека», «треков» по правилам русского языка.
fn tracks_word(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (_, 11..=14) => "треков",
        (1, _) => "трек",
        (2..=4, _) => "трека",
        _ => "треков",
    }
}

// ============================================================================================
// Полка с горизонтальной прокруткой и стрелками
// ============================================================================================

/// Прокрутка и анимация полки между кадрами (хранится в памяти egui).
#[derive(Clone, Default)]
struct ShelfState {
    offset: f32,
    max: f32,
    /// Идущая анимация от стрелки: (откуда, куда, время старта).
    anim: Option<(f32, f32, f64)>,
}

/// Полка: заголовок с подписью, справа две круглые стрелки, под ними ряд карточек, который
/// прокручивается стрелками (плавно), колесом при зажатом Shift или перетаскиванием.
fn shelf(ui: &mut Ui, theme: &Theme, id: &str, title: &str, subtitle: &str, add_cards: impl FnOnce(&mut Ui)) {
    let p = &theme.palette;
    let sid = ui.scope_id().with(("shelf", id));
    let mut st: ShelfState = ui.ctx().data_mut(|d| d.get_temp(sid)).unwrap_or_default();
    let now = ui.input(|i| i.time);

    // Шапка: текст слева, стрелки справа.
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    let text_rect = Rect::from_min_max(head.min, pos2((head.right() - 100.0).max(head.left() + 80.0), head.bottom()));
    ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
        ui.add(egui::Label::new(RichText::new(title).size(22.0).strong()).truncate());
        ui.add(egui::Label::new(RichText::new(subtitle).size(13.0).color(p.on_surface_variant)).truncate());
    });
    let arrows = Rect::from_min_max(pos2(head.right() - 92.0, head.top()), pos2(head.right(), head.bottom() - 8.0));
    let mut step = 0.0_f32;
    ui.scope_builder(UiBuilder::new().max_rect(arrows).layout(Layout::right_to_left(Align::Center)), |ui| {
        // right_to_left: первой добавленная стрелка окажется правее.
        if arrow_button(ui, theme, true, st.offset < st.max - 1.0).clicked() {
            step = 1.0;
        }
        if arrow_button(ui, theme, false, st.offset > 1.0).clicked() {
            step = -1.0;
        }
    });
    if step != 0.0 {
        let to = (st.offset + step * SHELF_STEP).clamp(0.0, st.max.max(0.0));
        st.anim = Some((st.offset, to, now));
    }

    // Пока идёт анимация стрелки, сами задаём смещение (с замедлением к концу); иначе прокруткой управляет пользователь.
    let mut forced = None;
    if let Some((from, to, t0)) = st.anim {
        let k = ((now - t0) / 0.28).clamp(0.0, 1.0) as f32;
        forced = Some(from + (to - from) * (1.0 - (1.0 - k).powi(3)));
        if k >= 1.0 {
            st.anim = None;
        } else {
            ui.ctx().request_repaint();
        }
    }

    let mut area = ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(x) = forced {
        area = area.horizontal_scroll_offset(x);
    }
    let out = area.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SHELF_GAP;
            add_cards(ui);
        });
    });
    st.offset = out.state.clamped_offset().x;
    st.max = (out.content_size.x - out.inner_rect.width()).max(0.0);
    ui.ctx().data_mut(|d| d.insert_temp(sid, st));
}

/// Круглая кнопка-стрелка полки. Если прокручивать некуда, она бледная и не нажимается.
fn arrow_button(ui: &mut Ui, theme: &Theme, right: bool, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter();
    let fade = if enabled { 255 } else { 90 };
    painter.circle_filled(rect.center(), 20.0, with_alpha(theme.tonal_fill(), fade));
    if enabled && resp.hovered() {
        painter.circle_filled(rect.center(), 20.0, theme.row_hover_fill());
    }
    let dx = if right { 1.0 } else { -1.0 };
    let c = rect.center();
    let pts = vec![c + vec2(-3.0 * dx, -6.0), c + vec2(3.0 * dx, 0.0), c + vec2(-3.0 * dx, 6.0)];
    painter.add(Shape::line(pts, Stroke::new(2.0, with_alpha(theme.on_tonal(), fade))));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

// ============================================================================================
// Карточки и строки (Material 3)
// ============================================================================================

/// Что показать на карточке полки.
struct MediaCard<'a> {
    /// От чего зависит цвет плитки-заглушки (пока нет обложек).
    seed: &'a str,
    title: &'a str,
    subtitle: &'a str,
    /// Круглая картинка (исполнитель) вместо скруглённого квадрата (трек).
    round: bool,
    w: f32,
    h: f32,
}

/// Текст в одну строку с многоточием, если не влез. `max_w` в пикселях.
fn elided(painter: &Painter, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, max_w);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}

/// «Слой состояния» M3: наведение 8%, нажатие 12% цвета контента поверх контейнера.
fn state_layer(painter: &Painter, rect: Rect, radius: f32, resp: &Response, content: Color32) {
    let a = if resp.is_pointer_button_down_on() { 31 } else if resp.hovered() { 20 } else { 0 };
    if a > 0 {
        painter.rect_filled(rect, radius, with_alpha(content, a));
    }
}

/// Карточка M3 (filled card): контейнер surface_container_highest, внутри обложка, ниже заголовок
/// (title) и подзаголовок (supporting text). Без рамок, без градиента поверх картинки.
/// Выбранный (играющий) трек получает secondary_container. При наведении на обложке появляется
/// маленькая FAB-кнопка «играть», как в карточках M3.
fn media_card(ui: &mut Ui, theme: &Theme, card: &MediaCard, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(card.w, card.h), Sense::click());
    let radius = 16.0; // M3 shape: large
    let (container, on_container, on_variant) = if is_current {
        (p.secondary_container, p.on_secondary_container, with_alpha(p.on_secondary_container, 200))
    } else {
        (p.surface_container_highest, p.on_surface, p.on_surface_variant)
    };
    ui.painter().rect_filled(rect, radius, container);

    // Обложка: у трека скруглённый квадрат (12), у исполнителя круг.
    let pad = 8.0;
    let side = card.w - pad * 2.0;
    let cover = Rect::from_min_size(rect.min + vec2(pad, pad), Vec2::splat(side));
    let cover_radius = if card.round { side / 2.0 } else { 12.0 };
    cover_tile(ui, theme, cover, card.seed, cover_radius);

    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, cover_radius, Color32::from_black_alpha(100));
        icons::paint_equalizer(painter, cover.center(), 32.0, Color32::WHITE, time, is_playing);
    } else if resp.hovered() {
        // Small FAB: 40x40, форма medium (12), цвета primary_container / on_primary_container.
        let fab = Rect::from_min_size(pos2(cover.right() - 8.0 - 40.0, cover.bottom() - 8.0 - 40.0), Vec2::splat(40.0));
        painter.rect_filled(fab, 12.0, p.primary_container);
        icons::paint(painter, Icon::Play, fab.center(), 26.0, p.on_primary_container);
    }

    // Подписи: title (14, средний), supporting text (12).
    let text_w = card.w - pad * 2.0 - 4.0;
    let ty = cover.bottom() + 10.0;
    let title = elided(painter, card.title, FontId::proportional(14.5), on_container, text_w);
    let sub = elided(painter, card.subtitle, FontId::proportional(12.0), on_variant, text_w);
    let tx = rect.left() + pad + 2.0;
    painter.galley(pos2(tx, ty), title.clone(), on_container);
    painter.galley(pos2(tx, ty + title.size().y + 2.0), sub, on_variant);

    state_layer(painter, rect, radius, &resp, on_container);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ============================================================================================
// Волна из точек: всегда анимирована, реагирует на курсор
// ============================================================================================

/// Состояние взаимодействия волны с курсором (хранится в памяти egui между кадрами).
#[derive(Clone, Copy, Default)]
struct WaveState {
    t_prev: f64,
    /// Сглаженная позиция курсора в координатах холста.
    cursor: Vec2,
    /// 0..1: насколько курсор «над волной» (плавно появляется и исчезает).
    hover: f32,
    /// 0..1: энергия от скорости курсора (быстрое движение сильнее будоражит волну).
    energy: f32,
    /// 0..1: затухающая «вспышка» от клика и точка, откуда она расходится кольцом.
    burst: f32,
    burst_pos: Vec2,
}

impl WaveState {
    /// Один шаг по времени. Повторный вызов с тем же временем (второй проход стекла) ничего не меняет.
    fn update(&mut self, now: f64, pointer: Option<Vec2>, clicked: bool) {
        let dt = ((now - self.t_prev) as f32).clamp(0.0, 0.1);
        self.t_prev = now;
        if dt <= 0.0 {
            return;
        }
        match pointer {
            Some(p) => {
                if self.hover < 0.02 {
                    self.cursor = p; // курсор только что вошёл: не тянем волну через весь холст
                }
                let prev = self.cursor;
                self.cursor += (p - self.cursor) * (1.0 - (-dt * 14.0).exp()); // сглаживание движения
                let speed = (self.cursor - prev).length() / dt; // пикс/сек
                self.energy = self.energy.max((speed / 900.0).min(1.0));
                self.hover += (1.0 - self.hover) * (1.0 - (-dt * 8.0).exp());
                if clicked {
                    self.burst = 1.0;
                    self.burst_pos = p;
                }
            }
            None => self.hover -= self.hover * (1.0 - (-dt * 4.0).exp()),
        }
        self.energy *= (-dt * 2.2).exp();
        self.burst *= (-dt * 1.4).exp();
    }
}

/// Внешний вид волны на этот кадр.
struct WaveLook {
    time: f64,
    amp: f32,
    base: Color32,
    accent: Color32,
}

/// Три ленты точек, которые плывут по синусоидам. Время идёт всегда. Курсор:
///  - притягивает гребень волны к себе по вертикали;
///  - раздвигает точки вокруг (пузырь) и пускает по ним круговую рябь, сильнее при быстром движении;
///  - подкрашивает ближние точки акцентным цветом;
/// клик пускает от места нажатия расходящееся кольцо.
fn paint_wave(painter: &Painter, rect: Rect, look: &WaveLook, st: &WaveState) {
    const RIBBONS: u64 = 3;
    const DOTS: u64 = 1100;
    const SIGMA: f32 = 110.0; // радиус влияния курсора, px
    let t = look.time;
    let phase = t * 0.8;
    let cy = rect.height() / 2.0; // считаем в координатах холста, потом сдвигаем на rect.min
    let amp = look.amp * (1.0 + 0.35 * st.burst + 0.25 * st.energy * st.hover);
    let mut mesh = egui::Mesh::default();
    mesh.reserve_triangles((RIBBONS * DOTS * 2) as usize);
    mesh.reserve_vertices((RIBBONS * DOTS * 4) as usize);
    let cur = st.cursor;
    let tf = t as f32;
    for k in 0..RIBBONS {
        let kf = k as f64;
        for i in 0..DOTS {
            let u = i as f64 / (DOTS - 1) as f64;
            let a = (u * TAU * 1.3 + phase * 0.9 + kf * 1.7).sin();
            let b = (u * TAU * 2.7 - phase * 0.6 + kf * 0.9).sin() * 0.5;
            let c = (u * TAU * 0.6 + phase * 0.4 + kf).sin() * 0.7;
            let envelope = (u * PI).sin().powf(0.6);
            let center_y = cy + (((a + b + c) / 1.8 * envelope) as f32) * amp * rect.height() * 0.32;

            let (n1, n2, n3) = (hash01(k * 7919, i), hash01(k * 7919, i + 100_000), hash01(k * 7919, i + 200_000));
            let spread = ((n1 + n2) - 1.0) as f32;
            let thickness = rect.height() * 0.17 * (0.3 + 0.7 * a.abs() as f32) * (0.4 + 0.6 * amp.min(1.3));
            let mut x = (u as f32) * rect.width() + (n3 as f32 - 0.5) * 4.0;
            let mut y = center_y + spread * thickness;
            let mut tint = 0.0_f32;

            if st.hover > 0.002 || st.burst > 0.01 {
                if st.hover > 0.002 {
                    let (dx, dy) = (x - cur.x, y - cur.y);
                    let d = (dx * dx + dy * dy).sqrt().max(0.001);
                    let infl = (-(d * d) / (2.0 * SIGMA * SIGMA)).exp() * st.hover;
                    y += (cur.y - y) * infl * 0.22; // гребень тянется к курсору
                    let push = infl * (22.0 + 38.0 * st.energy); // пузырь вокруг курсора
                    x += dx / d * push;
                    y += dy / d * push * 0.7;
                    y += (d * 0.07 - tf * 5.0).sin() * infl * (3.0 + 9.0 * st.energy); // рябь
                    tint = infl;
                }
                if st.burst > 0.01 {
                    // Кольцо от клика: радиус растёт по мере затухания вспышки.
                    let (dx, dy) = (x - st.burst_pos.x, y - st.burst_pos.y);
                    let d = (dx * dx + dy * dy).sqrt().max(0.001);
                    let r = (1.0 - st.burst) * 520.0;
                    let ring = (-((d - r) / 38.0).powi(2)).exp() * st.burst;
                    x += dx / d * ring * 26.0;
                    y += dy / d * ring * 26.0;
                    tint = tint.max(ring);
                }
            }

            let color = look.base.lerp_to_gamma(look.accent, tint.min(1.0));
            let alpha = ((40.0 + 170.0 * n2 as f32 * (1.0 - spread.abs() * 0.5)) * (1.0 + 0.7 * tint)).clamp(0.0, 255.0) as u8;
            let size = 1.4 + 1.6 * tint;
            mesh.add_colored_rect(Rect::from_center_size(rect.min + vec2(x, y), vec2(size, size)), with_alpha(color, alpha));
        }
    }
    let painter = painter.with_clip_rect(rect);
    painter.add(Shape::mesh(mesh));
}

/// Псевдослучайное число 0..1 из (seed, i): один и тот же вход всегда даёт один и тот же результат.
fn hash01(seed: u64, i: u64) -> f64 {
    let mut z = seed.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}
