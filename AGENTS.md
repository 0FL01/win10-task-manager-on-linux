# Mission Center

GTK4/libadwaita system monitor (CPU, memory, disk, network, GPU) in Rust. Meson wraps Cargo for all builds and tests.

## Map
- `src/` — app code: `main.rs`/`window.rs`, pages in `apps_page/`, `performance_page/`, `services_page/`; shared UI in `table_view/`, `widgets/`.
- `src/magpie_client/` — NNG IPC client to the magpie stats daemon; data collection itself lives in the magpie submodule.
- `resources/ui/**/*.blp` — Blueprint UI definitions (not GTK XML). New `.blp` files must be registered in `resources/meson.build`.
- `subprojects/magpie` — submodule (gng): stats daemon; provides the `magpie-types` path dependency.
- `subprojects/graph-widget` — submodule: graph rendering crate used for all charts.
- `data/` — GSettings schema, icons, desktop/appdata.
- `po/` — gettext translations (managed via Weblate).
- `support/` — fmt check and AppImage/packaging scripts.

## Rules
- Submodules are required: run `git submodule update --init --recursive` after cloning, or the build fails.
- No bare `cargo build`/`cargo test` from the repo root: `src/main.rs` includes the generated config via `env!("BUILD_ROOT")`. Always go through meson (see Verify).
- `src/config.rs` is generated from `src/config.rs.in` by meson — edit the `.in` template, never the output.
- Run `cargo fmt` after touching Rust code; CI enforces it via `support/check-cargo-fmt.sh`.
- Native builds need GTK 4.22, libadwaita 1.9, Rust >= 1.90 (roughly Arch >= 20260501 / Fedora >= 44 / Ubuntu >= 26.04).

## Verify
- `meson setup _build -Dbuildtype=debug && ninja -C _build` — build (incremental `ninja` is the fastest type check).
- `meson test -C _build --suite missioncenter --print-errorlogs` — run tests (wraps `cargo test`).
- `cargo fmt` — before committing.

## Docs
- `README.md` — full dependency list, Flatpak/AppImage packaging, and env vars (`GSETTINGS_SCHEMA_DIR`, `MC_RESOURCE_DIR`, …) needed to run the app from the build dir.
