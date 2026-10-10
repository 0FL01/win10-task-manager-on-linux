# План: порт GUI Mission Center → Task Manager с GTK4/libadwaita на egui/eframe (pure-Rust GUI)

> Утверждённый исходный план, сохранённый как история миграции. GTK/Meson/`src/`/`resources/`
> пути ниже относятся к [root `671bc544ac4f7a3149c53db6df8a453252547d0a`](https://github.com/0FL01/win10-task-manager-on-linux/tree/671bc544ac4f7a3149c53db6df8a453252547d0a),
> graph-widget — к [upstream `8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc`](https://gitlab.com/mission-center-devs/graph-widget/-/tree/8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc).
> После утверждённой зачистки root — pure workspace, graph core — `crates/mc-graph-core`,
> SVG — `crates/mc-ui/assets/icons`, единственный required submodule — magpie.
> Silver experiment удалён; исходный notice сохранён в `docs/PROVENANCE.md`.
> `perf/RESULTS.md`/`perf/budgets.toml` ниже — планировавшиеся артефакты, они не созданы.
> Текущие решения и незакрытые критерии — в [goal-документе](2026-10-10-egui-gui-port.md).

## Context

Mission Center 1.2.0 (этот репо, GPL-3.0-or-later, 26.6k LOC в `src/`) написан на Rust, но GUI построен на GTK4/libadwaita/GSK (C) + Blueprint + GSettings + gresource. Цель: GUI-стек целиком на Rust (egui 0.36 + eframe 0.36 + wgpu, glow как альтернатива), лёгкий в простое: почти никакой работы между обновлениями и дешёвое обновление UI.

Решения пользователя (зафиксированы):
- порт **в этом репо**, лицензия GPL-3.0 остаётся, переиспользуем magpie-демон, `magpie_client`, форматтеры, логику сайдбара, тесты и `po/`;
- сначала **вертикальный срез + замеры (decision gate)**, затем поэтапно до **полного паритета** с 1.2.0;
- сборка: **Cargo + xtask**, meson/blueprint/gresource/gschema уходят.

Честная граница термина «pure Rust»: чистым Rust становится GUI-стек. В data path C остаётся: NNG через `nng-c-sys` (собирается из бандла), magpie использует nvtop, libudev, drm, а eframe динамически грузит системные libwayland/xkbcommon/Vulkan/GL. Замена NNG в объём работ не входит.

Подсказка `rust-gui-silver-bullet/` (MIT/Apache, совместима с GPL). Это **ни разу не собранный** kit на 1.2k LOC, см. его `verification/RESULTS.md`. Что берём:
- правила из его `AGENTS.md`: core без GUI-зависимостей, typed `Command` + `dispatch`, никаких `Arc<Mutex<AppState>>`, никакого безусловного `request_repaint`, никакого I/O и тяжёлых операций в `ui()`, `sync_channel(1)`/latest-slot, честный протокол замеров;
- паттерн `Tokens::apply(ctx)` из `crates/ui-kit/src/theme.rs` (палитры будут свои, в духе Adwaita);
- `typography.rs` (системный шрифт + обязательный fallback);
- кастомные виджеты через `allocate_exact_size` + `Sense`;
- eframe 0.36 API: `App::ui` + `App::logic`, не deprecated `update`;
- headless-снимки через `egui_kittest` (`examples/snapshot.rs`, `scripts/capture.sh`).

**Не** берём GPU-canvas (`gpu.rs`, `canvas.wgsl`, `cpu.rs`), COW-тайлы, undo и docking. Для графиков достаточно `egui::Painter`. Скопированный код обязательно прогнать через `cargo fmt`/`clippy`: kit не форматирован и не проверен.

## Ключевые факты из кода

- `subprojects/magpie` и `subprojects/graph-widget` в этом импорте **пустые** (gitlinks не закоммичены, в `.gitmodules` относительные URL `../gng.git`, `../graph-widget.git`). Сейчас ничего не собирается.
- Seam данных: `src/magpie_client/mod.rs`.
  - Поток `gather_and_proxy` (`mod.rs:874-1151`) владеет `!Send` `Client`, собирает `Readings` (`mod.rs:121-140`, обычные prost-структуры) и отдаёт их в UI через `glib::idle_add_once` + `app!()` (`mod.rs:909`, `1058`, `1075`). Очередь idle-колбэков неограничена: при подвисшем UI снимки копятся.
  - Остальная GTK-связность: `g_debug!`/`g_critical!` и `show_error_dialog_and_exit` (`src/main.rs:441-461`), вызываемый из `client.rs:314`, `540`, `574`, `605`.
  - Часть методов `MagpieClient` блокируется на `receiver.recv()` (`mod.rs:420`, `521` и далее), из UI-потока их вызывать нельзя.
  - Интервал: `INTERVAL_STEP=0.05`, `BASE_INTERVAL=1.0` (`application.rs:41-42`).
- Pure-логика к переиспользованию:
  - форматтеры `DataType`/`to_human_readable_*` (`src/main.rs:98-441`; кроме `BOOLEAN_DICT_CACHE` на `settings!()`, `main.rs:364-407`);
  - `src/i18n.rs`;
  - сортировка и prune сайдбара + 18 тестов (`src/performance_page/mod.rs:2389-2622`);
  - кодирование имён устройств и overrides + 8 тестов (`src/performance_page/summary_graph.rs:433+`);
  - compare-хелперы колонок (`src/table_view/columns/mod.rs`);
  - построение строк из `Readings` (`src/table_view/models.rs`, логика отделима от `RowModel` GObject).
- В GSettings 56 ключей (`data/io.missioncenter.MissionCenter.gschema.xml`). В i18n около 494 msgid, 45 языков, примерно 490 строк живут в `.blp`.
- Не-UI связность с GTK:
  - clipboard: 11 мест;
  - `gio::AppInfo::launch_default_for_uri`;
  - `gtk::IconTheme` + декодирование иконок приложений (`table_view/cached_icon.rs`, `application.rs:231`, `application.rs:288`);
  - accent color через `adw::StyleManager`;
  - 50 мс анимационный таймер всегда активен (`window.rs:1133`).

## Целевая архитектура

```
Cargo.toml                  # [workspace]; корневой пакет = legacy GTK до фазы 7; default-members = новые крейты
crates/
  mc-core/      # без GUI-зависимостей: Command/AppState, Config (serde), History ring buffers,
                # форматтеры, логика сайдбара, i18n facade (крейт `gettext`, чистый Rust, читает .mo)
  mc-magpie/    # копия magpie_client: `log` вместо glib, LatestSlot<Readings> + waker,
                # bounded-канал событий (иконки, ответы на команды, Fatal), trait ReadingsSource
                # (Magpie | Fixture)
  mc-graph/     # egui-график: datasets, Fixed/Stacking/dynamic scaling, fill, smooth, sliding,
                # grid; кеш Mesh по (generation, rect, ppp, style); downsampling min/max до ширины в px
  mc-ui/        # страницы, виртуализированная таблица, диалоги, theme Tokens, виджеты, иконки
apps/taskmanager/            # eframe-бинарник; --renderer wgpu|glow, --fixture <dir>,
                             # --dump-readings, --perf-overlay
xtask/                       # mo/pot, install, run-dev, bench-idle, appimage, flatpak
perf/budgets.toml, perf/RESULTS.md
```

Инварианты (записываются в `AGENTS.md`):
- В `ui()`/`logic()` нет I/O, IPC, сортировки и форматирования всего списка за кадр.
- Неограниченных каналов нет. Снимок передаётся через latest-slot (перезапись), события через `sync_channel(N)`.
- Repaint только по событию: новый `Readings`, ввод пользователя, sliding-анимация (только пока она идёт, с ограничением fps). В свёрнутом окне 0 кадров плюс `set_reduced_activity(true)`.
- Порядок строк таблицы, отформатированные строки и меши графиков пересчитываются только при смене ревизии данных, сортировки, фильтра или размера.

## Фазы

### Фаза 0. Bootstrap
1. Подтянуть submodules из `gitlab.com/mission-center-devs/{gng,graph-widget}` на коммитах, на которые указывает upstream-тег v1.2.0. В `.gitmodules` перейти на абсолютные https URL и закоммитить gitlinks.
2. Один раз собрать GTK-версию (`meson setup _build && ninja -C _build`): это проверка сабмодулей и **baseline-бинарник** для замеров. Плюс выяснить, как native-сборка находит `missioncenter-magpie` (`client.rs:168-245`) и как собрать magpie через cargo (flatpak делает `cd subprojects/magpie && cargo build --release`).
3. Превратить корневой `Cargo.toml` в workspace с `default-members` без GTK-пакета. В `src/meson.build` добавить `-p missioncenter`, чтобы meson продолжил собирать legacy-версию. GTK-код в `src/` замораживается и правок не получает.
4. Обновить `AGENTS.md`: слои, инварианты выше, команды проверки. `rust-gui-silver-bullet/` остаётся справочником вне workspace.

### Фаза 1. `mc-core` + `mc-magpie` (без UI)
- Скопировать `src/magpie_client/{mod,client}.rs` в `mc-magpie`:
  - `g_*!` заменить на `log`;
  - `idle_add_once` + `app!()` заменить на `LatestSlot::publish(readings)` + `waker()`, где waker — это `Box<dyn Fn()+Send+Sync>` с вызовом `ctx.request_repaint()`;
  - `app!().missing_icons` заменить множеством известных иконок внутри gatherer'а; новые иконки уходят событием `IconsReady`;
  - `show_error_dialog_and_exit` заменить событием `MagpieEvent::Fatal(String)`;
  - блокирующие запросы (`service_logs`, setup-script и т.п.) переделать в `Command` + ответ через событие. UI держит `Pending<T>` и опрашивает его в `logic()`.
- `ReadingsSource` trait: реальный magpie или fixtures. В magpie-types есть serde, поэтому `--dump-readings N` пишет JSON-фикстуры. Фикстуры дают детерминированные снимки и бенчи без демона.
- В `mc-core`:
  - форматтеры из `main.rs:98-441` (настройки единиц передаются явным параметром вместо `settings!()`);
  - чистые функции сайдбара и кодирования имён, вынесенные из `performance_page/mod.rs` и `summary_graph.rs`, вместе с их 26 тестами;
  - `History`: кольцевой буфер фиксированной ёмкости с счётчиком generation;
  - `Config` (serde TOML в `$XDG_CONFIG_HOME/missioncenter/config.toml`, поля 1:1 с 56 ключами gschema);
  - `Command`/`AppState::dispatch`.
- Тесты: перенесённые 26; golden-тесты форматтеров против текущих выводов; ring buffer; roundtrip Config; загрузка фикстур; LatestSlot (перезапись, отсутствие роста).

### Фаза 2. Вертикальный срез
- `apps/taskmanager` на eframe с cargo-features `wgpu` (default) и `glow`.
- Оболочка: левая навигация Performance/Apps, светлая/тёмная/системная тема (`Tokens`, системная тема через `ctx.system_theme()`), системный шрифт через typography.
- `mc-graph` с API, повторяющим `GraphWidget`, по фактическим вызовам из `cpu.rs:136-145`, `cpu.rs:253`, `cpu.rs:385-387`, `cpu.rs:652-664`, `graph_widget_settings.rs:33-47`. Если `mc-graph-core` (0 зависимостей) окажется полезным, переиспользовать его.
- Страницы CPU (общий график + по ядрам, kernel times) и Memory (график + swap).
- Apps: плоский список процессов на `egui_extras::TableBuilder` + `body.rows()` (виртуализация); сортировка по клику на заголовок, поиск, terminate/kill через `Command`.
- Perf overlay: время UI pass p50/p95, кадров в секунду и с момента старта, allocations/frame (counting allocator за feature `alloc-stats`).
- Снимки kittest на фикстурах: dark/light, 1280×800, узкое окно 720×600, HiDPI 2×, кириллица.

### Фаза 3. Замеры и decision gate
- `cargo xtask bench-idle`: запускает GTK baseline и egui (wgpu, glow) в одинаковых условиях. Условия: KDE Plasma Wayland, окно 1280×800, интервал 1 с, открыта страница CPU, 10 с прогрева, 60 с замера, 5 повторов.
- Что меряется:
  - CPU UI-процесса и демона отдельно по `/proc/<pid>/stat`;
  - PSS из `smaps_rollup`;
  - VRAM из `/proc/<pid>/fdinfo` (`drm-memory-*`);
  - время старта до первого кадра с данными;
  - UI pass p95;
  - число кадров между обновлениями.
- Сценарии: sliding-анимация вкл/выкл, окно свёрнуто, скролл таблицы при более чем 500 процессах.
- **Go**, если выполнено всё:
  - средний CPU UI-процесса ≤ GTK (со sliding и без);
  - PSS ≤ GTK + 10%;
  - UI pass p95 ≤ 4 мс;
  - без sliding ровно 1 кадр на одно обновление данных;
  - в свёрнутом окне 0 кадров;
  - старт ≤ GTK.
- Из двух рендереров выбрать лучший, второй оставить feature-флагом.
- **No-go** (хуже GTK более чем на 20% по CPU или PSS после одной итерации оптимизаций): остановиться и пересмотреть стек (следующий кандидат — Slint с winit).
- Результаты и окружение (OS, GPU, backend, версии) честно записать в `perf/RESULTS.md`, пороги в `perf/budgets.toml`.

### Фаза 4. Паритет Performance
- Страницы Disk, Network (dynamic scaling, bytes/bits, base2), GPU (middle/bottom graphs), Fans, Batteries; summary mode; detail-панели.
- Диалоги: SMART (SATA/NVMe), eject + eject failure, smart failure; виджет memory composition.
- Сайдбар:
  - мини-графики;
  - drag-reorder через `dnd_drag_source`/`dnd_drop_zone` с переиспользованием логики из фазы 1;
  - edit mode, hidden graphs, per-device overrides, фильтры типов сетей.
- «Copy summary» через `ctx.copy_text`.

### Фаза 5. Паритет Apps + Services
- Дерево «приложение → процессы»: плоский список видимых строк, перестраивается только при смене ревизии, сортировки, фильтра или expand.
- Merged stats; все 8 сигналов (`mod.rs:271-394`) с горячими клавишами; process details.
- Колонки: порядок и видимость через контекстное меню заголовка (TableBuilder не умеет drag колонок) с сохранением в Config; hide-zero, separators, core-count affects percentages.
- Иконки приложений:
  - байты из magpie декодируются в отдельном потоке (`image` для PNG, `resvg` через `egui_extras` svg);
  - icon-name ищется через крейт `freedesktop-icons`;
  - текстура грузится в `logic()`, кеш по app id.
- Services (user + system): start/stop/restart/enable/disable; логи в диалоге с копированием.

### Фаза 6. Остальное
- Preferences: 4 страницы (general, performance, apps/services, units).
- About system; first-run dialog (setup script: run/revert/open, копирование вывода); keyboard shortcuts dialog.
- Сохранение размера окна и состояния maximized; interface style.
- Placeholder/loading; адаптивная раскладка (collapse threshold, summary mode).
- Открытие wiki через `ctx.open_url`.
- Опциональный одноразовый импорт старых настроек через `gsettings list-recursively io.missioncenter.MissionCenter`.
- i18n: перенести все строки из `.blp` в Rust-вызовы `i18n!`/`pi18n!` **с идентичными msgid и контекстами**, чтобы переводы Weblate продолжили работать. `xtask pot` вызывает `xgettext` по `.rs` с прежними keywords.

### Фаза 7. Упаковка и удаление GTK
- `xtask`:
  - `install` ставит бинарник, magpie, desktop/metainfo, иконки и `.mo` (prefix/DESTDIR);
  - `run-dev` прописывает путь к magpie и `MC_LOCALEDIR`;
  - обновлённые AppImage-скрипт и Flatpak-манифест (runtime `org.freedesktop.Platform` вместо GNOME).
- Удалить:
  - `src/` GTK-приложения, `resources/**/*.blp`, `style.css`, gresource, gschema;
  - все `meson.build`/`meson_options.txt`, `subprojects/blueprint-compiler.wrap`;
  - submodule `graph-widget`;
  - зависимости `gtk4`/`libadwaita`/`gettext-rs`.
- Обновить README, AGENTS.md, `.gitlab-ci.yml` (cargo fmt/clippy/test + snapshots + budgets check).

## Проверка (каждая фаза)

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings      # весь pure workspace, включая graph core и xtask
cargo test --workspace                                      # unit + перенесённые 26 тестов + kittest-снимки на фикстурах
cargo run --release -p taskmanager-app -- --fixture testkit/fixtures/basic   # без демона
cargo xtask run-dev                                          # с настоящим magpie
cargo xtask bench-idle --warmup 8 --seconds 40 --repeats 3   # текущий egui-only benchmark; метод — perf/README.md
```

Ручной smoke на KDE Plasma Wayland и X11:
- переключение страниц, сортировка, поиск, kill процесса, рестарт сервиса;
- смена темы без перезапуска; HiDPI/fractional scale;
- свернуть окно и убедиться, что кадров нет.

PNG-снимки просматриваются глазами. В `perf/RESULTS.md` попадают только реально полученные цифры.

## Риски
- **Upstream сабмодулей недоступен или коммиты не находятся.** Блокер фазы 0. Запасной путь: взять исходники из Flathub/Fedora source tarball 1.2.0 (vendored).
- **Sliding-анимация.** Это основной расход CPU в простое у обеих версий. Её надо ограничить по fps (например, 30) и сделать отключаемой; в gate меряется отдельно.
- **Таблица/дерево.** Самая парадигмо-зависимая часть GTK-версии, около 4.8k LOC. Drag колонок заменяется контекстным меню, потому что TableBuilder его не умеет.
- **Внешний вид.** Не будет Adwaita один в один: нет нативных GTK-виджетов и accent color из системы. Accent можно позже брать через xdg-desktop-portal (`zbus`, чистый Rust).
- **Переводы.** Новые строки потребуют работы переводчиков; при совпадении msgid старые переводы сохранятся.
