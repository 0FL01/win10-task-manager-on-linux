# Goal: Wireless mouse battery, starting with MX Master 3S

Status: complete
Source: approved audited plan below; user: «делайю копию плана в цель, итеративно реализовать и коммит пуш», «задача считается выполненой после успешного прохождения github action (tool используй)».
Last updated: 2026-10-16

## Objective
Display the current host's Logitech MX Master 3S battery through the existing Performance battery UI, without running Task Manager or magpie as root. Commit and push the verified implementation; finish only after GitHub Actions succeeds on the pushed SHA and its expected artifacts/release are verified.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence. Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools, speculative risks, or optional source text. Finish when every required outcome is resolved and affected constraints remain satisfied. Stop substantive work at a proven external blocker, recording the exact evidence and smallest unlock.

## Frozen Contract

### Required Outcomes
- R1: Preserve the approved plan and resumable execution state.
  - Source: latest user instruction.
  - Acceptance: this goal contains the plan, current evidence and next checkpoint.
  - Primary evidence: this document and git history.
  - Status: verified.
  - Evidence: approved implementation plan copied below before runtime edits.
- R2: Display MX Master 3S model, valid charge and available charging state using the existing battery pipeline; preserve generic batteries and rootless operation with granted hidraw access.
  - Source: approved audited plan, steps 1–3; original request for MX Master 3S, universality and operation without root.
  - Acceptance: capability-based Bolt/0x1004 data reaches a normal Mouse Battery; real zero is valid, unknown is not zero; reconnect/source identity and late UI insertion work without changing mouse settings or depending on Solaar at runtime.
  - Primary evidence: focused protocol/publication/UI tests and an unprivileged getter smoke on the current host.
  - Status: verified.
  - Evidence: UPower has no mouse; existing ACL grants user stfu read/write on Bolt interface 02. The implemented Rust host_bolt_battery_getters smoke passed as UID 1000 with Solaar running: MX Master 3S, 50%, Discharging. Four protocol/publication/deadline tests and the UI lifecycle regression pass. Physical charging/replug and a cold connection before Solaar starts were not exercised; no claim of those manual observations.
- R3: Verify the affected runtime and document permissions and consulted sources.
  - Source: approved audited plan checks and documentation; AGENTS.md Verify.
  - Acceptance: focused tests, both workspace formatting checks, pinned Rust 1.96.1 workspace Clippy, root workspace tests and required backend tests pass; README/provenance accurately state rootless preconditions and implementation origin.
  - Primary evidence: exact commands/results and README/docs/PROVENANCE.md diff.
  - Status: verified.
  - Evidence: Rust 1.96.1 root and backend fmt checks, exact-toolchain workspace Clippy -D warnings, all root workspace tests (including existing renders), and backend release tests (86 passed; opt-in hardware test also passed separately). Logs: target/mouse-battery-{workspace,backend}-checks.log. README/provenance document permission scope and local implementation from protocol facts. Only preexisting third-party NVTOP C warnings appeared in backend build.
- R4: Commit, push and verify GitHub Actions on the resulting SHA.
  - Source: latest user finish line; AGENTS.md after-push rule.
  - Acceptance: workflow checks, five native builds and publish succeed; expected nonempty artifacts and any required release point to the pushed SHA.
  - Primary evidence: GitHub Actions tools plus artifact/release API.
  - Status: verified.
  - Evidence: implementation pushed to main as 0fb280b2549b3f6ffaf11fa5afa4b2e6e0f32255. GitHub Actions tools confirm Native packages run 38929371108 attempt 2 completed successfully: metadata, checks, all five native builds and publish. API verification confirms six nonempty, unexpired artifacts for that SHA and published v1.2.31 with five native packages plus source archive; release target and tag resolve to the implementation SHA. Initial Ubuntu checkout HTTP 503 was resolved by retrying failed jobs without source/CI changes.

### Constraints
- Existing UPower/sysfs behavior, battery protobuf, IPC and public frontend API remain unchanged.
- Collect only in host magpie; no blocking HID I/O in startup, UI, the single-thread Tokio runtime or the shared collector pool.
- Only HID++ getters: no receiver/device lifecycle, configuration flags, pairing, firmware, diversion or input grabs.
- No new dependencies, persistent state, source framework, privileged helper, automatic ACL changes or new Flatpak permissions.
- Separate generic collector state from HID++ data; merge only on reliable identity, never model names.
- Preserve Mission Center notices; document Solaar protocol sources and preserve donor notices if code is actually ported. Do not port GPL-2.0-only kernel implementation.
- No force push, unrelated fixes, private serials/dumps, new screenshot baselines or changes to host settings.

### Non-goals
- Full Solaar functionality; other receiver families, legacy battery features, direct Bluetooth/HID transports or closed protocols of other brands.
- New peripheral UI, global identity registry, strict IPC freshness SLA, automatic restoration of mouse selection.
- Repairing preexisting generic UPower limitations or Flatpak packaging. The manifest references absent crates/mc-magpie/spawner/spawner.rs; this is not part of the battery implementation.

## Change Envelope
- Runtime: subprojects/magpie/platform-linux/src/battery.rs, private battery/hidpp.rs; crates/mc-ui/src/perf/mod.rs late-insertion indexing only.
- Tests: private protocol/publication tests and nearest existing Performance UI unit tests, synthetic identities only.
- Docs: README.md, docs/PROVENANCE.md, this goal. No separate udev-rule artifact or installer.
- One dedicated HID worker and one bounded latest snapshot are explicitly approved; no further service/queue/API abstraction.
- Commit/push to the current main branch is authorized; main Rust changes automatically publish a native release. Inspect CI on each pushed SHA.

## Approved Plan (copy)

### Шаг 1. Проверить существующий путь

Без изменения настроек:
1. Проверить представление MX Master 3S в UPower и прохождение данных до UI.
2. Проверить реальные права пользователя backend на Bolt interface 02.
3. Выбрать первый достаточный вариант: существующий путь уже работает — runtime-diff не нужен; данные теряются в нём — исправить конкретное нарушение; системного источника нет — добавить HID++-сборщик.

### Шаг 2. При необходимости добавить узкий Bolt-сборщик

Область: subprojects/magpie/platform-linux/src/battery.rs и приватный battery/hidpp.rs.
- Только Bolt 046d:c548, interface 02, устройства типа mouse с 0x1004.
- std::fs::File и существующий libc; без новых dependencies.
- Один worker, первый опрос сразу, затем выбранный интервал около 30 секунд.
- Неблокирующие чтения, ограниченные readiness-waits и абсолютный deadline обработки ответов.
- Открывать endpoint на цикл, не оставляя несчитываемую очередь на весь интервал; очистку посторонних сообщений ограничить.
- Собственный software ID, один outstanding request на приёмник, строгое сопоставление ответов.
- Никаких settings, pairing, notification flags и lifecycle-вызовов Solaar.
- Полностью заменять результат завершённого цикла; не сохранять старый процент после известного отказа.
- Публиковать обычный Battery: Mouse, power_supply=false, процент 0…1, реальные модель/состояние. Без выдуманных Wh, мощности и истории.
- Не добавлять fallback в generic-вектор: его refresh использует devices.get_mut(...).unwrap().
- Сохранить локальный stop/wakeup/join, без глобальной переделки shutdown.
- Один непотребляемый snapshot с временем; проверка возраста при merge — локальный предохранитель, не SLA каждого IPC-ответа.

Протокольные уточнения аудита: проверять capability точного процента; 0% допустим; значения >100 отвергать. Unified Battery status 0→Discharging, 1/2→Charging, 3→FullyCharged, 4/unknown→None. Pairing serial и Unit ID не считать тождественными; slot — адрес, source key не зависит от hidrawN. Для межисточникового сравнения использовать подтверждённые нормализованные идентификаторы, при необходимости Unit ID из 0x0003.

### Шаг 3. Подключить существующий UI, проверить и документировать

- Исправить учёт добавлений в PerformancePage::update отдельно от удалений, включая same-count replacement.
- Не менять layout, protobuf, IPC, глобальные synthetic defaults и настройки.
- README: условия rootless, необязательный узкий udev-пример для interface 02, диагностика. ACL разрешает raw read/write, а не только безопасные getters.
- Provenance: фактически использованные источники Solaar и характер заимствования; копирование кода требует исходных notices.
- Узкие тесты: capability/percent/status; matching/errors/deadlines; полная замена snapshot, точный identity merge и смена pairing; UI BAT0→mouse→replacement→remove→return.
- Аппаратный smoke: getter без предварительного Solaar, затем coexistence; зарядка, недоступность и reconnect без дублей. Не утверждать выполнение недоступных ручных/холодных сценариев.
- Gates AGENTS.md без дублирования Win10 probes/snapshots поверх полного workspace test. Без новых screenshot эталонов, benchmarks и DPI/renderer matrix.

