//! Текст в одну строку с многоточием.

use egui::{Color32, FontId, Painter};
use std::sync::Arc;

/// Текст в одну строку с многоточием, если не влез. `max_w` в пикселях.
pub(crate) fn elided(painter: &Painter, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, max_w);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}
