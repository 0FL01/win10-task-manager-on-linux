# Goal: Iterative resource optimizations

Status: active
Source: user-approved plan below; user instruction: «Делай копию плана в цель и итеративно реализовать и коммит».
Last updated: 2026-10-12

## Objective
Reduce backend/UI CPU and temporary allocations without reducing metrics, refresh frequency or functionality. Implement the approved slices separately, retain only measured improvements, and commit verified results.

## Execution Directive
Complete the frozen outcomes using the listed change envelope and primary evidence. Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools or speculative risks. Finish when every required outcome is resolved and affected constraints remain satisfied. A candidate rejected by the approved measurement gate is a completed experiment, not a retained optimization.

## Frozen Contract
- R1: Copy the approved plan into this goal and freeze the existing diagnostic baseline.
  - Source: latest user instruction and plan §0.
  - Acceptance: runnable copied bundle, binary hashes/toolchain/workload recorded; baseline diagnostic work committed separately from optimization candidates.
  - Primary evidence: baseline artifact/metadata under `target/diagnostic/opt-baseline/`, commit and existing baseline reports.
  - Status: verified.
  - Evidence: diagnostic commit `0f3c6fa`; full bundle copied under `target/diagnostic/opt-baseline/`, hashes/toolchain/workload in its `BUILD.md`; relocated daemon --test passed. Goal contains the approved plan and frozen per-slice outcomes.
- R2: Test the fdinfo reader without metadata sizing; retain only measured benefit.
  - Source: plan §1.
  - Acceptance: complete reads/UTF-8/error parity, no skipped FDs or changed collector semantics; tests and paired quiet/profile observations support retaining or reverting.
  - Primary evidence: reader/fdinfo tests, syscall summary, paired benchmark/profile report, commit.
  - Status: pending.
- R3: Test a snapshot-local PID-to-client index with full-scan parity.
  - Source: plan §2a.
  - Acceptance: identical aggregation and state semantics; measured CPU/allocations benefit without confirmed resource regression, or documented rollback.
  - Primary evidence: core GPU parity tests, synthetic/live benchmark/profile observations, commit.
  - Status: pending.
- R4: Test reuse of process GPU stats for process rows only.
  - Source: plan §2b.
  - Acceptance: missing-entry fallback retained; app/service groups still aggregate unique clients; measured benefit or documented rollback.
  - Primary evidence: table/GPU tests, benchmark/profile observations, commit.
  - Status: pending.
- R5: Test num_threads from already-read stat with the existing task-directory fallback.
  - Source: plan §3.
  - Acceptance: valid zero retained; malformed/missing/overflow fields fall back; CPU/I/O/memory baselines and CPU-page independent collection unchanged; measured benefit or documented rollback.
  - Primary evidence: stat/count tests, process/profile/syscall observations, commit.
  - Status: pending.
- R6: Attribute Apps stages; test the compact process snapshot only if preparation is a material expense.
  - Source: plan §4.
  - Acceptance: bounded fixed-stage reporting; preserve grouping/icons/invalidation and short process-cache lock. Conditional candidate requires evidence and measured benefit, otherwise record why it was not selected.
  - Primary evidence: Apps profile and relevant tests/conditional comparison, commit.
  - Status: pending.
- R7: Run final required gates, close this goal and commit the verified result.
  - Source: latest user instruction and plan criteria.
  - Acceptance: root/backend checks pass on Rust 1.96.1; final bundle runs; all retained changes and goal evidence committed. No push requested.
  - Primary evidence: fmt/workspace tests/backend release tests/Clippy/runtime evidence and git history/status.
  - Status: pending.

