# Mcrate — Material 3 P0

## Scope

This change implements the P0 colour-system stage from `mcrate-material3-audit.md`.
It deliberately does not redesign the Liquid Glass composition, panels, blur, wallpaper,
player placement, or navigation.

## What changed

### `mcrate-app/src/theme/palette.rs`

- Expanded `Palette` from a partial set to all 49 color roles exposed by the pinned
  `material-colors = 0.5` `Scheme` type: primary/secondary/tertiary roles, their `on-*`
  and container pairs, fixed roles, error roles, the complete surface family,
  outline roles, inverse roles, background, shadow, and scrim.
- Added one `from_seed(seed, dark)` path built with the existing `material-colors` crate,
  `SpecVersion::Spec2025`, and `Variant::TonalSpot`.
- Routed all Material 3 presets through this same generator. Built-in presets retain
  their existing seed hues; `Custom` uses the user-selected accent and requested mode.
- Kept AMOLED as a special monochrome surface treatment while retaining the semantic
  M3 roles and readable foreground colors.
- Kept the former palette values for existing Liquid Glass roles in a separate
  `for_glass_preset` path. New roles are populated from MCU, but existing Glass values
  (primary, containers, surfaces, on-surface, outline variant, error) remain the same.
- Added unit tests for role-pair contrast and legacy Liquid Glass palette values.

### `mcrate-app/src/theme/mod.rs`

- Passes the active design into `Palette::from_settings`, so Material 3 and Liquid Glass
  select their own palette construction paths without changing the shared Theme API.

### `mcrate-app/src/ui/widgets.rs`

- The Material 3 tertiary cover tile now draws its icon with `on_tertiary_container`.
  Liquid Glass keeps its previous icon colour.

## Validation status

- Static source check: all 49 fields in the pinned crate's `Scheme` are mapped exactly
  into `Palette`.
- Static call-site check: the changed `Palette::from_settings` signature has one call
  site, updated in `theme/mod.rs`.
- Diff inspection: changes are limited to the palette, theme selection, and the single
  Material 3 cover-tile foreground pairing.
- **Not run:** `cargo test`, `cargo check`, `cargo fmt`, application launch, or screenshot
  verification. The editing environment does not contain `cargo`, `rustc`, or `rustfmt`.
  The added Rust tests are present in source but have not been executed here.

## Remaining work

- Add a user-facing contrast preference (standard / medium / high) if desired. The current
  generator uses the library's default contrast level; this P0 adds pair validation rather
  than introducing a new settings control.
- Continue with P1: typography tokens, shape scale, elevation/state layers, shared M3
  SearchBar, list metrics, and button variants.
- Perform visual comparison and run `cargo test --workspace` / `cargo check --workspace`
  in a Rust 1.95+ environment before merging.

## References used

- Material 3 `ColorScheme` API (semantic roles, including surface-container and fixed roles):
  https://developer.android.com/reference/kotlin/androidx/compose/material3/ColorScheme
- Material Color Utilities guide (seed, variant, light/dark mode, and contrast-level inputs):
  https://github.com/material-foundation/material-color-utilities/blob/main/dev_guide/creating_color_scheme.md
- Pinned Rust crate API (`material-colors` 0.5.0 `scheme::Scheme`):
  https://docs.rs/material-colors/0.5.0/material_colors/scheme/struct.Scheme.html


## Исправление после первой компиляции

- Устранён вызов удалённого `Palette::for_preset` в `ui/settings_view.rs`.
- Предпросмотр цвета в списке тем теперь строится через `Palette::from_settings` с временно выбранным preset и текущим `Design`. Поэтому swatch соответствует активному оформлению: Material 3 использует новую схему, Liquid Glass — сохранённую legacy-палитру.