## Current Checkpoint
- No further implementation checkpoint: R1–R4 verified.
- Delivery only: commit/push this closure record and verify that documentation-only push with GitHub Actions tools. No new release is expected without unpublished Rust changes; do not create another documentation commit merely to embed its own SHA/run ID.
- Stop after the delivery gate; do not expand scope or rerun unchanged successful local gates.

## Current State
- Resolved: R1–R4. Native implementation, focused/full gates and release workflow passed; live Rust getter succeeded alongside Solaar without root or configuration writes.
- Last relevant evidence: [successful runtime workflow](https://github.com/0FL01/win10-task-manager-on-linux/actions/runs/38929371108), all eight jobs successful, six verified artifacts; [v1.2.31](https://github.com/0FL01/win10-task-manager-on-linux/releases/tag/v1.2.31) and its six uploaded assets target 0fb280b. No runtime changes after local verification.
- Blocker: none. Flatpak packaging remains an explicitly excluded preexisting limitation.
- Next: documentation-only closure delivery gate above, then stop.

## Material Decisions
- 2026-10-16: use the approved Bolt fallback because live UPower has no mouse; rootless ACL exists, without installing/changing any host rule.
- 2026-10-16: do not alter root-owned ~/.cargo/git; backend commands use CARGO_HOME=/home/stfu/.cache/opencode-tmp/opencode/taskmanager-battery-cargo (registry cache reused). Existing NVTOP download failed in ureq; fetched the same pinned archive with curl and verified its required SHA256 before staging in the ignored backend target. No build/runtime source changes for these environment issues.
- 2026-10-16: mesa-libgbm-devel is absent; download the matching Fedora RPM into the approved temporary directory and extract build files there, without sudo or host package changes. Existing libdrm/systemd development packages are installed.
- 2026-10-16: matching mesa-libgbm-devel RPM signature/digests verified. Backend commands additionally use PKG_CONFIG_PATH=/home/stfu/.cache/opencode-tmp/opencode/taskmanager-gbm-devel/root/usr/lib64/pkgconfig; only this extracted gbm.pc prefix and its local runtime-library link were adjusted. Host packages, ACLs and runtime files remain unchanged.

## Checkpoint History
- 2026-10-16: plan frozen; live source/access preflight completed; no runtime edits before this contract.
- 2026-10-16: getter-only prototype confirms the protocol on the live mouse; independent UI red→green regression completed and root gates passed. Backend compilation has not yet reached Rust checking because of local build prerequisites; resolve those without expanding the product diff.
- 2026-10-16: R2/R3 verified by four backend focused tests, live Rust host getter (50%, Discharging, 0.44 s test elapsed), backend release suite (86 passed) and both workspace formatting checks. Root gates remain current; ready for the authorized commit/push and CI finish line.
- 2026-10-16: implementation committed/pushed as 0fb280b; Actions tool confirms run 38929371108 on the matching main SHA. No runtime changes after local verification.
- 2026-10-16: first run completed with checks/four native builds successful; Ubuntu job 117069140339 failed solely on GitLab HTTP 503 fetching the unchanged app-rummage submodule. Requested failed-job retry (HTTP 201); Actions tool confirms attempt 2. No runtime/CI configuration changes for this external failure.
- 2026-10-16: R4 verified: attempt 2 succeeded, all eight jobs green. Artifact/release API verified six artifacts, v1.2.31 with six nonempty uploaded assets, and exact release/tag SHA 0fb280b2549b3f6ffaf11fa5afa4b2e6e0f32255. Closure check passes: all frozen outcomes verified; final product diff remains inside the six-file envelope and affected constraints remain satisfied.

## Completion
- Resolved outcomes: R1–R4, all verified with evidence above.
- Commands/artifacts: pinned-toolchain root fmt/Clippy/workspace tests; backend fmt/release tests; unprivileged Rust hardware smoke; successful Actions run 38929371108, six artifacts, release v1.2.31.
- Constraint/scope check: only private collector/merge, late UI indexing/regression and the three approved documents; no dependencies, schema/IPC/layout changes, privileged actions, host ACL changes or unrelated fixes.
- Final status: complete. Only commit/push of this record and its mandatory after-push verification remain; no further implementation or audit.