### Constraints and Non-goals
- Preserve Readings/fixtures, refresh frequency, metric completeness, PID/start-time checks, unknown/idle/partial states, client/device deduplication and busiest-engine percentages. Device memory does not come from process sums.
- No I/O/logging/blocking or unconditional repaint in UI/logic; existing bounded hand-offs and concurrency guarantees remain.
- Keep attribution and daemon-only relative GPU RUNPATH/host driver loading. No global loader/security override, system installation/root, new dependency/service, allocator/IPC/backend rewrite, app-rummage submodule change or push.
- Raw diagnostics and build artifacts remain git-ignored under `target/`; do not commit private process data.

## Change Envelope
- GPU: `platform-linux/src/gpus/fdinfo.rs` private reader and nearest tests.
- UI: `crates/mc-core/src/{gpu,table}.rs` private snapshot index/process-row reuse and nearest parity tests.
- Processes: `platform-linux/src/processes/{stat,mod}.rs` stat accessor/fallback and nearest tests.
- Apps: `platform-linux/src/apps.rs`, `platform/src/profile.rs`, nearest bounded-report tests; optional compact snapshot only when evidence selects it.
- Existing diagnostic bundle/benchmark paths, minimal measurement helpers if required by the evidence surface, and this goal. Toolchain Rust 1.96.1; backend workspace separate.

## Current Checkpoint
- Closes: R2.
- Smallest next action: implement the private complete-read fdinfo reader, run parity tests, rebuild the candidate and compare it against the frozen bundle.
- Expected evidence: fewer metadata syscalls plus repeated CPU/call and quiet benchmark benefit without functional/resource regression; retain or revert before the next slice.

## Current State
- Diagnostic work committed as `0f3c6fa` from initial base `fce5c3c`; current bundle at `target/diagnostic/bundle/`, frozen baseline at `target/diagnostic/opt-baseline/`.
- Existing quiet/profile benchmarks: 8 s warm-up, 40 s measurement, 3 repeats, both renderers, live CPU page and 1000-process synthetic fixture; all 3/3 valid.
- Existing root/client/UI/backend/xtask tests and Rust 1.96.1 Clippy passed. Final native smoke emitted UI and daemon final summaries.
- Read-only RECON verified Rust 1.96.1 metadata sizing in both string-reading helpers, repeated UI full-client scans, and stat field 20 num_threads. No optimization from this plan implemented yet.

## Approved Plan (copy)

### Цель
Снизить CPU и временные аллокации backend/UI, сохранив частоту обновления, набор метрик и функционал. Каждое изменение проверять отдельно в локальном bundle, без установки и root.

### Что нашлось
| Участок | Факт из кода и профиля | Направление |
|---|---|---|
| GPU fdinfo | ~150–170 мс CPU на обновление; читается каждый fdinfo | Убрать ненужный запрос размера файла, не пропуская FD |
| UI GPU | Каждый PID перебирает всех GPU-клиентов; таблицы повторяют расчёт | Индекс внутри snapshot + использование рассчитанных значений |
| Processes | После чтения stat отдельно перечисляются /proc/PID/task | Взять число потоков из уже прочитанного stat |
| Apps | ~50 мс CPU на обновление, но внутренние стадии пока не разделены | Сначала локализовать расход; не вводить кэш наугад |

В одном из синтетических прогонов с 1000 процессов gpu-snapshot занимал около 20 мс CPU на обновление, а table.gpu — ещё около 17 мс суммарно на таблицы. Это отдельные стадии; вложенные таймеры к ним не прибавляю.

### 0. Зафиксировать сравнимую baseline
Артефакты: отдельная копия текущего bundle под target/diagnostic/opt-baseline/, новые отчёты рядом.
- Сохранить оба бинарника и относительную структуру библиотек/data.
- Записать hashes бинарников, Rust 1.96.1, renderer, интервал и параметры запуска.
- Использовать bench-idle: 8 секунд прогрева, 40 секунд измерения, 3 повтора, оба рендерера.
- Quiet CPU/PSS и profiling запускать раздельно.
- Каждый следующий слайс сравнивать с предыдущим подтверждённым состоянием; при небольшом эффекте повторять парное A/B-сравнение, а не сравнивать только с цифрами, снятыми час назад.

