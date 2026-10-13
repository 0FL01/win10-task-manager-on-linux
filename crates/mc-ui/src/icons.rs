/* crates/mc-ui/src/icons.rs
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

//! Icon textures. Decoding (SVG rasterisation, PNG decoding, icon theme lookup) happens on a
//! worker thread; the UI thread only uploads finished images in `poll`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

use egui::{ColorImage, TextureHandle, TextureOptions};
use mc_magpie::Icon;

macro_rules! bundled {
    ($($name: literal),* $(,)?) => {
        const BUNDLED: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*
        ];
    };
}

bundled!(
    "application-x-executable",
    "arrow-circular-top-right-symbolic",
    "check-plain-symbolic",
    "cross-small-circle-filled-symbolic",
    "details-symbolic",
    "edit-symbolic",
    "external-link-symbolic",
    "general-properties-symbolic",
    "info-outline-symbolic",
    "info-symbolic",
    "line-dashed-disk",
    "line-dashed-gpu",
    "line-dashed-mem",
    "line-dashed-net",
    "line-solid-disk",
    "line-solid-gpu",
    "line-solid-mem",
    "line-solid-net",
    "list-collapse-symbolic",
    "list-disable-all-symbolic",
    "list-drag-handle-symbolic",
    "list-enable-all-symbolic",
    "mount-point-symbolic",
    "nm-signal-00-symbolic",
    "nm-signal-100-symbolic",
    "nm-signal-25-symbolic",
    "nm-signal-50-symbolic",
    "nm-signal-75-symbolic",
    "overlapping-windows-symbolic",
    "padlock2-symbolic",
    "ruler-angled-symbolic",
    "service-disabled",
    "service-failed",
    "service-running",
    "service-stopped",
    "sidebar-toggle-left-symbolic",
    "speedometer-symbolic",
);

/// Task Manager application icon (256×256 PNG, the largest installed hicolor size)
pub const APP_ICON_PNG: &[u8] = include_bytes!(
    "../../../data/icons/hicolor/256x256/apps/io.github.taskmanager.TaskManager.png"
);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IconKey {
    Bundled(&'static str, u32),
    App(String, u32),
    Themed(String, u32),
}

enum Job {
    Svg {
        key: IconKey,
        bytes: Arc<[u8]>,
        size: u32,
        symbolic: bool,
    },
    Raster {
        key: IconKey,
        bytes: Vec<u8>,
        size: u32,
    },
    Themed {
        key: IconKey,
        name: String,
        size: u32,
    },
}

type Waker = Arc<dyn Fn() + Send + Sync>;

pub struct IconCache {
    textures: HashMap<IconKey, TextureHandle>,
    failed: HashSet<IconKey>,
    pending: HashSet<IconKey>,
    app_icons: HashMap<String, Icon>,
    jobs: SyncSender<Job>,
    results: Receiver<(IconKey, Option<ColorImage>)>,
}

fn rasterize_svg(bytes: &[u8], size: u32, symbolic: bool) -> Option<ColorImage> {
    let options = resvg::usvg::Options::default();
    let mut image = egui_extras::image::load_svg_bytes_with_size(
        bytes,
        egui::SizeHint::Size {
            width: size,
            height: size,
            maintain_aspect_ratio: true,
        },
        &options,
    )
    .ok()?;

    if symbolic {
        // Turn the (dark) symbolic icon into a white mask so it can be tinted
        for px in image.pixels.iter_mut() {
            let a = px.a();
            *px = egui::Color32::from_rgba_premultiplied(a, a, a, a);
        }
    }
    Some(image)
}

fn decode_raster(bytes: &[u8], size: u32) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > size * 2 || img.height() > size * 2 {
        img.thumbnail(size, size)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

fn lookup_theme_icon(name: &str, size: u32) -> Option<PathBuf> {
    if name.starts_with('/') {
        let path = PathBuf::from(name);
        return path.exists().then_some(path);
    }

    let themes = ["hicolor", "Adwaita", "breeze"];
    for theme in std::iter::once(current_icon_theme())
        .flatten()
        .chain(themes.iter().map(|s| s.to_string()))
    {
        if let Some(p) = freedesktop_icons::lookup(name)
            .with_size(size as u16)
            .with_theme(&theme)
            .with_cache()
            .find()
        {
            return Some(p);
        }
    }

    // Plain pixmaps
    for ext in ["png", "svg", "xpm"] {
        let p = PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn current_icon_theme() -> Option<String> {
    use std::sync::OnceLock;
    static THEME: OnceLock<Option<String>> = OnceLock::new();
    THEME
        .get_or_init(|| {
            // KDE stores the theme in kdeglobals; GNOME in gsettings (not read here)
            let home = std::env::var_os("HOME")?;
            let text =
                std::fs::read_to_string(PathBuf::from(home).join(".config/kdeglobals")).ok()?;
            let mut in_icons = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_icons = line == "[Icons]";
                } else if in_icons && let Some(v) = line.strip_prefix("Theme=") {
                    return Some(v.trim().to_owned());
                }
            }
            None
        })
        .clone()
}

fn run_job(job: Job) -> (IconKey, Option<ColorImage>) {
    let _profile = mc_magpie::profile::Span::new(mc_magpie::profile::Phase::IconDecode);
    match job {
        Job::Svg {
            key,
            bytes,
            size,
            symbolic,
        } => (key, rasterize_svg(&bytes, size, symbolic)),
        Job::Raster { key, bytes, size } => (key, decode_raster(&bytes, size)),
        Job::Themed { key, name, size } => {
            let image = lookup_theme_icon(&name, size).and_then(|path| {
                let bytes = std::fs::read(&path).ok()?;
                if path.extension().is_some_and(|e| e == "svg" || e == "svgz") {
                    rasterize_svg(&bytes, size, name.ends_with("-symbolic"))
                } else {
                    decode_raster(&bytes, size)
                }
            });
            (key, image)
        }
    }
}

impl IconCache {
    pub fn new(waker: Waker) -> Self {
        let (job_tx, job_rx) = mpsc::sync_channel::<Job>(512);
        let (res_tx, res_rx) = mpsc::sync_channel(512);

        let _ = std::thread::Builder::new()
            .name("mc-icons".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    if res_tx.send(run_job(job)).is_err() {
                        break;
                    }
                    waker();
                }
            });

        Self {
            textures: HashMap::new(),
            failed: HashSet::new(),
            pending: HashSet::new(),
            app_icons: HashMap::new(),
            jobs: job_tx,
            results: res_rx,
        }
    }

    /// Uploads decoded images; call from `logic`. Returns `true` if anything changed.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let mut profile = mc_magpie::profile::Span::new(mc_magpie::profile::Phase::IconUpload);
        let mut count = 0;
        let mut changed = false;
        while let Ok((key, image)) = self.results.try_recv() {
            count += 1;
            self.pending.remove(&key);
            match image {
                Some(image) => {
                    let name = format!("icon:{key:?}");
                    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
                    self.textures.insert(key, texture);
                }
                None => {
                    self.failed.insert(key);
                }
            }
            changed = true;
        }
        profile.items(count);
        changed
    }

    pub fn merge_app_icons(&mut self, icons: HashMap<String, Icon>) {
        for (id, icon) in icons {
            // a new icon invalidates any previous failure for that app
            self.failed
                .retain(|k| !matches!(k, IconKey::App(a, _) if *a == id));
            self.app_icons.insert(id, icon);
        }
    }

    fn submit(&mut self, key: IconKey, job: Job) {
        match self.jobs.try_send(job) {
            Ok(()) => {
                self.pending.insert(key);
            }
            Err(TrySendError::Full(_)) => {} // retried on a later frame
            Err(TrySendError::Disconnected(_)) => {
                self.failed.insert(key);
            }
        }
    }

    fn get_or_request(
        &mut self,
        key: IconKey,
        make: impl FnOnce() -> Option<Job>,
    ) -> Option<&TextureHandle> {
        if self.textures.contains_key(&key) {
            return self.textures.get(&key);
        }
        if !self.pending.contains(&key) && !self.failed.contains(&key) {
            match make() {
                Some(job) => self.submit(key.clone(), job),
                None => {
                    self.failed.insert(key);
                }
            }
        }
        None
    }

    fn pixel_size(ctx: &egui::Context, points: f32) -> u32 {
        (points * ctx.pixels_per_point()).round().max(1.) as u32
    }

    /// One of the bundled SVGs.
    pub fn bundled(
        &mut self,
        ctx: &egui::Context,
        name: &'static str,
        points: f32,
    ) -> Option<&TextureHandle> {
        let size = Self::pixel_size(ctx, points);
        let key = IconKey::Bundled(name, size);
        self.get_or_request(key.clone(), || {
            let bytes = BUNDLED.iter().find(|(n, _)| *n == name)?.1;
            Some(Job::Svg {
                key,
                bytes: Arc::from(bytes),
                size,
                symbolic: name.ends_with("-symbolic"),
            })
        })
    }

    /// The application icon.
    pub fn app_logo(&mut self, ctx: &egui::Context, points: f32) -> Option<&TextureHandle> {
        let size = Self::pixel_size(ctx, points);
        let key = IconKey::Themed("io.github.taskmanager.TaskManager".into(), size);
        self.get_or_request(key.clone(), || {
            Some(Job::Raster {
                key,
                bytes: APP_ICON_PNG.to_vec(),
                size,
            })
        })
    }

    /// Icon of a running app as reported by Magpie (theme name or raw image data).
    pub fn app(
        &mut self,
        ctx: &egui::Context,
        app_id: &str,
        points: f32,
    ) -> Option<&TextureHandle> {
        let size = Self::pixel_size(ctx, points);
        let key = IconKey::App(app_id.to_owned(), size);
        if self.textures.contains_key(&key) {
            return self.textures.get(&key);
        }
        let icon = self.app_icons.get(app_id).cloned();
        let icon = icon?;
        self.get_or_request(key.clone(), || match icon {
            Icon::Id(name) => Some(Job::Themed { key, name, size }),
            Icon::Data(bytes) => {
                if bytes.starts_with(b"<?xml") || bytes.starts_with(b"<svg") {
                    Some(Job::Svg {
                        key,
                        bytes: Arc::from(bytes.as_slice()),
                        size,
                        symbolic: false,
                    })
                } else {
                    Some(Job::Raster { key, bytes, size })
                }
            }
            Icon::Empty(_) => None,
        })
    }
}

/// The application icon as RGBA pixels, for the window icon.
pub fn app_icon_rgba(size: u32) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?;
    let img = if img.width() != size {
        img.resize(size, size, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some((rgba.as_raw().clone(), rgba.width(), rgba.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_svg_rasterizes() {
        for (name, bytes) in BUNDLED {
            let img = rasterize_svg(bytes, 24, name.ends_with("-symbolic"));
            assert!(img.is_some(), "{name} failed to rasterize");
        }
    }

    #[test]
    fn symbolic_icons_become_white_masks() {
        let (_, bytes) = BUNDLED.iter().find(|(n, _)| *n == "edit-symbolic").unwrap();
        let img = rasterize_svg(bytes, 16, true).unwrap();
        assert!(img.pixels.iter().any(|p| p.a() > 0));
        assert!(img.pixels.iter().all(|p| p.r() == p.a()));
    }

    #[test]
    fn app_icon_decodes_and_scales() {
        let (rgba, w, h) = app_icon_rgba(64).unwrap();
        assert_eq!((w, h), (64, 64));
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert!(decode_raster(APP_ICON_PNG, 256).is_some());
    }

    #[test]
    fn installed_app_icons_have_declared_sizes() {
        let icons =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/icons/hicolor");
        for size in [16, 22, 24, 32, 48, 64, 128, 256] {
            let path = icons.join(format!(
                "{size}x{size}/apps/io.github.taskmanager.TaskManager.png"
            ));
            let bytes = std::fs::read(&path).unwrap();
            let image =
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
            assert_eq!(
                (image.width(), image.height()),
                (size, size),
                "{}",
                path.display()
            );
        }
    }
}
