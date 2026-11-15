//! Главная. Структура навеяна главной SoundCloud-Desktop (приветствие и поиск, «продолжить», полки
//! с подборками), но блоков мало и они свои: один экран не должен быть перегружен.
//!
//! Блоки сверху вниз:
//!   1. поиск (по Enter открывает «Мою музыку» с этим запросом);
//!   2. «карточка-продолжение»: что играет сейчас или с чего начать;
//!   3. три быстрых действия: перемешать всё, открыть SoundCloud, добавить музыку;
//!   4. полка «Недавно добавленные» (горизонтальная прокрутка).
//!
//! Пока в ядре нет обложек, истории и подборок сервисов (см. AUDIT_UI_vs_core.md), данные берутся
//! только из локальной библиотеки. Полки «Недавно слушали» и «Для вас» добавим, когда они появятся.

use egui::{Align, Align2, Color32, CursorIcon, FontId, Key, Layout, Margin, Rect, Response, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use mcrate_core::Track;

use super::widgets::{ButtonKind, artists_text, cover_tile, icon_button, pill_button};
use crate::app::{McrateApp, View};
use crate::icons::{self, Icon};
use crate::theme::{Theme, with_alpha};

const HERO_H: f32 = 120.0; // высота карточки «продолжить»
const TILE_H: f32 = 72.0; // высота плитки быстрого действия
const CARD_W: f32 = 160.0; // ширина карточки трека на полке (она же сторона обложки)
const CARD_H: f32 = 214.0; // высота карточки: обложка + две строки текста
const RECENT_MAX: usize = 12; // сколько карточек на полке

impl McrateApp {
    pub fn home_view(&mut self, ui: &mut Ui, theme: &Theme) {
        ui.heading("Главная");
        ui.add_space(4.0);

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // ---------- поиск ----------
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Что послушаем? Название или исполнитель")
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(16, 10)),
            );
            // Enter в поле: результаты показывает экран «Моя музыка» (там же будут и сервисы).
            if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.search.trim().is_empty() {
                self.view = View::Library;
            }
            ui.add_space(14.0);

            self.home_hero(ui, theme);
            ui.add_space(14.0);
            self.home_tiles(ui, theme);
            ui.add_space(18.0);
            self.home_recent(ui, theme);

            ui.add_space(8.0 + self.bottom_inset); // подушка под плавающую панель плеера (Liquid Glass)
        });
    }

    /// Большая карточка: играющий трек (с кнопками) или подсказка, с чего начать.
    fn home_hero(&mut self, ui: &mut Ui, theme: &Theme) {
        let now = self.ctl.now.clone();
        let playing = self.ctl.is_playing();
        let count = self.ctl.library.len();
        let on = theme.on_tonal();

        // Вся карточка кликабельна: открывает «Сейчас играет», если там есть трек.
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), HERO_H), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(rect, 24.0, theme.tonal_fill());
        if resp.hovered() {
            painter.rect_filled(rect, 24.0, theme.row_hover_fill());
        }
        let cover = Rect::from_min_size(rect.min + vec2(16.0, 16.0), Vec2::splat(HERO_H - 32.0));
        cover_tile(ui, theme, cover, now.as_ref().map(|t| t.id.as_str()).unwrap_or("mcrate"), 18.0);

        let (kicker, title, sub) = match &now {
            Some(t) => (if playing { "Сейчас играет" } else { "Пауза" }.to_string(), t.title.clone(), artists_text(t)),
            None if count > 0 => ("Ваша музыка".to_string(), "Включите что-нибудь".to_string(), format!("Треков в библиотеке: {count}")),
            None => (
                "Добро пожаловать".to_string(),
                "Добавьте свою музыку".to_string(),
                "Перетащите файлы в окно или выберите их в «Моей музыке»".to_string(),
            ),
        };
        // Текст: от обложки до кнопок. Длинные строки обрезаются многоточием (`truncate`).
        let text_rect = Rect::from_min_max(
            pos2(cover.right() + 18.0, rect.top() + 16.0),
            pos2((rect.right() - 150.0).max(cover.right() + 80.0), rect.bottom() - 16.0),
        );
        ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(kicker).size(12.5).color(with_alpha(on, 190))).truncate());
            ui.add(egui::Label::new(RichText::new(title).size(22.0).strong().color(on)).truncate());
            ui.add(egui::Label::new(RichText::new(sub).size(14.0).color(with_alpha(on, 190))).truncate());
        });

        // Кнопки справа, по центру по вертикали. В right_to_left первый элемент оказывается правее.
        let btn_rect = Rect::from_min_max(pos2(rect.right() - 148.0, rect.top()), pos2(rect.right() - 16.0, rect.bottom()));
        ui.scope_builder(UiBuilder::new().max_rect(btn_rect).layout(Layout::right_to_left(Align::Center)), |ui| {
            if now.is_some() {
                if icon_button(ui, theme, Icon::Next, 40.0, ButtonKind::Tonal, false, "Следующий").clicked() {
                    self.ctl.next();
                }
                let (icon, tip) = if playing { (Icon::Pause, "Пауза") } else { (Icon::Play, "Играть") };
                if icon_button(ui, theme, icon, 52.0, ButtonKind::Filled, false, tip).clicked() {
                    self.ctl.toggle_pause();
                }
            } else if count > 0 {
                if icon_button(ui, theme, Icon::Play, 52.0, ButtonKind::Filled, false, "Играть всё").clicked() {
                    let items = self.ctl.library.clone();
                    self.ctl.play_list(items, 0);
                }
            } else if pill_button(ui, theme, "Добавить", Some(Icon::Plus), ButtonKind::Filled).clicked() {
                self.view = View::Library;
            }
        });

        if resp.clicked() && now.is_some() {
            self.view = View::NowPlaying;
        }
    }

    /// Три плитки быстрых действий. `horizontal_wrapped`: на узком окне плитки переносятся на вторую строку.
    fn home_tiles(&mut self, ui: &mut Ui, theme: &Theme) {
        let gap = ui.spacing().item_spacing.x;
        let w = ((ui.available_width() - gap * 2.0) / 3.0).floor().max(170.0);
        let has_music = !self.ctl.library.is_empty();
        ui.horizontal_wrapped(|ui| {
            if action_tile(ui, theme, w, Icon::Shuffle, "Перемешать", "всю мою музыку", has_music).clicked() {
                self.shuffle_all();
            }
            if action_tile(ui, theme, w, Icon::Cloud, "SoundCloud", "бета", true).clicked() {
                self.view = View::SoundCloud;
            }
            if action_tile(ui, theme, w, Icon::Plus, "Добавить", "файлы или папку", true).clicked() {
                self.view = View::Library;
            }
        });
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

    /// Полка «Недавно добавленные»: последние добавленные треки (LocalProvider хранит их в порядке добавления).
    fn home_recent(&mut self, ui: &mut Ui, theme: &Theme) {
        if self.ctl.library.is_empty() {
            return;
        }
        // Берём только нужные 12 штук вместе с их номерами в библиотеке (номер нужен, чтобы начать с этого трека).
        let recents: Vec<(usize, Track)> = self.ctl.library.iter().cloned().enumerate().rev().take(RECENT_MAX).collect();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Недавно добавленные").size(18.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if pill_button(ui, theme, "Все треки", None, ButtonKind::Tonal).clicked() {
                    self.view = View::Library;
                }
            });
        });
        ui.add_space(8.0);

        let current_id = self.ctl.now.as_ref().map(|t| t.id.clone());
        let playing = self.ctl.is_playing();
        let time = ui.input(|i| i.time);
        let mut play_index: Option<usize> = None;
        ScrollArea::horizontal().id_salt("home_recent").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (idx, track) in &recents {
                    let is_current = current_id.as_deref() == Some(track.id.as_str());
                    if track_card(ui, theme, track, is_current, playing, time).clicked() {
                        play_index = Some(*idx);
                    }
                }
            });
        });
        if let Some(i) = play_index {
            let items = self.ctl.library.clone();
            self.ctl.play_list(items, i);
        }
    }
}

