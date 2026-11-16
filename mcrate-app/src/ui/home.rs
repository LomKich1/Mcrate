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
//! Волна (модуль `wave.rs`) живёт всегда (в паузе медленно дрейфует), реагирует на воспроизведение, смену трека,
//! прогресс, курсор и клики. В настройках доступно уменьшение движения. К звуку волна пока не привязана: спектр должен
//! поступать из аудиодвижка, отдельной реализации которого в текущем API ещё нет.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use egui::{
    Align, Color32, CursorIcon, FontId, Key, Layout, Painter, Rect, Response, RichText, ScrollArea,
    Sense, Shape, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use mcrate_core::Track;

use super::wave::{self, COL_STEP_PX, WaveInput, WaveSim};
use super::layout::{Bleed, bleed_clip, pane_scroll, scroll_end_padding};
use super::widgets::{ButtonKind, artists_text, cover_tile, glide_begin, glide_end, glass_icon_button, pill_button, search_bar, squish, track_row};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::settings::Design;
use crate::theme::{TextRole, Theme, with_alpha};

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
        let bleed = self.bleed;
        // Прокрутка растянута до краёв панели: полоса прокрутки лежит на самом краю, а карточки
        // и заголовки уходят под край, а не режутся в отступе от него (см. `pane_scroll`).
        // Скругление панели: бегунок отступает от углов на этот радиус, а в углах (только Material 3
        // без Mica/Acrylic, где за панелью сплошной цвет `surface`) закрашиваются квадратные «пустоты».
        let corner = theme.pane_radius() as f32;
        let mask = (theme.settings.design == Design::Material3 && !theme.window_backdrop_active()).then_some(theme.palette.surface);
        pane_scroll(ui, bleed, corner, mask, ScrollArea::vertical().auto_shrink([false, false]), |area, ui| area.show(ui, |ui| {
            // ---------- поиск ----------
            let search = search_bar(ui, theme, &mut self.search, "Поиск по названию и исполнителю");
            // Enter в поле: результаты показывает экран «Ваши загрузки» (свои файлы).
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
            scroll_end_padding(ui, self.bottom_inset);
        }));
    }

    /// «Моя волна»: заголовок, две кнопки и анимированная волна из точек.
    fn home_wave(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        let playing = self.ctl.is_playing();
        let has_music = !self.ctl.library.is_empty();
        // Время берём у основного контекста (в закадровом проходе стекла свои часы).
        let ctx_time = ui.input(|i| i.time);
        let now = if self.frame_time > 0.0 { self.frame_time } else { ctx_time };
        // Закадровый проход стекла (его часы не совпадают с основными) рисует облегчённую волну: через стекло видно только размытое пятно.
        let backdrop_pass = (ctx_time - now).abs() > 1e-3;
        let wave_id = ui.scope_id().with("wave_state");
        let reduce_motion = theme.settings.reduce_motion;

        // Шапка: текст слева, кнопки справа.
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let text_rect = Rect::from_min_max(head.min, pos2((head.right() - 140.0).max(head.left() + 120.0), head.bottom()));
        ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
            ui.add(egui::Label::new(RichText::new("Моя волна").size(theme.text_size(TextRole::DisplaySmall)).strong()).truncate());
            ui.add(egui::Label::new(RichText::new("Ваша музыка вперемешку: нажмите ▶ и слушайте").size(theme.text_size(TextRole::LabelLarge)).color(p.on_surface_variant)).truncate());
        });
        let btn_rect = Rect::from_min_max(pos2(head.right() - 120.0, head.top()), head.max);
        ui.scope_builder(UiBuilder::new().max_rect(btn_rect).layout(Layout::right_to_left(Align::Center)), |ui| {
            let (icon, tip) = if playing { (Icon::Pause, "Пауза") } else { (Icon::Play, "Запустить волну") };
            if glass_icon_button(ui, theme, icon, 52.0, ButtonKind::Filled, false, tip).clicked() && has_music {
                if self.ctl.now.is_some() {
                    self.ctl.toggle_pause();
                } else {
                    self.shuffle_all();
                }
            }
            if glass_icon_button(ui, theme, Icon::Shuffle, 40.0, ButtonKind::Tonal, false, "Перемешать заново").clicked() && has_music {
                self.shuffle_all();
            }
        });

        // Холст волны слушает курсор (hover), нажатие и клик.
        let wave_height = if ui.clip_rect().height() < 640.0 { 148.0 } else { WAVE_H };
        // Волна идёт от края до края панели в обеих темах: холст шире страницы на её боковые отступы.
        // Нажатие и наведение ловим на всём широком холсте (`interact`), а не на узкой рамке.
        let wave_bleed = self.bleed.left.min(self.bleed.right);
        let (page_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), wave_height), Sense::hover());
        let canvas = page_rect.expand2(vec2(wave_bleed, 0.0));
        let resp = ui.interact(canvas, wave_id.with("hit"), Sense::click());
        // Волна прокручена за экран (или скрыта): не считаем и не рисуем, и не просим перерисовку.
        if !ui.is_rect_visible(canvas) {
            return;
        }

        // Что сейчас играет: из этого волна узнаёт про смену трека и прогресс.
        let duration = self.ctl.now.as_ref().map(|t| t.duration_secs as f32).unwrap_or(0.0);
        let progress = if duration > 0.0 { Some((self.ctl.position().as_secs_f32() / duration).clamp(0.0, 1.0)) } else { None };
        let input = WaveInput {
            now,
            playing,
            track_id: self.ctl.now.as_ref().map(|t| t.id.as_str()),
            progress,
            reduce_motion,
            pointer: resp.hover_pos().map(|p| p - canvas.min),
            pressed: resp.is_pointer_button_down_on(),
            size: canvas.size(),
        };
        let mut sim: WaveSim = ui.ctx().data_mut(|d| d.get_temp(wave_id)).unwrap_or_default();
        sim.step(&input);

        if !backdrop_pass && !reduce_motion {
            // 60 к/с, пока есть заметное движение; в покое волна дрейфует на ~30 к/с (дешевле для CPU).
            let ms = if sim.busy() { 16 } else { 33 };
            ui.ctx().request_repaint_after(Duration::from_millis(ms));
        }
        // Колонок больше при широком окне; кратно 20, чтобы таблица не пересобиралась на каждый пиксель.
        let cols = ((canvas.width() / COL_STEP_PX / 20.0).round() as usize * 20).clamp(180, 900);
        let table = wave::wave_table(ui.ctx(), cols);
        wave::paint(ui.painter(), canvas, &sim, &table, p.on_surface, p.primary, if backdrop_pass { 3 } else { 1 });
        ui.ctx().data_mut(|d| d.insert_temp(wave_id, sim));
    }

    /// Пустая библиотека: подсказка вместо полок.
    fn home_empty(&mut self, ui: &mut Ui, theme: &Theme) {
        let p = &theme.palette;
        ui.label(RichText::new("Здесь пока пусто").size(theme.text_size(TextRole::TitleLarge)).strong());
        ui.add_space(4.0);
        ui.label(RichText::new("Перетащите файлы в окно или добавьте музыку в «Библиотека → Ваши загрузки».").color(p.on_surface_variant));
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

        shelf(ui, theme, self.bleed, "quick", "Быстрый выбор", "Недавно добавленные: запустите музыку с любого трека", |ui| {
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

        shelf(ui, theme, self.bleed, "artists", "Ваши исполнители", "Из вашей библиотеки, больше всего треков сверху", |ui| {
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
        let rows: Vec<(usize, Track)> = self.ctl.library.iter().cloned().enumerate().take(CHART_COUNT).collect();
        let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
        let playing = self.ctl.is_playing();
        let time = ui.input(|i| i.time);

        // Заголовок и кнопка «Показать всё» в одной строке: кнопка стоит рядом с названием
        // (раньше здесь была ссылка «вся музыка →», а кнопка лежала под списком).
        ui.horizontal(|ui| {
            ui.label(RichText::new("Мои треки").size(theme.text_size(TextRole::TitleLarge)).strong());
            ui.add_space(8.0);
            let kind = if theme.glass_active() { ButtonKind::Tonal } else { ButtonKind::Text };
            if pill_button(ui, theme, "Показать всё", None, kind).clicked() {
                self.view = View::Library;
            }
        });
        ui.add_space(10.0);

        let mut play_index: Option<usize> = None;
        // Тёмная подсветка под курсором скользит между строками (см. widgets::glide_end).
        let glide_slot = glide_begin(ui);
        let mut hovered = None;
        for (idx, track) in &rows {
            let is_current = current_id.as_deref() == Some(track.id.as_str());
            let resp = track_row(ui, theme, *idx, track, is_current, is_current && playing, time);
            if resp.hovered() {
                hovered = Some((egui::Id::unique(&track.id).value(), resp.rect));
            }
            if resp.clicked() {
                play_index = Some(*idx);
            }
        }
        glide_end(ui, theme, glide_slot, ui.scope_id().with("glide_home_tracks"), hovered, 18.0);
        if let Some(i) = play_index {
            let items = self.ctl.library.clone();
            self.ctl.play_list(items, i);
        }
    }
}

/// «трек», «трека», «треков» по правилам русского языка.
pub(super) fn tracks_word(n: usize) -> &'static str {
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
fn shelf(ui: &mut Ui, theme: &Theme, bleed: Bleed, id: &str, title: &str, subtitle: &str, add_cards: impl FnOnce(&mut Ui)) {
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
        // Карточки при прокрутке уходят до самых краёв панели, а не обрезаются в отступе от них.
        bleed_clip(ui, bleed.sides());
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
    let fade = if enabled { 255 } else { 72 };
    if theme.glass_active() {
        // В Liquid Glass стрелки — лёгкие toolbar-контролы: без постоянной M3-заливки.
        if resp.hovered() || resp.is_pointer_button_down_on() {
            let alpha = if resp.is_pointer_button_down_on() { 58 } else { 38 };
            painter.circle_filled(rect.center(), 19.0, with_alpha(theme.palette.on_surface, alpha));
            painter.circle_stroke(rect.center(), 18.5, Stroke::new(0.8, with_alpha(theme.palette.on_surface, 66)));
        }
    } else {
        painter.circle_filled(rect.center(), 20.0, with_alpha(theme.tonal_fill(), fade));
        if enabled && resp.hovered() {
            painter.circle_filled(rect.center(), 20.0, theme.row_hover_fill());
        }
    }
    let dx = if right { 1.0 } else { -1.0 };
    let c = rect.center();
    let pts = vec![c + vec2(-3.0 * dx, -6.0), c + vec2(3.0 * dx, 0.0), c + vec2(-3.0 * dx, 6.0)];
    let arrow_color = if theme.glass_active() { theme.palette.on_surface } else { theme.on_tonal() };
    painter.add(Shape::line(pts, Stroke::new(if theme.glass_active() { 1.8 } else { 2.0 }, with_alpha(arrow_color, fade))));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

// ============================================================================================
// Карточки и строки: M3 filled cards и более лёгкая медиаподача Liquid Glass.
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

/// Медиакарточка. В M3 — filled card; в Liquid Glass — обложка и подписи напрямую
/// на контентной поверхности без отдельного M3-контейнера.
fn media_card(ui: &mut Ui, theme: &Theme, card: &MediaCard, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let glass = theme.glass_active();
    let (rect, resp) = ui.allocate_exact_size(vec2(card.w, card.h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp; // карточка за краем полки или экрана: не рисуем
    }
    // Продавливание при нажатии, как у строк треков (`squish` в widgets.rs): карточка плавно уменьшается
    // вокруг центра. Доля по высоте подобрана так, чтобы абсолютное сжатие по ширине и высоте было
    // одинаковым: содержимое (обложка и подписи) считается от верхнего края и остаётся внутри карточки.
    // Клик по-прежнему ловится по исходному прямоугольнику (`resp`), поэтому края не «дрожат».
    const PRESS: f32 = 0.05;
    let rect = squish(ui, resp.id, &resp, rect, PRESS, PRESS * card.w / card.h);
    let radius = if glass { 14.0 } else { 16.0 };
    let (on_container, on_variant) = if is_current && !glass {
        (p.on_secondary_container, with_alpha(p.on_secondary_container, 200))
    } else {
        (p.on_surface, p.on_surface_variant)
    };
    // Отпускаем неизменяемое заимствование `ui` до `cover_tile()`, которому
    // требуется `&mut Ui`. После отрисовки обложки painter запрашивается повторно.
    {
        let painter = ui.painter();
        if glass {
            // Apple Music-подобная медиаполка: обложка и подписи лежат непосредственно
            // на контентной поверхности, без отдельного M3 filled-card контейнера.
            if resp.hovered() || resp.is_pointer_button_down_on() {
                painter.rect_filled(rect, radius, with_alpha(p.on_surface, if resp.is_pointer_button_down_on() { 18 } else { 10 }));
                painter.rect_stroke(rect, radius, Stroke::new(0.8, with_alpha(p.on_surface, 26)), egui::StrokeKind::Inside);
            }
        } else {
            let container = if is_current { p.secondary_container } else { p.surface_container_highest };
            painter.rect_filled(rect, radius, container);
        }
    }

    // Обложка: у трека скруглённый квадрат, у исполнителя круг.
    let pad = if glass { 4.0 } else { 8.0 };
    let side = rect.width() - pad * 2.0; // от сжатого прямоугольника, а не от card.w
    let cover = Rect::from_min_size(rect.min + vec2(pad, pad), Vec2::splat(side));
    let cover_radius = if card.round { side / 2.0 } else if glass { 10.0 } else { 12.0 };
    cover_tile(ui, theme, cover, card.seed, cover_radius);
    let painter = ui.painter();
    if glass {
        painter.rect_stroke(cover, cover_radius, Stroke::new(0.8, with_alpha(p.on_surface, 36)), egui::StrokeKind::Inside);
    }

    if is_current {
        painter.rect_filled(cover, cover_radius, Color32::from_black_alpha(if glass { 84 } else { 100 }));
        icons::paint_equalizer(painter, cover.center(), 32.0, Color32::WHITE, time, is_playing);
        if glass {
            painter.rect_stroke(cover, cover_radius, Stroke::new(1.4, with_alpha(p.primary, 210)), egui::StrokeKind::Inside);
        }
    } else if resp.hovered() {
        // Для Liquid Glass — компактная круглая prominent-кнопка поверх обложки;
        // в M3 оставляем квадратную FAB.
        if glass {
            let fab = Rect::from_min_size(pos2(cover.right() - 42.0, cover.bottom() - 42.0), Vec2::splat(34.0));
            painter.circle_filled(fab.center() + vec2(0.0, 1.0), 17.0, Color32::from_black_alpha(46));
            painter.circle_filled(fab.center(), 17.0, with_alpha(p.surface_container_high, 218));
            painter.circle_stroke(fab.center(), 16.5, Stroke::new(0.9, with_alpha(p.on_surface, 70)));
            icons::paint(theme, painter, Icon::Play, fab.center(), 21.0, p.on_surface);
        } else {
            let fab = Rect::from_min_size(pos2(cover.right() - 8.0 - 40.0, cover.bottom() - 8.0 - 40.0), Vec2::splat(40.0));
            painter.rect_filled(fab, 12.0, p.primary_container);
            icons::paint(theme, painter, Icon::Play, fab.center(), 26.0, p.on_primary_container);
        }
    }

    let text_w = rect.width() - pad * 2.0 - 4.0;
    let ty = cover.bottom() + if glass { 8.0 } else { 10.0 };
    let title = elided(painter, card.title, FontId::proportional(14.5), on_container, text_w);
    let sub = elided(painter, card.subtitle, FontId::proportional(12.0), on_variant, text_w);
    let tx = rect.left() + pad + 2.0;
    painter.galley(pos2(tx, ty), title.clone(), on_container);
    painter.galley(pos2(tx, ty + title.size().y + 2.0), sub, on_variant);

    if !glass {
        state_layer(painter, rect, radius, &resp, on_container);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}
