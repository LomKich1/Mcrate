//! Экраны и общие виджеты.
//!
//! Каждый файл ниже добавляет свои методы к `McrateApp` (`home_view`, `rail_view`,
//! `player_bar`, `view_host` и остальные). Тип приложения и раскладка окна — в `app.rs`.
//! Цвета, стекло и Material 3 живут в `crate::theme`, не здесь.

mod anim;
pub(crate) mod autoscroll;
pub(crate) mod collection;
pub(crate) mod components;
pub(crate) mod format;
pub(crate) mod home;
pub(crate) mod layout;
pub(crate) mod library;
mod metrics;
pub(crate) mod nav_history;
pub(crate) mod now_playing;
pub(crate) mod player_bar;
pub(crate) mod playlists;
pub(crate) mod rail;
pub(crate) mod ripple;
pub(crate) mod settings;
pub(crate) mod shortcuts;
pub(crate) mod soundcloud;
pub(crate) mod track_menu;
pub(crate) mod thanos;
pub(crate) mod transition;
pub(crate) mod tray_menu;
pub(crate) mod wave;
pub(crate) mod widgets;
pub(crate) mod window_layout;
