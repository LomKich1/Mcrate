//! Экраны и общие виджеты.
//!
//! Каждый файл ниже добавляет свои методы к `McrateApp` (`home_view`, `rail_view`,
//! `player_bar`, `view_host` и остальные). Тип приложения и раскладка окна — в `app.rs`.
//! Цвета, стекло и Material 3 живут в `crate::theme`, не здесь.

pub mod collection;
pub mod home;
pub mod layout;
pub mod library;
pub mod now_playing;
pub mod player_bar;
pub mod rail;
pub mod settings_main;
pub mod settings_more;
pub mod settings_screens;
pub mod settings_view;
pub mod settings_widgets;
pub mod soundcloud_view;
pub mod transition;
pub mod widgets;
pub mod wave;
