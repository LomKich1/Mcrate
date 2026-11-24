//! Окно ввода названия («Новый плейлист», «Переименовать»): заголовок, поле и кнопки «Отмена» / действие.
//! Ничего не создаёт: возвращает выбор, действие делает вызывающий. Пустое название принять нельзя.

use super::confirm_dialog::{DialogChoice, modal_card};
use crate::theme::{TextRole, Theme};
use crate::ui::widgets::{ButtonKind, pill_button};
use egui::{Align, Context, Key, Layout, RichText, TextEdit};

/// Ширина окна ввода: как у окна подтверждения, но с запасом под название.
const TEXT_DIALOG_WIDTH: f32 = 380.0;
/// Самое длинное название, которое можно ввести (в символах).
pub(crate) const NAME_MAX_CHARS: usize = 60;

/// Что показать в окне ввода.
pub(crate) struct TextDialog<'a> {
    /// Ключ окна: у разных окон он разный.
    pub key: &'a str,
    pub title: &'a str,
    /// Подсказка в пустом поле.
    pub hint: &'a str,
    /// Подпись кнопки действия («Создать»).
    pub confirm_label: &'a str,
}

/// Показывает окно. `None`, пока пользователь ничего не выбрал; клик по затемнению и Esc это отмена.
/// Enter в поле подтверждает, если название не пустое.
pub(crate) fn text_dialog(ctx: &Context, theme: &Theme, spec: &TextDialog, text: &mut String) -> Option<DialogChoice> {
    let out = modal_card(ctx, theme, (spec.key, TEXT_DIALOG_WIDTH), |ui| {
        let p = &theme.palette;
        ui.label(RichText::new(spec.title).size(theme.text_size(TextRole::HeadlineSmall)).color(p.on_surface));
        ui.add_space(16.0);
        let field = ui.add(TextEdit::singleline(text).hint_text(spec.hint).desired_width(f32::INFINITY).char_limit(NAME_MAX_CHARS));
        // Курсор сразу в поле: окно открыто ради ввода.
        if !field.has_focus() {
            field.request_focus();
        }
        let can_confirm = !text.trim().is_empty();
        let enter = field.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(24.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // В right_to_left первой рисуется самая правая кнопка.
            let confirm =
                ui.add_enabled_ui(can_confirm, |ui| pill_button(ui, theme, spec.confirm_label, None, ButtonKind::Tonal).clicked()).inner;
            if confirm || (enter && can_confirm) {
                return Some(DialogChoice::Confirm);
            }
            if pill_button(ui, theme, "Отмена", None, ButtonKind::Plain).clicked() {
                return Some(DialogChoice::Cancel);
            }
            None
        })
        .inner
    });
    match out.inner {
        Some(choice) => Some(choice),
        None if out.dismissed => Some(DialogChoice::Cancel),
        None => None,
    }
}
