//! Экраны и виджеты. Экраны это `impl McrateApp` в отдельных файлах, чтобы app.rs не пух.

pub mod widgets;

mod library;
mod now_playing;
mod player_bar;
mod rail;
mod settings_view;
