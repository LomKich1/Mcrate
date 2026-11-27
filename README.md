# Mcrate

Десктопный плеер для Windows 11 на Rust: Yandex Music, VK Music, SoundCloud, Spotify и свои локальные файлы.
Интерфейс на egui (wgpu) с Liquid Glass и Material 3. Правила и список задач в [CLAUDE.md](CLAUDE.md).

## Структура
- `mcrate-core`: общие типы (`Track`, `MusicProvider`) и хранилище токенов (Windows Credential Manager).
- `mcrate-audio`: воспроизведение (`PlaybackEngine`, очередь, кэш потоков).
- `mcrate-net`: HTTP-клиент, загрузка HLS/прямых ссылок.
- `mcrate-providers`: провайдеры (локальные файлы, SoundCloud).
- `mcrate-app`: окно и интерфейс.
- `vendor/egui_glass`: изменённая копия egui_glass с физической моделью линзы (см. `vendor/egui_glass/MCRATE.md`).
- [Документация и ссылки по iOS / Liquid Glass](docs/LIQUID_GLASS_IOS_REFERENCES.md): Apple HIG/WWDC, SwiftUI/UIKit API и репозитории иконок.

## Сборка
Нужен Rust 1.95+. Запуск: `cargo run -p mcrate-app`. Проверка: `cargo test --workspace`.
