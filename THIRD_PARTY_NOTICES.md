# Сторонние компоненты

## Material Symbols (шрифт иконок)
- Автор: Google. Лицензия: Apache License 2.0 — https://www.apache.org/licenses/LICENSE-2.0
- Источник шрифта: https://github.com/google/material-design-icons
- Подключён через крейт `egui_material_icons` (лицензия MIT) — https://github.com/lucasmerlin/hello_egui
- Шрифт вшит в исполняемый файл Mcrate без изменений.

## Цветовые темы
Цвета готовых тем (Тёмная, Светлая, Монохром, Эмбермайр и другие) перенесены из проекта
LomKich1/sheduleappreload (ui/theme/AppTheme.kt) — проект того же автора.

## egui_glass (Liquid Glass для egui)
- Лицензия: MIT. Источник: https://crates.io/crates/egui_glass (репозиторий heonny/egui-glass).
- В проекте лежит изменённая копия `vendor/egui_glass` (список изменений в `vendor/egui_glass/MCRATE.md`).

## liquid-glass-generator
- Автор: gdjohn4s (GitHub: imggion). Лицензия: MIT, © 2025 gdjohn4s — https://github.com/imggion/liquid-glass-generator
- Алгоритм линзы (преломление купола и контура края, свечение края) портирован с GLSL на WGSL в `vendor/egui_glass/src/glass.wgsl`. Код сайта (Next.js) не используется.
