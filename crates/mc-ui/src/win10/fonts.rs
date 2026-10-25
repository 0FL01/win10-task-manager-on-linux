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

//! Fonts of the Win10-style interface: the bundled Open Sans variable font (SIL OFL 1.1,
//! `assets/fonts/OFL.txt`), at its original width. The system sans-serif and egui's bundled
//! fonts follow as fallbacks for glyphs Open Sans does not cover.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::{FontData, FontDefinitions, FontFamily};

const OPEN_SANS: &[u8] = include_bytes!("../../assets/fonts/OpenSans-Variable.ttf");

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
    /// System sans-serif and monospace, used as fallbacks only
    pub system_sans: Option<PathBuf>,
    pub system_mono: Option<PathBuf>,
}

fn fc_match(pattern: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(text.trim());
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
        // Fixed-size parallel discovery, before any rendering. No font swap/reflow after the
        // first frame, and no persistent cache that can outlive fontconfig configuration.
        std::thread::scope(|scope| {
            let sans = scope.spawn(|| fc_match("sans-serif:style=Regular"));
            let mono = scope.spawn(|| fc_match("monospace:style=Regular"));
            Self {
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

        // Weight axis values of Regular, Light, Semilight and Semibold; the width axis stays at
        // its default 100.
        let weights = [400., 300., 350., 600.];
        let names = ["regular", LIGHT, SEMILIGHT, SEMIBOLD];
        let mut chains = vec![];
        for (name, weight) in names.iter().zip(weights) {
            let mut chain = vec![insert(
                &mut fonts,
                &format!("open-sans-{name}"),
                FontData::from_static(OPEN_SANS).tweak(FontTweak {
                    coords: VariationCoords::new([("wght", weight)]),
                    ..Default::default()
                }),
            )];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_families_use_open_sans_weights_and_preserve_fallbacks() {
        let defaults = FontDefinitions::default();
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/OpenSans-Variable.ttf");
        for fonts in [
            Fonts::default(),
            Fonts {
                system_sans: Some(path.clone()),
                system_mono: Some(path.clone()),
            },
        ] {
            let definitions = fonts.definitions();
            let mut fallback = defaults.families[&FontFamily::Proportional].clone();
            if fonts.system_sans.is_some() {
                fallback.insert(0, "system-sans".into());
            }
            for (name, family, weight) in [
                ("regular", FontFamily::Proportional, 400.),
                (LIGHT, family(LIGHT), 300.),
                (SEMILIGHT, family(SEMILIGHT), 350.),
                (SEMIBOLD, family(SEMIBOLD), 600.),
            ] {
                let chain = &definitions.families[&family];
                assert_eq!(chain[0], format!("open-sans-{name}"));
                assert_eq!(&chain[1..], fallback.as_slice());
                let data = &definitions.font_data[&chain[0]];
                assert_eq!(data.font.as_ref(), OPEN_SANS);
                assert_eq!(data.tweak.coords, VariationCoords::new([("wght", weight)]));
            }
            let mut mono = defaults.families[&FontFamily::Monospace].clone();
            if fonts.system_mono.is_some() {
                mono.insert(0, "system-mono".into());
            }
            assert_eq!(definitions.families[&FontFamily::Monospace], mono);
            assert_eq!(
                definitions.font_data.len(),
                defaults.font_data.len()
                    + 4
                    + usize::from(fonts.system_sans.is_some())
                    + usize::from(fonts.system_mono.is_some())
            );
        }
    }
}