/// Плитка быстрого действия: круглая иконка слева, название и подпись.
fn action_tile(ui: &mut Ui, theme: &Theme, width: f32, icon: Icon, title: &str, sub: &str, enabled: bool) -> Response {
    let p = &theme.palette;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, TILE_H), sense);
    let painter = ui.painter();
    painter.rect_filled(rect, 20.0, p.surface_container_low);
    if enabled && resp.hovered() {
        painter.rect_filled(rect, 20.0, theme.row_hover_fill());
    }
    let fade = if enabled { 255 } else { 110 };
    let center = pos2(rect.left() + 16.0 + 20.0, rect.center().y);
    painter.circle_filled(center, 20.0, with_alpha(theme.tonal_fill(), fade));
    icons::paint(painter, icon, center, 22.0, with_alpha(theme.on_tonal(), fade));
    let text_x = rect.left() + 16.0 + 40.0 + 12.0;
    let clip = painter.with_clip_rect(rect);
    clip.text(pos2(text_x, rect.center().y - 9.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), with_alpha(p.on_surface, fade));
    clip.text(pos2(text_x, rect.center().y + 11.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.5), with_alpha(p.on_surface_variant, fade));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Карточка трека на полке: квадратная обложка, под ней название и исполнитель.
fn track_card(ui: &mut Ui, theme: &Theme, track: &Track, is_current: bool, is_playing: bool, time: f64) -> Response {
    let p = &theme.palette;
    let (rect, resp) = ui.allocate_exact_size(vec2(CARD_W, CARD_H), Sense::click());
    let cover = Rect::from_min_size(rect.min, Vec2::splat(CARD_W));
    cover_tile(ui, theme, cover, &track.id, 20.0);

    let painter = ui.painter();
    if is_current {
        painter.rect_filled(cover, 20.0, Color32::from_black_alpha(110));
        icons::paint_equalizer(painter, cover.center(), 28.0, Color32::WHITE, time, is_playing);
    } else if resp.hovered() {
        painter.rect_filled(cover, 20.0, Color32::from_black_alpha(80));
        painter.circle_filled(cover.center(), 24.0, p.primary);
        icons::paint(painter, Icon::Play, cover.center(), 28.0, p.on_primary);
    }
    let clip = painter.with_clip_rect(Rect::from_min_max(pos2(rect.left(), cover.bottom()), rect.max));
    let title_color = if is_current { p.primary } else { p.on_surface };
    clip.text(pos2(rect.left() + 2.0, cover.bottom() + 18.0), Align2::LEFT_CENTER, &track.title, FontId::proportional(14.5), title_color);
    clip.text(pos2(rect.left() + 2.0, cover.bottom() + 37.0), Align2::LEFT_CENTER, artists_text(track), FontId::proportional(12.5), p.on_surface_variant);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}
