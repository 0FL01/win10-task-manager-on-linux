# Goal: порт GUI Mission Center → Task Manager с GTK4/libadwaita на egui/eframe

Status: active
Source: инструкция пользователя от 2026-10-10 («план рефакторинга GUI с GTK на Pure rust», подсказка `rust-gui-silver-bullet/`); утверждённый план в [2026-10-10-egui-gui-port-plan.md](2026-10-10-egui-gui-port-plan.md)
Last updated: 2026-10-10

> Historical GTK source paths below resolve against the
> [root tree at `671bc544ac4f7a3149c53db6df8a453252547d0a`](https://github.com/0FL01/win10-task-manager-on-linux/tree/671bc544ac4f7a3149c53db6df8a453252547d0a).
> Graph-widget references resolve against
> [upstream `8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc`](https://gitlab.com/mission-center-devs/graph-widget/-/tree/8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc).
> Cleanup removes these active paths; it does not by itself verify R1–R8 or functional parity.

## Objective
Task Manager 1.2.0 (на основе Mission Center) собирается через `cargo build` и работает как egui/eframe-приложение с функциональным паритетом с GTK-версией 1.2.0. GTK4, libadwaita, Blueprint, GSettings, gresource и meson из репозитория удалены. Продолжение после вертикального среза разрешается только при пройденном decision gate по замерам.

## Execution Directive
Выполнить замороженные Required Outcomes в рамках Change Envelope, подтверждая каждый через Primary Evidence. Брать наименьший нерешённый outcome. Не добавлять требования из ревью, тестов, инструментов, спекулятивных рисков или опционального текста источника. Цель закрыта, когда каждый outcome решён, а затронутые ограничения по-прежнему выполняются. Существенную работу остановить при доказанном внешнем блокере, на границе утверждённого бюджета или когда у оставшихся действий в рамках объёма нет фальсифицируемого ожидаемого результата; записать точное доказательство и минимальное условие разблокировки.

## Frozen Contract

### Required Outcomes
- R1: Bootstrap
  - Source: план, Фаза 0
  - Acceptance:
    - сабмодули `subprojects/magpie` и `subprojects/graph-widget` закреплены на коммитах upstream v1.2.0 с абсолютными URL;
    - GTK-версия один раз собрана через meson и служит baseline-бинарником;
    - корневой `Cargo.toml` стал workspace, в `default-members` нет legacy GTK;
    - в `AGENTS.md` записаны слои и инварианты из плана.
  - Primary evidence: `git submodule status`; `ninja -C _build` проходит; `cargo metadata` показывает workspace members; diff `AGENTS.md`
  - Status: pending
  - Evidence:
- R2: GUI-независимые крейты `mc-core` и `mc-magpie`
  - Source: план, Фаза 1
  - Acceptance:
    - ни в одном из двух крейтов нет зависимостей gtk/glib/adw;
    - данные из gatherer'а приходят через latest-slot + waker, блокирующих вызовов в UI-пути нет, fatal-ошибки идут событием;
    - есть fixture source и `--dump-readings`;
    - 26 существующих тестов перенесены и проходят.
  - Primary evidence: `cargo test -p mc-core -p mc-magpie`; `cargo tree -p mc-core -p mc-magpie | grep -E 'gtk|glib|adw'` ничего не выводит
  - Status: pending
  - Evidence:
- R3: вертикальный срез
  - Source: план, Фаза 2; выбор пользователя «Срез → полный паритет»
  - Acceptance:
    - eframe-приложение показывает страницы CPU и Memory с egui-графиками (`mc-graph`), а также виртуализированный список процессов с сортировкой, поиском и terminate/kill;
    - работает на фикстурах и на настоящем magpie;
    - собирается с feature `wgpu` и с feature `glow`;
    - есть perf overlay.
  - Primary evidence: `cargo run --release -p taskmanager-app -- --fixture ...` и `cargo xtask run-dev` на KDE Wayland; kittest-снимки, просмотренные глазами
  - Status: pending
  - Evidence:
- R4: decision gate по замерам
  - Source: план, Фаза 3
  - Acceptance:
    - `cargo xtask bench-idle` сравнивает GTK baseline с egui (wgpu, glow) по критериям плана;
    - `perf/RESULTS.md` и `perf/budgets.toml` заполнены реальными цифрами;
    - записано решение Go/No-go и выбранный рендерер.
  - Primary evidence: `perf/RESULTS.md` с окружением и цифрами
  - Status: pending
  - Evidence:
- R5: паритет страницы Performance
  - Source: план, Фаза 4
  - Acceptance:
    - все страницы устройств, summary mode, сайдбар (reorder, edit mode, hidden graphs, overrides) и диалоги SMART/eject работают как в 1.2.0;
    - «Copy summary» работает.
  - Primary evidence: ручной smoke по чек-листу Фазы 4 + kittest-снимки на фикстурах
  - Status: pending
  - Evidence:
- R6: паритет Apps и Services
  - Source: план, Фаза 5
  - Acceptance:
    - Apps: дерево приложений и процессов, все 8 сигналов, details, настройки колонок с сохранением, иконки приложений;
    - Services: start/stop/restart/enable/disable и логи с копированием.
  - Primary evidence: ручной smoke по чек-листу Фазы 5 + kittest-снимки
  - Status: pending
  - Evidence:
- R7: остальная функциональность
  - Source: план, Фаза 6
  - Acceptance:
    - Preferences (4 страницы), About system, first-run и setup script, shortcuts, сохранение состояния окна, interface style, адаптивная раскладка;
    - все строки из `.blp` перенесены в Rust с идентичными msgid и контекстами, `xtask pot` работает.
  - Primary evidence: ручной smoke; `cargo xtask pot` и сравнение msgid со старым `po/missioncenter.pot`
  - Status: pending
  - Evidence:
- R8: Cargo + xtask, GTK удалён
  - Source: план, Фаза 7; выбор пользователя «Cargo + xtask»
  - Acceptance:
    - `cargo build` и `cargo xtask install` работают;
    - AppImage и Flatpak обновлены;
    - из дерева и `Cargo.lock` удалены `gtk4`/`libadwaita`/`gettext-rs`, `.blp`, gschema, meson и submodule graph-widget;
    - README, AGENTS.md и CI обновлены.
  - Primary evidence: `cargo build --release`; `grep -E 'name = "(gtk4|libadwaita|gettext-rs)"' Cargo.lock` ничего не находит; `cargo xtask install --destdir /tmp/mc`
  - Status: in_progress
  - Evidence: approved cleanup removes the legacy tree and makes the root a pure workspace; local graph core is `crates/mc-graph-core`, embedded SVGs are `crates/mc-ui/assets/icons`. Cargo-only build, install, RPM, offline frontend dependencies and runtime smoke passed at the 2026-10-11 checkpoint below. Full AppImage/Snap/Flatpak artifact tests are not claimed.

### Constraints
- C1: лицензия GPL-3.0-or-later сохраняется; код, взятый из `rust-gui-silver-bullet/` (MIT/Apache), сохраняет уведомление.
- C2: данные собирает существующий magpie-демон; NNG не заменяется.
- C3: инварианты UI из плана: в `ui()`/`logic()` нет I/O и IPC; неограниченных каналов нет; repaint только по событию; в свёрнутом окне 0 кадров; нет `Arc<Mutex<AppState>>`.
- C4: msgid переводов совпадают со старыми, чтобы переводы Weblate продолжили работать.
- C5: `cargo fmt` и `clippy -D warnings` на новых крейтах; в `perf/RESULTS.md` и снимках только реально полученные данные.

### Non-goals
- Замена magpie собственным сбором метрик через procfs/sysfs; замена NNG.
- GPU-canvas, COW-тайлы, undo и docking из `rust-gui-silver-bullet/`.
- Пиксельное совпадение с Adwaita; системный accent color.
- Правки замороженного GTK-кода в `src/`, кроме `-p missioncenter` в `src/meson.build`, до его удаления в R8.

## Change Envelope
- Target: `packaging/rpm/` (запрос пользователя), workspace (`crates/mc-core`, `crates/mc-magpie`, `crates/mc-graph-core`, `crates/mc-graph`, `crates/mc-ui`, `apps/taskmanager`, `xtask`, `perf/`, `testkit/fixtures`), корневой `Cargo.toml`, `.gitmodules`, `AGENTS.md`, README, `docs/PROVENANCE.md`. Ограничение на `src/meson.build` относилось к bootstrap; при утверждённой зачистке legacy-дерево удаляется.
- Expected consumers: `po/`, `data/` (desktop, metainfo, icons), `flatpak/`, `support/` (AppImage), `.gitlab-ci.yml`, README; в R8 удаляются `src/`, `resources/`, meson-файлы.
- Allowed deps: egui/eframe/egui_extras 0.36, egui_kittest, serde/toml, log, gettext (чистый Rust), image, freedesktop-icons, magpie-types. Любой другой крейт, сервис или фоновый процесс сверх перечисленных в плане — это расширение envelope, его нужно сначала записать.
- Forbidden: WebView/Tauri, Qt/GTK/Slint как основной UI (если R4 не дал No-go), коммиты без запроса пользователя, секреты и дампы в документах.
- User or harness budget: не задан.

## Current Checkpoint
- Closes: R8 (утверждённая зачистка исходников и сборки; не весь функциональный паритет).
- Smallest next action: опубликовать проверенный cleanup по запросу пользователя; не закрывать оставшиеся acceptance items общего порта без их evidence.
- Expected evidence: `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, release build и staged install; проверки упаковки в её текущем объёме.
- Stop or replan if: текущая упаковка всё ещё требует удалённый Meson/GTK-путь; исправлять только непосредственного потребителя.

## Current State
- Cleanup: active GUI — egui; root — pure Cargo workspace, graph core — local, required submodule — magpie only. Legacy GTK/Meson and the Silver experiment are removed; provenance is retained in `docs/PROVENANCE.md`.
- Historical evidence: earlier checkpoints reported bootstrap, the data seam and vertical-slice smoke/snapshots. They are not a fresh verification of every acceptance item.
- Last relevant benchmark evidence: `perf/history/bench-latest.md`, `perf/history/bench-win10.md`, `perf/history/bench-win10-full.md`; validity and coverage limitations are documented in `perf/README.md`.
- R4 remains pending as a full gate: the planned `perf/RESULTS.md` and `perf/budgets.toml` do not exist; historical benchmark tables do not cover every criterion. Glow is the selected default, based on historical evidence.
- Functional parity R5–R7 remains subject to its recorded smoke and translation checks; cleanup does not close those outcomes.
- Blocker: —
- Next: commit/push the verified cleanup; keep outstanding full-port parity/gate evidence explicit.
- Спецификации GTK-страниц: `docs/port-spec/*.md` (замороженные исторические пути, ссылки в заголовках).

## Material Decisions
- 2026-10-10: пользователь заменил внешний вид на диспетчер задач Windows 10 (отдельная цель `docs/goals/2026-10-10-win10-skin.md`); визуальная часть паритета R5–R7 (Adwaita-вид) заменяется ей, функциональный паритет остаётся. Историческое сравнение: `perf/history/bench-latest.md` (egui 0.75% CPU vs GTK 3.12%, PSS 53 vs 140 МиБ, glow 103 МиБ GPU vs wgpu 423); это таблица замеров, не подтверждение всех критериев R4.
- 2026-10-10: порт делается в этом репо под GPL-3.0, переиспользуются magpie и `magpie_client` (выбор пользователя).
- 2026-10-10: сначала срез и decision gate, затем полный паритет (выбор пользователя).
- 2026-10-10: Cargo + xtask вместо meson (выбор пользователя).
- 2026-10-10: локальный импорт совпадает с upstream `main` f4109d6 (не тег v1.2.0, в котором ещё нет graph-widget); сабмодули закреплены на коммитах этого коммита: magpie 49573fff, graph-widget 8aadf1cd.
- 2026-10-10: `mc-graph-core` (GPL-3, без зависимостей) переиспользуется для данных/масштабирования графиков; mc-graph только рисует.
- 2026-10-10: расширение envelope: `textdistance`, `regex`, `arrayvec`, `nng-c(-sys)`, `rand`, `rust-ini`, `prost` (через magpie-types), `serde_json`, `env_logger` — существующие зависимости GTK-приложения/фикстур, нужны для переноса без изменения поведения.
- 2026-10-10: заголовок страницы диска всегда «Drive N (id)» (GTK показывал «Disk N (id)» до первого обновления — баг); msgid «Disk {} ({})» не используется.
- 2026-10-10: тосты (Memory: «Getting additional memory information failed») показываются подписью на странице; filter-кнопки Services без перевода, как в blueprint.
- 2026-10-10: перестановка колонок — через контекстное меню заголовка (⏴/⏵) вместо drag; разделы Apps/Processes всегда в фиксированном порядке при сортировке.
- 2026-10-10: снимки экрана только офскрин (egui_kittest); захват экрана исключён после случайного захвата чужого окна (файл удалён).
- 2026-10-10: Command/AppState держатся в mc-ui (UI-уровень), mc-core — доменная логика без GUI.
- 2026-10-10: `rust-gui-silver-bullet/` использовался как справочник (правила, Tokens, typography, kittest), но не как член workspace; эксперимент удалён при зачистке, исходный notice целиком сохранён в `docs/PROVENANCE.md`.
- 2026-10-10: пользователь утвердил атомарную зачистку legacy GUI/build/packaging: удалить GTK `src/`, `resources/`, Meson/Blueprint/GSettings, graph-widget submodule и Silver experiment; сохранить magpie, локально перенести GUI-free graph core с dynamic-dataset fixes и используемые SVG. Все три `perf/bench*.md` перенесены побайтово в `perf/history/`; текущий `bench-idle` — только egui wgpu/glow с Cargo-built daemon/hwdb. Документы/CI/упаковка обновляются без автоматического закрытия R1–R8 или объявления функционального паритета.

## Checkpoint History
- 2026-10-11: cleanup execution verified: `cargo fmt --all --check`, `cargo check --workspace --locked`, `cargo test --workspace --locked` (125 passed, 4 existing ignored), `cargo clippy --workspace --all-targets --locked -- -D warnings` and release build passed. All 37 migrated SVGs are byte-identical to the historical resources; graph core's 4 regression tests remain. Fresh Cargo daemon + Python hwdb, full staged install and RPM rebuild passed without `_build/`; desktop/AppStream metadata validate, RPM contains the backend, locales, eight PNGs and license notices. All 530 frontend Flatpak archive names/checksums match `Cargo.lock` and a vendored offline app/xtask check passed. Nested KWin smoke rendered synthetic 300 and real `run-dev` readings; the six egui-only benchmark combinations each had 1/1 valid runs with 20 s warmup and 4 s measurement (`target/cleanup-bench.md`, local smoke evidence, not a full R4 gate). Environment workarounds stayed under ignored `target/`: writable Cargo git cache, locally extracted GBM development package and checksum-verified nvtop archive download. No system package installation or submodule source edits. Full Snap/AppImage/Flatpak artifact builds and functional parity are not inferred from these checks.
- 2026-10-10: R8 documentation/CI/metadata cleanup checkpoint: README/AGENTS/provenance/history markers updated; benchmark moves preserve all three SHA-256 hashes; nonmutating fmt, shell syntax, CI/Snap YAML, AppStream validation and RPM spec parse pass. This is documentation/metadata evidence only; workspace runtime/install/package gates and functional parity remain open.
- 2026-10-10: по запросу пользователя добавлена сборка Fedora RPM: исторический `packaging/rpm/missioncenter.spec` (теперь `packaging/rpm/taskmanager.spec`) + `cargo xtask rpm` → `target/rpmbuild/RPMS/x86_64/missioncenter-1.2.0-1.fc44.x86_64.rpm` (бинарная пересборка staged-дерева; `rpm -i --test` проходит; без зависимостей от GTK). Бенчмарк R4 перезапущен через `setsid`, результаты теперь в `perf/history/bench-latest.md`.
- 2026-10-10: контракт заморожен, план скопирован в репо.
- 2026-10-10: R1 почти закрыт (сабмодули, baseline, workspace); R2: крейты и тесты готовы.

## Completion
- Resolved outcomes:
- Commands and artifacts:
- Constraint and diff-scope check:
- Final status:
