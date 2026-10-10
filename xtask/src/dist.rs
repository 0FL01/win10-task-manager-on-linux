/* xtask/src/dist.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Building the daemon, translations and installing the Cargo application.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_ID: &str = "io.github.taskmanager.TaskManager";
const DOMAIN: &str = "taskmanager";
/// Installed hicolor PNG sizes of the application icon
const ICON_SIZES: [u32; 8] = [16, 22, 24, 32, 48, 64, 128, 256];

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed with {status}"))
    }
}

/// Builds the magpie daemon with cargo and returns
/// the path of an executable named `missioncenter-magpie`.
pub fn magpie_binary(root: &Path) -> Result<PathBuf, String> {
    let target = root.join("target/magpie");
    run(Command::new("cargo")
        .args(["build", "--release", "--manifest-path"])
        .arg(root.join("subprojects/magpie/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target))?;
    let built = target.join("release/magpie");
    let named = target.join("release/missioncenter-magpie");
    fs::copy(&built, &named).map_err(|e| e.to_string())?;
    Ok(named)
}

pub fn hwdb(root: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let dir = root.join("subprojects/magpie/platform-linux/hwdb");
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    run(Command::new("python3")
        .arg(dir.join("generate_hwdb.py"))
        .arg("--output")
        .arg(out_dir)
        .arg(dir.join("20-pci-vendor-model.hwdb"))
        .arg(dir.join("20-usb-vendor-model.hwdb")))?;
    Ok(out_dir.join("hw.db"))
}

fn languages(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join("po/LINGUAS"))
        .unwrap_or_default()
        .split_whitespace()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

pub fn compile_mo(root: &Path, out: &Path) -> Result<(), String> {
    for lang in languages(root) {
        let dir = out.join(&lang).join("LC_MESSAGES");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        run(Command::new("msgfmt")
            .arg("-o")
            .arg(dir.join(format!("{DOMAIN}.mo")))
            .arg(root.join(format!("po/{lang}.po"))))?;
    }
    Ok(())
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    for d in [
        "crates/mc-core/src",
        "crates/mc-ui/src",
        "apps/taskmanager/src",
    ] {
        walk(&root.join(d), &mut files);
    }
    files
}

/// Extracts msgids from the Rust sources (same keywords as the GTK build) plus the
/// desktop/metainfo files into `po/taskmanager.pot`.
pub fn update_pot(root: &Path) -> Result<(), String> {
    let mut cmd = Command::new("xgettext");
    cmd.current_dir(root)
        .args([
            "--from-code=UTF-8",
            "--add-comments",
            "--package-name=taskmanager",
        ])
        .args(["-o", "po/taskmanager.pot", "--language=Rust"]);
    for kw in [
        "n_",
        "i18n",
        "i18n_f",
        "i18n_k",
        "ni18n:1,2",
        "ni18n_f:1,2",
        "ni18n_k:1,2",
        "pi18n:1c,2",
        "pi18n_f:1c,2",
        "pi18n_k:1c,2",
        "pni18n:1c,2,3",
        "pni18n_f:1c,2,3",
        "pni18n_k:1c,2,3",
    ] {
        cmd.arg(format!("--keyword={kw}"));
    }
    for f in rust_sources(root) {
        cmd.arg(f.strip_prefix(root).unwrap_or(&f));
    }
    run(&mut cmd)?;
    run(Command::new("xgettext")
        .current_dir(root)
        .args([
            "--join-existing",
            "--package-name=taskmanager",
            "-o",
            "po/taskmanager.pot",
        ])
        .arg(format!("data/{APP_ID}.desktop.in"))
        .arg(format!("data/{APP_ID}.metainfo.xml.in")))?;
    Ok(())
}

pub fn install(args: &[String]) -> Result<(), String> {
    let root = crate::root();
    let mut prefix = PathBuf::from("/usr/local");
    let mut destdir: Option<PathBuf> = None;
    let mut data_only = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--prefix" => prefix = it.next().map(PathBuf::from).unwrap_or(prefix),
            "--destdir" => destdir = it.next().map(PathBuf::from),
            "--data-only" => data_only = true,
            _ => {}
        }
    }
    let base = match &destdir {
        Some(d) => d.join(prefix.strip_prefix("/").unwrap_or(&prefix)),
        None => prefix.clone(),
    };
    let share = base.join("share");
    let mkdir = |p: &Path| fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()));
    let copy = |from: &Path, to: &Path| {
        fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
    };

    // `--data-only`: translations, desktop entry, metainfo and icons (packagers that build
    // the binaries themselves, e.g. Flatpak)
    if !data_only {
        let locale_dir = prefix.join("share/locale");
        run(Command::new("cargo")
            .current_dir(&root)
            .args(["build", "--release", "-p", "taskmanager-app"])
            .env("MC_LOCALEDIR", &locale_dir))?;

        mkdir(&base.join("bin"))?;
        copy(
            &root.join("target/release/taskmanager"),
            &base.join("bin/taskmanager"),
        )?;
        // The Mission Center statistics backend keeps its own names
        copy(
            &magpie_binary(&root)?,
            &base.join("bin/missioncenter-magpie"),
        )?;

        mkdir(&share.join("missioncenter"))?;
        copy(
            &hwdb(&root, &root.join("target/hwdb"))?,
            &share.join("missioncenter/hw.db"),
        )?;
    }

    compile_mo(&root, &share.join("locale"))?;

    mkdir(&share.join("applications"))?;
    run(Command::new("msgfmt")
        .args(["--desktop", "-d"])
        .arg(root.join("po"))
        .arg("--template")
        .arg(root.join(format!("data/{APP_ID}.desktop.in")))
        .arg("-o")
        .arg(share.join(format!("applications/{APP_ID}.desktop"))))?;
    mkdir(&share.join("metainfo"))?;
    run(Command::new("msgfmt")
        .args(["--xml", "-d"])
        .arg(root.join("po"))
        .arg("--template")
        .arg(root.join(format!("data/{APP_ID}.metainfo.xml.in")))
        .arg("-o")
        .arg(share.join(format!("metainfo/{APP_ID}.metainfo.xml"))))?;

    for size in ICON_SIZES {
        let sub = format!("{size}x{size}/apps");
        let name = format!("{APP_ID}.png");
        let dir = share.join("icons/hicolor").join(&sub);
        mkdir(&dir)?;
        copy(
            &root.join("data/icons/hicolor").join(&sub).join(&name),
            &dir.join(&name),
        )?;
    }

    println!("installed into {}", base.display());
    Ok(())
}

/// Builds a Fedora RPM from a fresh staged install (`packaging/rpm/taskmanager.spec`).
pub fn rpm(_args: &[String]) -> Result<(), String> {
    let root = crate::root();
    let stage = root.join("target/rpm-stage");
    let topdir = root.join("target/rpmbuild");
    let _ = fs::remove_dir_all(&stage);
    install(&[
        "--prefix".into(),
        "/usr".into(),
        "--destdir".into(),
        stage.display().to_string(),
    ])?;
    for d in ["BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS", "BUILDROOT"] {
        fs::create_dir_all(topdir.join(d)).map_err(|e| e.to_string())?;
    }
    let version = env!("CARGO_PKG_VERSION");
    // Release number of the package (`MC_RPM_RELEASE`, default 8 in the spec)
    let release = std::env::var("MC_RPM_RELEASE").unwrap_or_else(|_| "8".into());
    run(Command::new("rpmbuild")
        .arg("-bb")
        .arg(root.join("packaging/rpm/taskmanager.spec"))
        .arg("--define")
        .arg(format!("_topdir {}", topdir.display()))
        .arg("--define")
        .arg(format!("stagedir {}", stage.display()))
        .arg("--define")
        .arg(format!("srcroot {}", root.display()))
        .arg("--define")
        .arg(format!("mc_version {version}"))
        .arg("--define")
        .arg(format!("mc_release {release}")))?;
    println!("RPMs in {}", topdir.join("RPMS").display());
    Ok(())
}
