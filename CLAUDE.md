# Craftflow

Десктопный планировщик производственных цепочек для Minecraft (ванилла + моды) в духе Satisfactory Modeler. Первая цель — модпак ATM10 (NeoForge 1.21.1): Mekanism, Create, AE2.

- Полная спецификация: `docs/DESIGN.md`
- План и статус: `docs/ROADMAP.md`

## Стек
Rust (stable), eframe/egui + egui-snarl, good_lp (HiGHS / microlp), mlua (Luau) для плагинов, serde. Кроссплатформа: Linux, Windows, macOS.

## Структура
- `crates/model` — типы данных
- `crates/expr` — движок формул
- `crates/packs` — загрузка и слои паков
- `crates/graph` — схема, формат файла, миграции
- `crates/solver` — сведение к Process, LP
- `crates/import` — чтение JAR
- `crates/plugin` — хост Luau
- `crates/icons` — атлас иконок
- `crates/app` — GUI
- `crates/cli` — headless-утилита
- `plugins/` — встроенные плагины (vanilla, mekanism, create, ae2)
- `tests/scenarios/` — эталонные сценарии

## Команды
```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p app
cargo run -p cli -- solve tests/scenarios/<name>/schema.json
```

## Правила
1. **Ядро не знает о модах.** Ничего специфичного для Mekanism/Create/AE2 вне `plugins/` и их паков. Если для мода нужен хак в ядре — значит, нужно расширить API плагинов (обсудить со мной).
2. **GUI тонкий.** Логика — в библиотечных крейтах и покрыта тестами. Проверяй логику через `cli` и тесты, а не через окно.
3. **Солвер работает только с Process.** Ограничения линейные.
4. **Единицы внутри — в тик**, `f64`. Конвертация — только на границе UI/CLI. Сравнения с эпсилоном.
5. Ошибки: `thiserror` в библиотеках, `anyhow` в `app`/`cli`. Без `unwrap`/`expect` в библиотечном коде, кроме доказуемо невозможных случаев с комментарием.
6. Код, идентификаторы и комментарии — на английском. Документация в `docs/` — на русском.
7. Никаких платформозависимых путей и API без `cfg` и проверки в CI на всех трёх ОС.

## Как работать
- Перед задачей читай нужный раздел `docs/DESIGN.md`. Работай по этапам `docs/ROADMAP.md`, отмечай выполненное.
- **Спроси меня перед изменением** форматов файлов (пак, схема, манифест плагина) и API плагинов. Если решение отличается от DESIGN.md — сначала обнови DESIGN.md.
- Новая логика солвера или формул — с тестом; эталонные сценарии из DESIGN.md §11 — интеграционные тесты.
- Прежде чем сказать «готово»: `cargo fmt`, `clippy` без предупреждений, тесты зелёные.
- Я Rails-разработчик и в Rust не эксперт: в неочевидных местах (lifetimes, трейты, архитектурные решения) кратко объясняй, почему сделано так.