### 1. GPU: читать procfs без запроса размера файла
Файл: subprojects/magpie/platform-linux/src/gpus/fdinfo.rs; Collector::collect, чтение fd.path().

Rust 1.96.1 fs::read_to_string вызывает file.metadata для резервирования буфера; File::read_to_string тоже запрашивает metadata через buffer_capacity_required. Прошлый эксперимент с File::open + read_to_string сохранил этот системный вызов. Для procfs запрос размера не нужен: содержимое всё равно необходимо читать до EOF.
Источник: https://github.com/rust-lang/rust/blob/1.96.1/library/std/src/fs.rs

Добавить небольшой приватный reader в этом же файле:
- File::open, затем цикл обычного Read::read;
- один переиспользуемый Vec<u8> на проход collect;
- чтение до EOF без фиксированного ограничения длины;
- UTF-8 проверять после полного чтения;
- short read не считать EOF; Interrupted повторять;
- ошибочный/частично прочитанный текст не передавать парсеру.

Не менять parse, device_for, проверки PID/start time, группировку и unavailable/stale. Metadata для идентификации GPU-устройства оставить — убирается только запрос размера текста.

Сейчас: open → metadata для размера → read… → parse.
Будет: open → read до EOF в общий буфер → UTF-8 → parse.

Тесты reader: пустой файл/последовательное использование буфера, short reads/Interrupted/ошибка после частичного чтения, invalid UTF-8, большой fdinfo с DRM-полями в конце. Существующие fdinfo-тесты сохраняют dedup/warmup/PID reuse/permission/stale/unknown против idle.

Отдельный короткий strace -f -c проверяет сокращение metadata-вызовов; CPU под strace не является benchmark. Основной результат — quiet daemon CPU и gpu.fdinfo CPU/call при сопоставимой нагрузке.

### 2. UI: убрать полный перебор клиентов для каждого PID
#### 2а. Индекс клиентов внутри snapshot
Файл: crates/mc-core/src/gpu.rs; Client, GpuSnapshot::from_readings, GpuSnapshot::aggregate.

Сейчас каждый PID/группа проходит весь clients и проверяет owners; для одиночных PID выборка порядка P × C.
- При построении snapshot создать приватный PID → Vec<client_index>.
- Включать только owners, прошедших существующую проверку start time.
- В aggregate получать индексы для PID, сортировать и дедуплицировать.
- Сохранять исходный порядок клиентов по индексам, порядок суммирования и выбора состояний.
- Убрать постоянный Client.owners, если его полностью заменяет индекс.
- Индекс живёт только внутри одного snapshot, не является долгоживущим PID-кэшем.

#### 2б. Не пересчитывать одиночные process rows
Файл: crates/mc-core/src/table.rs; Tree::apply_gpu.
- RowKind::Process берёт уже рассчитанный gpu.processes[pid].
- При отсутствии записи сохраняется aggregate([pid]), не default.
- Apps/Services сохраняют объединение PID/descendants и агрегацию уникальных клиентов. Нельзя складывать готовые process percentages: shared clients задублируются, SUM(MAX) заменит MAX(SUM).

Проверки gpu.rs/table.rs: shared client у нескольких PID/повторяющиеся PID, одинаковый client ID на разных GPU, stale/repeated sample/PID reuse/отсутствующие observations, idle против unknown/partial memory/overflow, равенство process row snapshot.processes, группы с пересекающимися descendants. Индексированный выбор сравнивается с test-only полноскановым эталоном.

Замеры: ui.gpu-snapshot, table.gpu, ui.apply, UI CPU/allocations; allocations — отдельный rootless heaptrack UI, не смешивать его времена с quiet benchmark.

### 3. Processes: не перечислять task directory ради одного числа
Файлы: subprojects/magpie/platform-linux/src/processes/{stat,mod}.rs; stat accessors, task_count в ProcessCache::refresh.

