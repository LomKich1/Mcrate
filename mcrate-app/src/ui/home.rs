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
//! Волна пока чисто декоративная: когда у движка появится спектр (spectrum_request_for_LomKich.md),
//! её амплитуду можно привязать к нему.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Key, Layout, Margin, Painter, Rect, Response, RichText, ScrollArea, Sense, Shape, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};
use mcrate_core::Track;

use super::widgets::{ButtonKind, artists_text, cover_tile, fmt_time, icon_button, pill_button};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

const WAVE_H: f32 = 200.0; // высота волны
const SHELF_GAP: f32 = 12.0; // зазор между карточками на полке
const SHELF_STEP: f32 = 420.0; // на сколько пикселей прокручивает стрелка
const QUICK_W: f32 = 132.0; // карточка «Быстрого выбора»
const QUICK_H: f32 = 178.0;
const ARTIST_W: f32 = 150.0; // карточка исполнителя
const ARTIST_H: f32 = 200.0;
const QUICK_COUNT: usize = 12; // сколько треков на полке «Быстрый выбор»
const ARTISTS_COUNT: usize = 12; // сколько исполнителей на полке
const CHART_COUNT: usize = 10; // строк в списке «Мои треки»

impl McrateApp {
    pub fn home_view(&mut self, ui: &mut Ui, theme: &Theme) {
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ---------- поиск ----------
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Поиск по названию и исполнителю")
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(16, 10)),
            );
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
        let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;

        // Фаза волны хранится в памяти egui; бежит только пока играет музыка (на паузе волна замирает и не грузит видеокарту).
        let id = ui.scope_id().with("wave_phase");
        let mut phase: f64 = ui.ctx().data_mut(|d| d.get_temp(id)).unwrap_or(0.0);
        if playing {
            phase += dt * 0.8;
            ui.ctx().request_repaint();
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, phase));
        let amp = ui.ctx().animate_value_with_time(id.with("amp"), if playing { 1.0 } else { 0.5 }, 0.5);

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

        let (canvas, _) = ui.allocate_exact_size(vec2(ui.available_width(), WAVE_H), Sense::hover());
        paint_wave(ui.painter(), canvas, phase, amp, p.on_surface);
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
                let card = MediaCard { seed: &track.id, title: &track.title, subtitle: &artists, upper: false, w: QUICK_W, h: QUICK_H };
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
                let card = MediaCard { seed: name, title: name, subtitle: &sub, upper: true, w: ARTIST_W, h: ARTIST_H };
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
            if chart_row(ui, theme, idx + 1, track, is_current, playing, time).clicked() {
                play_index = Some(*idx);
            }
            ui.add_space(8.0);
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
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
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
// Карточки и строки
// ============================================================================================

/// Что показать на карточке полки.
struct MediaCard<'a> {
    /// От чего зависит цвет плитки-заглушки (пока нет обложек).
    seed: &'a str,
    title: &'a str,
    subtitle: &'a str,
    /// Название заглавными буквами (так у исполнителей).
    upper: bool,
    w: f32,
    h: f32,
}

/// Высокая карточка: плитка на всю карточку, внизу затемнение и подпись белым.
fn media_card(ui: &mut Ui, theme: &Theme, card: &MediaCard, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(card.w, card.h), Sense::click());
    let radius = 16.0;
    cover_tile(ui, theme, rect, card.seed, radius);

    let painter = ui.painter();
    paint_bottom_gradient(painter, rect, radius, 84.0, 215);
    if is_current {
        painter.rect_filled(rect, radius, Color32::from_black_alpha(90));
        icons::paint_equalizer(painter, rect.center() - vec2(0.0, 18.0), 28.0, Color32::WHITE, time, is_playing);
    } else if resp.hovered() {
        painter.rect_filled(rect, radius, Color32::from_black_alpha(60));
        painter.circle_filled(rect.center() - vec2(0.0, 18.0), 22.0, p.primary);
        icons::paint(painter, Icon::Play, rect.center() - vec2(0.0, 18.0), 26.0, p.on_primary);
    }
    let title = if card.upper { card.title.to_uppercase() } else { card.title.to_string() };
    let clip = painter.with_clip_rect(rect.shrink2(vec2(8.0, 0.0)));
    clip.text(pos2(rect.left() + 10.0, rect.bottom() - 32.0), Align2::LEFT_CENTER, title, FontId::proportional(13.5), Color32::WHITE);
    clip.text(pos2(rect.left() + 10.0, rect.bottom() - 14.0), Align2::LEFT_CENTER, card.subtitle, FontId::proportional(11.5), Color32::from_white_alpha(190));
    // Тонкая рамка, как на скриншоте.
    painter.rect_stroke(rect, radius, Stroke::new(1.0, with_alpha(p.on_surface, 28)), StrokeKind::Inside);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Тёмный градиент по нижней кромке карточки. Рисуется полосками по 2 px; на последних строках
