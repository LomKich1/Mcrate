//! Окно подтверждения опасного действия (удаление, очистка): затемнение, карточка с заголовком,
//! пояснением и двумя кнопками. Само ничего не удаляет: возвращает выбор, действие делает вызывающий.

use crate::theme::palette::with_alpha;
use crate::theme::{ShapeRole, TextRole, Theme};
use crate::ui::widgets::{ButtonKind, pill_button};
use egui::{Align, Color32, Context, Frame, Layout, Margin, Modal, RichText, Stroke, Ui};

/// Ширина карточки диалога. Material 3 допускает 280-560 px; 340 хватает для двух строк пояснения.
const DIALOG_WIDTH: f32 = 340.0;
/// Внутренний отступ карточки по спецификации M3 (24 px со всех сторон).
const DIALOG_PADDING: i8 = 24;
/// Затемнение фона (scrim) Material 3: чёрный с прозрачностью 32 % (0.32 * 255 ≈ 82).
const SCRIM_ALPHA: u8 = 82;
/// Прозрачность заливки карточки в Liquid Glass: чуть просвечивает, но текст остаётся читаемым.
const GLASS_FILL_ALPHA: u8 = 235;

/// Что показать в окне подтверждения.
pub(crate) struct ConfirmDialog<'a> {
    /// Ключ окна: у разных окон он разный, чтобы их состояние не смешивалось.
    pub key: &'a str,
    pub title: &'a str,
    pub text: &'a str,
    /// Подпись кнопки, которая выполняет действие («Очистить»).
    pub confirm_label: &'a str,
}

/// Выбор пользователя в окне подтверждения.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DialogChoice {
    Confirm,
    Cancel,
}

/// Результат показа модального окна за один кадр.
pub(crate) struct ModalOutcome<R> {
    /// Что вернуло содержимое окна (`None`, пока ничего не выбрано).
    pub inner: Option<R>,
    /// Окно просят закрыть: клик по затемнению или Esc.
    pub dismissed: bool,
}

/// Карточка модального окна поверх всего приложения: затемнение, рамка по теме (Material 3 или Liquid Glass) и
/// ширина `width`. Все окна приложения (подтверждение, ввод названия, выбор плейлиста) строятся на ней, чтобы выглядеть
/// одинаково. `key` отличает окна друг от друга.
pub(crate) fn modal_card<R>(
    ctx: &Context,
    theme: &Theme,
    (key, width): (&str, f32),
    add: impl FnOnce(&mut Ui) -> Option<R>,
) -> ModalOutcome<R> {
    let frame = if theme.glass_active() { glass_frame(theme) } else { m3_frame(theme) };
    let id = egui::Id::unique(("modal_card", key));
    let modal = Modal::new(id).frame(frame).backdrop_color(Color32::from_black_alpha(SCRIM_ALPHA)).show(ctx, |ui| {
        ui.set_width(width);
        add(ui)
    });
    let dismissed = modal.should_close();
    ModalOutcome { inner: modal.inner, dismissed }
}

/// Показывает окно поверх всего приложения. `None`, пока пользователь ничего не выбрал;
/// клик по затемнению и Esc считаются отменой.
pub(crate) fn confirm_dialog(ctx: &Context, theme: &Theme, spec: &ConfirmDialog) -> Option<DialogChoice> {
    let out = modal_card(ctx, theme, (spec.key, DIALOG_WIDTH), |ui| dialog_body(ui, theme, spec));
    match out.inner {
        Some(choice) => Some(choice),
        None if out.dismissed => Some(DialogChoice::Cancel),
        None => None,
    }
}

/// Карточка Material 3: тональная поверхность с крупным скруглением.
fn m3_frame(theme: &Theme) -> Frame {
    Frame::new()
        .fill(theme.palette.surface_container_high)
        .corner_radius(theme.shape_radius(ShapeRole::ExtraLarge))
        .inner_margin(Margin::same(DIALOG_PADDING))
}

/// Карточка Liquid Glass: полупрозрачная заливка и тонкая светлая кромка.
fn glass_frame(theme: &Theme) -> Frame {
    let p = &theme.palette;
    Frame::new()
        .fill(with_alpha(p.surface_container_high, GLASS_FILL_ALPHA))
        .stroke(Stroke::new(1.0, with_alpha(p.on_surface, if p.dark { 28 } else { 22 })))
        .corner_radius(theme.shape_radius(ShapeRole::ExtraLarge))
        .inner_margin(Margin::same(DIALOG_PADDING))
}

/// Заголовок, пояснение и кнопки «Отмена» / действие (справа).
fn dialog_body(ui: &mut Ui, theme: &Theme, spec: &ConfirmDialog) -> Option<DialogChoice> {
    let p = &theme.palette;
    ui.label(RichText::new(spec.title).size(theme.text_size(TextRole::HeadlineSmall)).color(p.on_surface));
    ui.add_space(16.0);
    ui.add(egui::Label::new(RichText::new(spec.text).size(theme.text_size(TextRole::BodyMedium)).color(p.on_surface_variant)).wrap());
    ui.add_space(24.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        // В right_to_left первой рисуется самая правая кнопка.
        if pill_button(ui, theme, spec.confirm_label, None, ButtonKind::Tonal).clicked() {
            return Some(DialogChoice::Confirm);
        }
        if pill_button(ui, theme, "Отмена", None, ButtonKind::Plain).clicked() {
            return Some(DialogChoice::Cancel);
        }
        None
    })
    .inner
}