Поле 20 /proc/PID/stat содержит num_threads; файл уже прочитан и разобран.
Источник: https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html
- Добавить accessor num_threads для элемента 19 в zero-based массиве.
- Использовать для process.task_count.
- Отсутствующее/невалидное поле оставляет нынешний обход /task и прежние ошибки.
- Валидный ноль не подменяется единицей.
- CPU/I/O baselines и memory accounting не меняются. CPU-page total thread count не брать из process cache: домены обновляются независимо.

Тесты: обычный stat, comm с пробелами/скобками, разные числа потоков/ноль/truncated/malformed/overflow поле; стабильный многопоточный процесс — сопоставить stat и task directory. Замеры proc.scan CPU/call и directory-вызовы; пятисекундный maps walk неизменен.

### 4. Apps и RAM: сначала разделить расход
Файлы: platform-linux/src/apps.rs, platform/src/profile.rs.

Apps уже используют опубликованный process cache, не делают второй production-обход /proc. Разделить таймер на фиксированные стадии: installed-app inventory, подготовка process snapshot, matching/cgroup walk, разрешение новых icons. Сохранить reporter, интервал и лимит; нет записей на PID/app.

Первый определённый кандидат: заменить ProcessNewType(Process) компактным snapshot pid/name/exe/cmd, используемыми app-rummage. Не удерживать process-cache lock во время cgroup/filesystem matching. Кандидат делается только если подготовка snapshot заметна; если доминирует matching/inventory — завершить слайс с конкретными измерениями, без PID-кэша.

PSS/RSS — весь процесс, не функция; heaptrack UI не профилирует daemon автоматически, launcher удаляет LD_PRELOAD. Synthetic PSS включает fixture readings, не является чистой ценой UI на 1000 процессов.

### Критерии done и проверки
Для каждого оставленного изменения:
1. Функциональные тесты зелёные.
2. Есть воспроизводимый выигрыш целевой CPU/allocations-метрики, не только меньше строк.
3. Нет подтверждённой регрессии соседних CPU/PSS/UI latency.
4. Все benchmark runs валидны; unknown не записан как zero.
5. Неподтверждённый кандидат откатан.

После отдельных слайсов — targeted tests; перед завершением: root/backend formatting checks, cargo test --workspace с UI snapshots/probes, отдельные backend Linux/types/profile release-тесты, workspace Clippy Rust 1.96.1 с -D warnings.

### Риски и вне scope
Не включать фильтрацию FD до чтения (FD reuse race), межобновленческий FD/PID-кэш, снижение частоты/пропуск страниц/удаление метрик, увеличение Rayon pool, allocator/IPC/backend rewrite или app-rummage submodule edits без доказанного расхода там.

Первый шаг — fdinfo reader без metadata, затем отдельно индекс GPU-клиентов и reuse process GPU stats: убрать лишнюю работу без потери полноты данных.

## Checkpoint History
- 2026-10-12: contract frozen; diagnostic baseline is prior verified work, optimization experiments not yet started.
- 2026-10-12: R1 verified; diagnostic baseline committed/frozen with hashes and toolchain metadata, relative daemon smoke passed. Next: fdinfo reader experiment only.
- 2026-10-12: R2 reader parity tests passed (12 fdinfo tests), candidate bundle built. Baseline quiet runs valid, but all subsequent candidate runs lacked exit stats; identical UI hash and synthetic/native reduced reproduction stalled at two logic passes while host screen locker was active. Diagnose isolated virtual KWin lockscreen handling, without unlocking/changing the host session; minimum benchmark helper change is within the envelope. Discard invalid runs and repeat both sides under identical launcher options.
- 2026-10-12: isolated virtual KWin `--no-lockscreen` restored all four short reproduction runs (1/1 per scenario/renderer). Retain the one-flag harness fix and document it; rerun the full baseline/candidate comparisons with that same launcher.

## Completion
- Pending final outcome verification and commits.