/// полоски сужаются по дуге скругления, чтобы градиент не торчал из углов.
fn paint_bottom_gradient(painter: &Painter, rect: Rect, radius: f32, height: f32, max_alpha: u8) {
    let top = rect.bottom() - height;
    let mut mesh = egui::Mesh::default();
    let step = 2.0;
    let mut y = top;
    while y < rect.bottom() {
        let y2 = (y + step).min(rect.bottom());
        let inset = |yy: f32| {
            let d = yy - (rect.bottom() - radius);
            if d > 0.0 { radius - (radius * radius - d * d).max(0.0).sqrt() } else { 0.0 }
        };
        let alpha = |yy: f32| {
            let t = ((yy - top) / height).clamp(0.0, 1.0);
            Color32::from_black_alpha((max_alpha as f32 * t.powf(1.4)) as u8)
        };
        let (i1, i2) = (inset(y), inset(y2));
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(rect.left() + i1, y), alpha(y));
        mesh.colored_vertex(pos2(rect.right() - i1, y), alpha(y));
        mesh.colored_vertex(pos2(rect.left() + i2, y2), alpha(y2));
        mesh.colored_vertex(pos2(rect.right() - i2, y2), alpha(y2));
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 3, base + 2);
        y = y2;
    }
    painter.add(Shape::mesh(mesh));
}

/// Строка списка «Мои треки»: номер, обложка, название, исполнитель, длительность; тонкая рамка.
fn chart_row(ui: &mut Ui, theme: &Theme, number: usize, track: &Track, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(rect, 14.0, with_alpha(p.primary, if p.dark { 34 } else { 28 }));
    } else if resp.hovered() {
        painter.rect_filled(rect, 14.0, theme.row_hover_fill());
    }
    painter.rect_stroke(rect, 14.0, Stroke::new(1.0, with_alpha(p.on_surface, 26)), StrokeKind::Inside);

    painter.text(pos2(rect.left() + 28.0, rect.center().y), Align2::CENTER_CENTER, number.to_string(), FontId::proportional(14.0), p.on_surface_variant);
    let cover = Rect::from_center_size(pos2(rect.left() + 82.0, rect.center().y), Vec2::splat(48.0));
    cover_tile(ui, theme, cover, &track.id, 10.0);
    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 10.0, Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 20.0, Color32::WHITE, time, is_playing);
    }
    let text_left = cover.right() + 16.0;
    let dur_x = rect.right() - 18.0;
    let clip = painter.with_clip_rect(Rect::from_min_max(pos2(text_left, rect.top()), pos2(dur_x - 56.0, rect.bottom())));
    let title_color = if is_current { p.primary } else { p.on_surface };
    clip.text(pos2(text_left, rect.center().y - 10.0), Align2::LEFT_CENTER, &track.title, FontId::proportional(15.0), title_color);
    clip.text(pos2(text_left, rect.center().y + 11.0), Align2::LEFT_CENTER, artists_text(track), FontId::proportional(13.0), p.on_surface_variant);
    painter.text(pos2(dur_x, rect.center().y), Align2::RIGHT_CENTER, fmt_time(track.duration_secs as u64), FontId::proportional(13.0), p.on_surface_variant);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ============================================================================================
// Волна из точек
// ============================================================================================

/// Три ленты точек, которые плывут по синусоидам. `phase` растёт со временем, `amp` 0..1 задаёт размах.
/// Это украшение: к звуку не привязано.
fn paint_wave(painter: &Painter, rect: Rect, phase: f64, amp: f32, color: Color32) {
    const RIBBONS: u64 = 3;
    const DOTS: u64 = 1100;
    let cy = rect.center().y;
    let mut shapes: Vec<Shape> = Vec::with_capacity((RIBBONS * DOTS) as usize);
    for k in 0..RIBBONS {
        let kf = k as f64;
        for i in 0..DOTS {
            let u = i as f64 / (DOTS - 1) as f64; // 0..1 по ширине
            // Три синусоиды с разной скоростью дают плавную «живую» волну.
            let a = (u * TAU * 1.3 + phase * 0.9 + kf * 1.7).sin();
            let b = (u * TAU * 2.7 - phase * 0.6 + kf * 0.9).sin() * 0.5;
            let c = (u * TAU * 0.6 + phase * 0.4 + kf).sin() * 0.7;
            let envelope = (u * PI).sin().powf(0.6); // к краям волна затухает
            let center_y = cy + (((a + b + c) / 1.8 * envelope) as f32) * amp * rect.height() * 0.32;

            // Разброс точек вокруг линии: чем ближе к гребню, тем облако толще.
            let (n1, n2, n3) = (hash01(k * 7919, i), hash01(k * 7919, i + 100_000), hash01(k * 7919, i + 200_000));
            let spread = ((n1 + n2) - 1.0) as f32; // -1..1, чаще около нуля
            let thickness = rect.height() * 0.17 * (0.3 + 0.7 * a.abs() as f32) * (0.4 + 0.6 * amp);
            let x = rect.left() + (u as f32) * rect.width() + (n3 as f32 - 0.5) * 4.0;
            let y = center_y + spread * thickness;
            let alpha = (40.0 + 170.0 * n2 as f32 * (1.0 - spread.abs() * 0.5)).clamp(0.0, 255.0) as u8;
            shapes.push(Shape::rect_filled(Rect::from_center_size(pos2(x, y), vec2(1.4, 1.4)), 0.7, with_alpha(color, alpha)));
        }
    }
    painter.with_clip_rect(rect).extend(shapes);
}

/// Псевдослучайное число 0..1 из (seed, i): один и тот же вход всегда даёт один и тот же результат.
fn hash01(seed: u64, i: u64) -> f64 {
    let mut z = seed.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}
