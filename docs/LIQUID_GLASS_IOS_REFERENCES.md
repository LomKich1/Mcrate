# iOS / Liquid Glass — документация и библиотеки иконок

Подборка официальных материалов Apple и открытых icon packs для Mcrate. Ссылки проверены 16 ноября 2026 года. Документация Apple описывает нативные SwiftUI/UIKit/AppKit API; Mcrate написан на Rust/egui, поэтому эти материалы используются как дизайн-спецификация, а визуальный материал реализован через уже встроенный `egui_glass`.

## 1. Что Apple называет Liquid Glass

1. [Liquid Glass — Technology Overview](https://developer.apple.com/documentation/technologyoverviews/liquid-glass) — концепция материала, где и зачем он используется.
2. [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) — основное руководство по миграции существующего интерфейса, слоям, навигации, контролам, группировке и производительности.
3. [Human Interface Guidelines: Materials](https://developer.apple.com/design/human-interface-guidelines/materials) — самое важное правило: Liquid Glass предназначен для функционального слоя (навигация и контролы), а не для основной поверхности контента. Активные бегунки и переключатели могут временно получать стеклянный материал.
4. [Apple Design Resources](https://developer.apple.com/design/resources/) — официальные UI Kits, шаблоны и материалы для дизайнеров.

## 2. HIG — элементы интерфейса

Эти страницы полезны не только для внешнего вида, но и для поведения иерархии, размеров интерактивных областей, читаемости и состояний.

- [Buttons](https://developer.apple.com/design/human-interface-guidelines/buttons) — типы кнопок, иерархия действий, состояния.
- [Segmented controls](https://developer.apple.com/design/human-interface-guidelines/segmented-controls) — переключение между близкими разделами и параметрами.
- [Pop-up buttons](https://developer.apple.com/design/human-interface-guidelines/pop-up-buttons) — компактный выбор одного значения из списка.
- [Controls](https://developer.apple.com/design/human-interface-guidelines/controls) — отдельные Control Center / Lock Screen controls и Action Button; это не общий гайд по кнопкам внутри приложения.
- [Sliders](https://developer.apple.com/design/human-interface-guidelines/sliders) — бегунок, активная дорожка и обратная связь.
- [Toggles](https://developer.apple.com/design/human-interface-guidelines/toggles) — переключатели.
- [Search fields](https://developer.apple.com/design/human-interface-guidelines/search-fields) — поиск и размещение поля.
- [Tab bars](https://developer.apple.com/design/human-interface-guidelines/tab-bars) — основная навигация.
- [Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars) — навигационные боковые панели.
- [Toolbars](https://developer.apple.com/design/human-interface-guidelines/toolbars) — группы родственных команд и расположение действий.
- [Menus](https://developer.apple.com/design/human-interface-guidelines/menus) — контекстные меню и иконки команд.
- [Sheets](https://developer.apple.com/design/human-interface-guidelines/sheets) — временные панели и модальные листы.
- [Alerts](https://developer.apple.com/design/human-interface-guidelines/alerts) — подтверждения и сообщения.
- [Scroll views](https://developer.apple.com/design/human-interface-guidelines/scroll-views) — прокрутка контента под функциональным слоем.
- [Accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility) — доступность и альтернативные способы взаимодействия.
- [Motion](https://developer.apple.com/design/human-interface-guidelines/motion) — движение и переходы.
- [Color](https://developer.apple.com/design/human-interface-guidelines/color) — цвет, контраст и светлый/тёмный варианты.
- [App icons](https://developer.apple.com/design/human-interface-guidelines/app-icons) — актуальные форматы, слои и appearance-варианты иконки приложения для Liquid Glass. Это руководство про иконку приложения, а не про набор glyph-иконок внутри UI.

## 3. Нативные API Liquid Glass

### SwiftUI

- [Glass](https://developer.apple.com/documentation/swiftui/glass) — тип материала, варианты `regular`/`clear`, интерактивность и tint.
- [glassEffect(_:in:)](https://developer.apple.com/documentation/swiftui/view/glasseffect%28_%3Ain%3A%29) — применение материала к View.
- [Applying Liquid Glass to custom views](https://developer.apple.com/documentation/swiftui/applying-liquid-glass-to-custom-views) — пользовательские элементы и композиция стекла.
- [PrimitiveButtonStyle.glass](https://developer.apple.com/documentation/swiftui/primitivebuttonstyle/glass) — стандартный стеклянный стиль кнопок.
- [GlassEffectContainer](https://developer.apple.com/documentation/swiftui/glasseffectcontainer) — контейнер для объединения/морфинга нескольких Glass-элементов и эффективного рендеринга.

### UIKit

- [UIGlassEffect](https://developer.apple.com/documentation/uikit/uiglasseffect) — эффект Liquid Glass для кастомных UIView.
- [UIGlassContainerEffect](https://developer.apple.com/documentation/uikit/uiglasscontainereffect) — согласованное объединение стеклянных элементов.
- [UIVisualEffectView](https://developer.apple.com/documentation/uikit/uivisualeffectview) — контейнер визуальных эффектов.
- [UIButton](https://developer.apple.com/documentation/uikit/uibutton) — системная кнопка и стили.
- [UISlider](https://developer.apple.com/documentation/uikit/uislider) — системный слайдер.
- [UISwitch](https://developer.apple.com/documentation/uikit/uiswitch) — системный переключатель.

## 4. Видео WWDC25 — самые полезные сессии

У видео есть главы, поэтому можно сразу перейти к нужной части.

1. [Meet Liquid Glass](https://developer.apple.com/videos/play/wwdc2025/219/) — концепция, визуальные свойства и правила материала.
2. [Get to know the new design system](https://developer.apple.com/videos/play/wwdc2025/356/) — структура интерфейса, разделение слоёв и адаптация дизайна.
3. [Build a SwiftUI app with the new design](https://developer.apple.com/videos/play/wwdc2025/323/) — структура приложения (03:07), toolbars (07:49), поиск (11:10), controls (14:03), Liquid Glass effects (17:57).
4. [Build a UIKit app with the new design](https://developer.apple.com/videos/play/wwdc2025/284/) — tab/sidebar, навигация и toolbars (07:19), search (15:18), controls (17:24), custom elements (19:15).
5. [Build an AppKit app with the new design](https://developer.apple.com/videos/play/wwdc2025/326/) — адаптация того же дизайн-системного подхода к macOS.
6. [What’s new in SF Symbols 7](https://developer.apple.com/videos/play/wwdc2025/337/) — Draw, Variable Draw, gradients, Magic Replace и custom symbols.
7. [Create icons with Icon Composer](https://developer.apple.com/videos/play/wwdc2025/361/) — создание многослойных app icons, настройка стеклянных свойств и вариантов для appearance modes.

### Дополнительные материалы WWDC26

8. [Platforms State of the Union](https://developer.apple.com/videos/play/wwdc2026/112/) — обновления Liquid Glass для следующих системных релизов: читаемость на сложном фоне, более выраженные края/блики, поведение панелей и значков.
9. [What’s new in SwiftUI](https://developer.apple.com/videos/play/wwdc2026/269/) — обновлённое Liquid Glass-оформление, интерактивные элементы и поведение toolbar; глава про визуальный стиль начинается с 02:12. Часть обсуждения относится к поколениям ОС 2027 года.
10. [Design intuitive search experiences](https://developer.apple.com/videos/play/wwdc2026/292/) — структура Search Field, расположение поиска и адаптация к стеклянному toolbar или inline-контенту.

## 5. Иконки: репозитории GitHub

### Наиболее близкие к iOS

| Набор | Репозиторий | Лицензия / особенности |
|---|---|---|
| **Ionicons** | [ionic-team/ionicons](https://github.com/ionic-team/ionicons) | MIT. Есть iOS/Outline и Filled стили; выбран для Liquid Glass в Mcrate. |
| Cupertino Icons | [flutter/packages — cupertino_icons](https://github.com/flutter/packages/tree/main/packages/cupertino_icons) | Иконки, используемые в Flutter Cupertino UI; хороший источник для сравнения с системной эстетикой Apple, но это не полный набор SF Symbols. Проверяйте лицензию конкретной версии/пакета перед самостоятельным копированием ресурсов. |
| SF Symbols | [официальный сайт Apple](https://developer.apple.com/sf-symbols/) | Официальный набор Apple (7 000+ символов в SF Symbols 7). Это не обычный open-source GitHub pack: распространяйте экспортированные символы только в рамках действующей лицензии Apple. |

### Открытые альтернативы

| Набор | Репозиторий | Лицензия / особенности |
|---|---|---|
| Tabler Icons | [tabler/tabler-icons](https://github.com/tabler/tabler-icons) | MIT; тысячи SVG, чистый outline-стиль. |
| Phosphor | [phosphor-icons/core](https://github.com/phosphor-icons/core) | MIT; несколько толщин и характеров линий. |
| Lucide | [lucide-icons/lucide](https://github.com/lucide-icons/lucide) | ISC; минималистичные линейные символы. |
| Heroicons | [tailwindlabs/heroicons](https://github.com/tailwindlabs/heroicons) | MIT; outline/solid/mini. |
| Feather | [feathericons/feather](https://github.com/feathericons/feather) | MIT; компактный однообразный line style. |
| Octicons | [primer/octicons](https://github.com/primer/octicons) | MIT; системные иконки GitHub. |
| Iconoir | [iconoir-icons/iconoir](https://github.com/iconoir-icons/iconoir) | MIT; широкая библиотека outline-иконок. |
| Remix Icon | [Remix-Design/remixicon](https://github.com/Remix-Design/RemixIcon) | Apache-2.0; line/fill пары. |
| Bootstrap Icons | [twbs/icons](https://github.com/twbs/icons) | MIT; большой набор универсальных SVG. |
| Fluent UI System Icons | [microsoft/fluentui-system-icons](https://github.com/microsoft/fluentui-system-icons) | MIT; regular/filled варианты. |
| Carbon | [carbon-design-system/carbon-icons](https://github.com/carbon-design-system/carbon-icons) | Apache-2.0; строгая корпоративная система иконок IBM. |
| Lobe Icons | [lobehub/lobe-icons](https://github.com/lobehub/lobe-icons) | MIT; аккуратные UI- и бренд-глифы; брендовые знаки требуют отдельной проверки товарных знаков. |
| Devicon | [devicons/devicon](https://github.com/devicons/devicon) | MIT; логотипы языков и инструментов разработки, не универсальная UI-иконография. |
| Material Design Icons | [google/material-design-icons](https://github.com/google/material-design-icons) | Apache-2.0; сохранён для Material 3 в Mcrate. |

Для дополнительного сравнения паков, числа иконок, объёма TTF и лицензий: [iconflow 2.1.0 — список 14 поддерживаемых паков](https://docs.rs/iconflow/2.1.0/iconflow/). В версии 2.1.0 в Ionicons указано 1 356 glyph-имён и около 0,28 MiB TTF; Tabler — 4 964, Phosphor — 9 072, Lucide — 1 665. В таблице выше собраны наиболее подходящие для интерфейса наборы, включая Ionicons, Tabler, Lucide, Iconoir, Phosphor, Feather, Heroicons, Octicons, Fluent UI и другие.

## 6. Что применено в Mcrate

- **Liquid Glass:** существующий `vendor/egui_glass` и его GPU backdrop/refraction renderer оставлены основой темы; дополнительно используется публичный `GlassButton`/`paint_glass` API библиотеки.
- **Кнопки:** `pill_button` и контентные icon buttons в Liquid Glass переведены на настоящий `GlassButton`, который читает backdrop, использует настройки blur/refraction темы, поддерживает интерактивную анимацию, доступное имя и минимум 44 px для hit area.
- **Без стекла поверх стекла:** кнопки мини-плеера остаются плоскими дочерними контролами на уже стеклянном PlayerBar; навигационная рейка также не получает повторные стеклянные подложки.
- **Слайдер:** во время наведения/перетаскивания thumb получает настоящий shader-backed glass effect; неактивное состояние остаётся лёгким и читаемым.
- **Глобальные контролы:** `Theme::apply` задаёт Liquid Glass палитру, hover/pressed/focus состояния, скругления, отступы и 44 px высоту интерактивных областей для стандартных egui widgets.
- **Иконки:** Liquid Glass использует Ionicons Outline; Material 3 продолжает использовать Material Symbols Rounded. Оба набора зарегистрированы одновременно, поэтому переключение темы не требует перезапуска. В Liquid Glass доступны 24 общих действия/навигационных glyphs, нужных текущему UI; в бинарник не добавляются остальные 13 паков iconflow.
- **Почему не SF Symbols:** их можно изучать и применять в пределах лицензии Apple, но нельзя автоматически подменять открытым GitHub-фонтом или считать извлечённые чужими репозиториями SVG свободными для распространения.

### Дизайн-принципы для дальнейших правок

1. Стекло маркирует функциональный слой: навигацию, toolbar, контекстные действия и отдельные важные контролы. Длинные списки, текст и основная область остаются на более спокойной поверхности.
2. Не наслаивать маленькие glass buttons на уже стеклянную панель. На PlayerBar/рейке использовать плоский glyph + прозрачный hover/pressed highlight.
3. Primary действие должно быть визуально сильнее secondary; не окрашивать все кнопки акцентом.
4. Стеклянный материал должен учитывать, что находится под ним: контраст, tint и blur регулируются от темы и настроек графики.
5. Проверять Light/Dark, keyboard focus, контраст, уменьшение прозрачности/движения, HiDPI и слабые GPU.

## 7. Технические ссылки используемых библиотек

- [`egui_glass`](https://crates.io/crates/egui_glass) — GPU стекло для egui; локальная модифицированная копия находится в `vendor/egui_glass`.
- [`iconflow 2.1.0`](https://docs.rs/iconflow/2.1.0/iconflow/) — единый API и встраиваемые шрифты. В Cargo включён только feature `pack-ionicons` (не весь набор из 14 паков).
- [`Ionicons`](https://github.com/ionic-team/ionicons) — исходный MIT-репозиторий выбранных glyphs.
- [Third-party notices проекта](../THIRD_PARTY_NOTICES.md) — сведения о лицензиях встроенных ресурсов.
