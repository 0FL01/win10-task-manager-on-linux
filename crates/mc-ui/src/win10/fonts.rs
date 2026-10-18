/* crates/mc-ui/src/win10/fonts.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Fonts of the Win10-style interface: Segoe UI when installed, otherwise the bundled Selawik
//! (SIL OFL 1.1, `assets/fonts/OFL.txt`), which is metric-compatible. The system sans-serif and
//! egui's bundled fonts follow as fallbacks for scripts Selawik does not cover.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{FontData, FontDefinitions, FontFamily};

const SELAWIK: &[u8] = include_bytes!("../../assets/fonts/selawk.ttf");
const SELAWIK_LIGHT: &[u8] = include_bytes!("../../assets/fonts/selawkl.ttf");
const SELAWIK_SEMILIGHT: &[u8] = include_bytes!("../../assets/fonts/selawksl.ttf");
const SELAWIK_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/selawksb.ttf");

/// Named font families besides `Proportional` (= Regular) and `Monospace`.
pub const LIGHT: &str = "light";
pub const SEMILIGHT: &str = "semilight";
pub const SEMIBOLD: &str = "semibold";

pub fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// Font files resolved before the UI starts (fontconfig is queried off the UI thread).
#[derive(Debug, Clone, Default)]
pub struct Fonts {
    /// Segoe UI Regular, Light, Semilight, Semibold
    pub segoe: [Option<PathBuf>; 4],
    /// System sans-serif and monospace, used as fallbacks only
    pub system_sans: Option<PathBuf>,
    pub system_mono: Option<PathBuf>,
}

fn fc_match(pattern: &str, family: Option<&str>) -> Option<PathBuf> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{family}\n%{file}", pattern])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let (found_family, file) = text.split_once('\n')?;
    if let Some(family) = family
        && !found_family
            .split(',')
            .any(|f| f.trim().eq_ignore_ascii_case(family))
    {
        return None;
    }
    let path = PathBuf::from(file.trim());
    // egui cannot render font collections reliably
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    (path.exists() && (ext == "ttf" || ext == "otf")).then_some(path)
}

/// Maps a system font once per process as a `'static` slice. epaint copies owned font data
/// into its glyph blob while the definitions keep the original, so borrowed bytes avoid a
/// second copy of every system font. A read-only file mapping instead of a heap copy keeps
/// the bytes out of anonymous memory (~1.2 MiB for a typical sans fallback): the pages are
/// clean, reclaimable and shared with every other process that maps the same font.
fn read_static(path: &Path) -> Option<&'static [u8]> {
    static FILES: Mutex<Vec<(PathBuf, &'static [u8])>> = Mutex::new(Vec::new());
    let mut files = FILES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, bytes)) = files.iter().find(|(p, _)| p == path) {
        return Some(bytes);
    }
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: font files are replaced, not rewritten in place, by package managers (as
    // fontconfig and FreeType also assume when they map them); the mapping lives until exit.
    let bytes: &'static [u8] = match unsafe { memmap2::Mmap::map(&file) } {
        Ok(map) => &Box::leak(Box::new(map))[..],
        Err(_) => Box::leak(std::fs::read(path).ok()?.into_boxed_slice()),
    };
    files.push((path.to_owned(), bytes));
    Some(bytes)
}

impl Fonts {
    pub fn discover() -> Self {
        let segoe = |style: &str| fc_match(&format!("Segoe UI:style={style}"), Some("Segoe UI"));
        // Fixed-size parallel discovery, before any rendering. No font swap/reflow after the
        // first frame, and no persistent cache that can outlive fontconfig configuration.
        std::thread::scope(|scope| {
            let regular = scope.spawn(|| segoe("Regular"));
            let light = scope.spawn(|| segoe("Light"));
            let semilight = scope.spawn(|| segoe("Semilight"));
            let semibold = scope.spawn(|| segoe("Semibold"));
            let sans = scope.spawn(|| fc_match("sans-serif:style=Regular", None));
            let mono = scope.spawn(|| fc_match("monospace:style=Regular", None));
            Self {
                segoe: [regular, light, semilight, semibold]
                    .map(|job| job.join().unwrap_or_default()),
                system_sans: sans.join().unwrap_or_default(),
                system_mono: mono.join().unwrap_or_default(),
            }
        })
    }

    pub fn definitions(&self) -> FontDefinitions {
        let mut fonts = FontDefinitions::default();
        let egui_proportional = fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();

        fn insert(fonts: &mut FontDefinitions, name: &str, data: FontData) -> String {
            fonts.font_data.insert(name.to_owned(), Arc::new(data));
            name.to_owned()
        }

        let mut fallback = vec![];
        if let Some(bytes) = self.system_sans.as_deref().and_then(read_static) {
            fallback.push(insert(
                &mut fonts,
                "system-sans",
                FontData::from_static(bytes),
            ));
        }
        fallback.extend(egui_proportional);

        let bundled = [SELAWIK, SELAWIK_LIGHT, SELAWIK_SEMILIGHT, SELAWIK_SEMIBOLD];
        let names = ["regular", LIGHT, SEMILIGHT, SEMIBOLD];
        let mut chains = vec![];
        for ((name, bytes), segoe) in names.iter().zip(bundled).zip(&self.segoe) {
            let mut chain = vec![];
            if let Some(bytes) = segoe.as_deref().and_then(read_static) {
                chain.push(insert(
                    &mut fonts,
                    &format!("segoe-{name}"),
                    FontData::from_static(bytes),
                ));
            }
            chain.push(insert(
                &mut fonts,
                &format!("selawik-{name}"),
                FontData::from_static(bytes),
            ));
            chain.extend(fallback.iter().cloned());
            chains.push(chain);
        }

        let mut chains = chains.into_iter();
        fonts
            .families
            .insert(FontFamily::Proportional, chains.next().unwrap_or_default());
        for name in [LIGHT, SEMILIGHT, SEMIBOLD] {
            fonts
                .families
                .insert(family(name), chains.next().unwrap_or_default());
        }

        if let Some(bytes) = self.system_mono.as_deref().and_then(read_static) {
            let name = insert(&mut fonts, "system-mono", FontData::from_static(bytes));
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, name);
        }
        fonts
    }
}
