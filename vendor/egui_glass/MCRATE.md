# Изменения Mcrate в egui_glass 0.2.0

Исходник: https://crates.io/crates/egui_glass (MIT, heonny). Подключён через `[patch.crates-io]` в корневом `Cargo.toml`.

Что изменено:
- `src/style.rs`: поля `lens_mix`, `refractive_index`, `dome`, `curvature`, `edge_sharpness`, `glow` и метод `physical(...)`. У всех готовых пресетов `lens_mix = 0`, то есть прежний вид не меняется.
- `src/renderer.rs`: два новых `vec4` в `Uniforms` (`lens_a`, `lens_b`), размер 208 байт (слот 256).
- `src/glass.wgsl`: блок «lens model» (купол + контур края по закону Снеллиуса) и внутреннее свечение края. Алгоритм портирован из https://github.com/imggion/liquid-glass-generator (MIT, © 2025 gdjohn4s), GLSL -> WGSL, единицы UV -> пиксели.
- Удалена папка `tests/` оригинала (интеграционные тесты не нужны приложению).
