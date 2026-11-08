# Consolidated Cooling page

## Outcome

The Performance sidebar listed every hwmon fan header as its own page ("Fan 0" … "Fan 8"), most
of them unused, each with a temperature taken from the `tempN` channel that merely shared the
fan's number. One **Cooling** entry replaces them, in the style of the consolidated batteries
page: a shared RPM graph, fan cards grouped by reporting device, a collapsed "Stopped (N)"
section and cards for every hwmon temperature sensor.

## Scope

- Backend: one hwmon inventory (fans + temperatures + device identity) published in the
  existing fans response; no fan/temperature pairing; periodic rescans; validity instead of
  zeros (`docs/hardware-monitoring.md`).
- Core: `DeviceType::Cooling`, key `cooling`, migration of legacy `fan-*` preferences,
  `performance-cooling-stopped-expanded`.
- UI: `perf/cooling.rs` (group model), `perf/fan.rs` and `perf/temperature.rs` (children with
  independent histories), `win10/performance.rs` `cooling_page_ui` sharing the card, detail and
  scroll helpers with the battery page. Visual contract: `docs/win10-spec/spec.md`,
  "Consolidated cooling".

## Acceptance

- One sidebar row; legacy per-fan selection/order/visibility fold into it.
- Absolute RPM on one axis; card legends use the exact line inks in both themes.
- 0 RPM fans collapse into "Stopped (N)"; the expansion persists; unknown is not stopped.
- Sensors show real zeros, unknown as `—`, limit bars only for plausible limits.
- Evidence: `perf::cooling`/`perf::temperature` unit tests, `migrate_cooling_group` tests, the
  collector fixture tests in `platform-linux/src/fan/mod.rs`, and the
  `performance_cooling_groups_stopped_fans_and_details` probe (renders
  `target/ui-snapshots/win10-performance-cooling-*.png`).

## Fan glyph animation

Fan glyphs (sidebar and cards) turn with their fan's RPM — 1/20 of the real speed, capped at
3 rev/s, eased between readings, at rest on a quarter turn — at the display rate while visible
(`win10/fan_spin.rs`, spec "Consolidated cooling"). View → "Animate fans" turns it off; paused
updates stop it. Evidence: `fan_spin` unit tests, the
`performance_cooling_fan_glyphs_animate_only_when_useful` probe and the Cooling scenarios of
`cargo xtask bench-idle` (`perf/README.md`, "Fan glyph animation").
