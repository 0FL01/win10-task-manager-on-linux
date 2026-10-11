# Provenance and preserved notices

Task Manager is based on [Mission Center](https://missioncenter.io/) and remains licensed under
GPL-3.0-or-later (`COPYING`). Its statistics backend is magpie, its application logic was ported
from Mission Center, and its translations and embedded SVGs carry that project's history.
The Mission Center Developers copyright headers and About attribution are retained.

## Mission Center sources

The original root manifest at
[`671bc544ac4f7a3149c53db6df8a453252547d0a`](https://github.com/0FL01/win10-task-manager-on-linux/blob/671bc544ac4f7a3149c53db6df8a453252547d0a/Cargo.toml)
lists these authors:

- Jan Luca <janluca.oster@proton.me>
- jojo2357
- Romeo Calota <mail@romeocalota.me>

The removed GTK application (`src/`), Blueprint templates and SVG resources (`resources/`),
GSettings schema and Meson files are available in the
[historical root tree](https://github.com/0FL01/win10-task-manager-on-linux/tree/671bc544ac4f7a3149c53db6df8a453252547d0a).
`docs/port-spec/` is frozen evidence of those sources, not a map of the active build.
The SVGs used by the egui application now live in `crates/mc-ui/assets/icons/`.
`subprojects/magpie` contains the tracked sources from
[gng upstream](https://gitlab.com/mission-center-devs/gng/-/tree/49573fffefd0c1dc9c2ebcc6a24b0619b3403725)
at `49573fffefd0c1dc9c2ebcc6a24b0619b3403725`, with local host-side GPU telemetry,
process-identity and private-library bootstrap changes. Its GPL notice and authors are retained.
The nested `platform-linux/crates/app-rummage` dependency remains a submodule at
`26b93b8c6f0de6912f0a17022f7ece76bad87612` from
[app-detection](https://gitlab.com/mission-center-devs/app-detection).

## GPU libraries

NVTOP is built into magpie from revision `3d4a953da02bc18886734613bb9f60ff80669de7`;
the archive checksum and existing patches remain in `platform-linux/3rdparty/nvtop/`.
The new Rust collector reads formal DRM fdinfo counters and dynamically calls the installed
NVIDIA driver's NVML interface. No NVIDIA library or SDK is redistributed.

Packaged open libraries live in `lib/taskmanager/gpu`, separate from the UI's graphics-driver
lookup. Their generated `manifest.json` records actual source/package versions, architecture,
SONAMEs, checksums, symbol versions and preserved notices. Flatpak uses separate glibc/musl
bundles and checksum-pinned libdrm/eudev source notices; native builds use matching builder
package notices or explicitly supplied matching-source notices. These binaries are build
artifacts, not repository source assets.

## Local graph core

`crates/mc-graph-core/` carries the GUI-free `core/` portion of
[Mission Center graph-widget](https://gitlab.com/mission-center-devs/graph-widget), based on
[`8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc`](https://gitlab.com/mission-center-devs/graph-widget/-/tree/8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc/core).
It retains GPL-3.0-or-later and Mission Center Developers headers. Local dynamic-dataset fixes
are preserved: dataset groups grow to accept additional series and retain series for which an
update supplies no point. The core has no GUI dependencies; `crates/mc-graph/` supplies egui
rendering. The graph-widget GTK renderer and its submodule are no longer part of the active tree.

## Silver: original historical notice

The removed `rust-gui-silver-bullet/` experiment served as a reference during the GUI port.
The following is the entire original `LICENSES.md`, preserved verbatim. Its paths, fonts and
licensing statements describe that historical experiment, not the contents or license of the
current Task Manager application. Current bundled Selawik notices are in
`crates/mc-ui/assets/fonts/OFL.txt`.

```markdown
# License / provenance

- Исходный код **Silver** и оригинальный геометрический SVG в `workspace/assets/icons/spark.svg`: предоставлены по `MIT OR Apache-2.0` (пользователь может выбрать одну лицензию). Это новая реализация; большие участки WordCraft и PhotoCraft не скопированы.
- WordCraft: лицензии MIT OR Apache-2.0 согласно репозиторию; исследованы чтением исходников. Исходники, логотип, фотографии и исходные assets **не включены**.
- PhotoCraft: лицензии MIT OR Apache-2.0 согласно репозиторию; исследованы чтением исходников. Его shaders/assets **не включены**.
- egui/eframe/egui-wgpu: MIT OR Apache-2.0 (проверить конкретные лицензионные условия используемых версий при распространении сборки).
- Inter: SIL Open Font License 1.1; JetBrains Mono: SIL Open Font License 1.1. **Бинарные шрифты не включены**. `typography.rs` использует локально установленные версии, если они имеются, затем egui fallback.
- SVG иконки собственного дизайна, 20×20 Painter-геометрия также оригинальная.
- В каталоге нет фирменных логотипов WordCraft/PhotoCraft и нет растровых скриншотов, взятых из чужих приложений.

При распространении compiled app сохраняй license notices реально собранных транзитивных Rust crates и native libraries. Это не заявление об отсутствии системных зависимостей.
```
