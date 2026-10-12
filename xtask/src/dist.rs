/* xtask/src/dist.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Building the daemon, translations and installing the Cargo application.

use std::fs;
use std::os::unix::fs::PermissionsExt;
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

/// Builds a completed relocatable daemon tree (bin + private GPU libraries).
pub fn magpie_binary(root: &Path) -> Result<PathBuf, String> {
    let target = root.join("target/magpie");
    stage_magpie(root, &target, &target, false, None)
}

fn release_build(root: &Path, target: &Path, ui: bool, diagnostic: bool) -> Command {
    let mut cmd = Command::new("cargo");
    if diagnostic {
        cmd.arg("+1.96.1")
            .env("CARGO_PROFILE_RELEASE_DEBUG", "2")
            .env("CARGO_PROFILE_RELEASE_STRIP", "none");
    }
    cmd.current_dir(root)
        .args(["build", "--locked", "--release"]);
    if ui {
        cmd.args(["-p", "taskmanager-app"]);
    } else {
        cmd.arg("--manifest-path")
            .arg(root.join("subprojects/magpie/Cargo.toml"));
    }
    if diagnostic {
        cmd.args([
            "--no-default-features",
            "--features",
            if ui {
                "diagnostic-profile,glow,wgpu"
            } else {
                "diagnostic-profile"
            },
        ]);
    }
    cmd.arg("--target-dir").arg(target);
    cmd
}

fn stage_magpie(
    root: &Path,
    target: &Path,
    prefix: &Path,
    diagnostic: bool,
    patchelf: Option<&Path>,
) -> Result<PathBuf, String> {
    run(&mut release_build(root, target, false, diagnostic))?;
    let built = target.join("release/magpie");
    fs::create_dir_all(prefix.join("bin")).map_err(|e| e.to_string())?;
    let named = prefix.join("bin/missioncenter-magpie");
    fs::copy(&built, &named).map_err(|e| e.to_string())?;
    let mut bundle = Command::new("python3");
    bundle
        .arg(root.join("support/bundle-gpu-libs.py"))
        .arg("--binary")
        .arg(&named)
        .arg("--dest")
        .arg(prefix.join("lib/taskmanager/gpu"));
    if let Some(path) = patchelf {
        bundle.env("PATCHELF", path);
    }
    run(&mut bundle)?;
    run(Command::new(&named).arg("--test"))?;
    Ok(named)
}

fn diagnostic_patchelf(root: &Path) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PATCHELF") {
        return Ok(path.into());
    }
    let executable = |path: &Path| {
        fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let candidates = std::env::split_paths(&path)
        .map(|dir| dir.join("patchelf"))
        .chain([
            root.join("target/gpu-bundle-tools/bin/patchelf"),
            root.join("target/gpu-bundle-tools/gpu-patchelf"),
        ]);
    candidates.into_iter().find(|p| executable(p)).ok_or_else(|| {
        "patchelf >= 0.18 is required; set PATCHELF or provide target/gpu-bundle-tools/bin/patchelf"
            .into()
    })
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// A local, symbol-bearing release bundle; normal build/install targets are untouched.
pub fn diagnostic(args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("diagnostic takes no arguments".into());
    }
    let root = crate::root();
    let diagnostic = root.join("target/diagnostic");
    let bundle = diagnostic.join("bundle");
    let stage = diagnostic.join("bundle-stage");
    let patchelf = diagnostic_patchelf(&root)?;
    run(Command::new(&patchelf).arg("--version"))?;
    run(&mut release_build(
        &root,
        &diagnostic.join("ui"),
        true,
        true,
    ))?;
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    stage_magpie(
        &root,
        &diagnostic.join("magpie"),
        &stage,
        true,
        Some(&patchelf),
    )?;
    fs::copy(
        diagnostic.join("ui/release/taskmanager"),
        stage.join("bin/taskmanager"),
    )
    .map_err(|e| e.to_string())?;
    hwdb(&root, &stage.join("share/missioncenter"))?;
    compile_mo(&root, &stage.join("share/locale"))?;
    // Publish only after the helper has verified libraries, notices and relative RUNPATH.
    if bundle.exists() {
        fs::remove_dir_all(&bundle).map_err(|e| e.to_string())?;
    }
    fs::rename(&stage, &bundle).map_err(|e| e.to_string())?;
    let binary = bundle.join("bin/taskmanager");
    println!("diagnostic binary: {}", binary.display());
    println!(
        "run locally:\nenv MC_MAGPIE_HW_DB={} MC_LOCALEDIR={} {}",
        shell_quote(&bundle.join("share/missioncenter/hw.db")),
        shell_quote(&bundle.join("share/locale")),
        shell_quote(&binary),
    );
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let destination = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
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
    for kw in ["n_", "i18n", "i18n_f", "ni18n_f:1,2", "pi18n:1c,2"] {
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
        run(release_build(&root, &root.join("target"), true, false)
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
        let gpu_dir = base.join("lib/taskmanager/gpu");
        if gpu_dir.exists() {
            fs::remove_dir_all(&gpu_dir).map_err(|e| e.to_string())?;
        }
        copy_tree(&root.join("target/magpie/lib/taskmanager/gpu"), &gpu_dir)?;
        // Only the daemon uses monitoring libraries; do not alter the UI's driver lookup.
        // Verify the staged tree too, so an incomplete install cannot be packaged.
        run(Command::new("python3")
            .arg(root.join("support/bundle-gpu-libs.py"))
            .arg("--verify")
            .arg("--dest")
            .arg(&gpu_dir)
            .arg("--binary")
            .arg(base.join("bin/missioncenter-magpie")))?;
        run(Command::new(base.join("bin/missioncenter-magpie")).arg("--test"))?;

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
    // Release number of the package (`MC_RPM_RELEASE`, default 9 in the spec)
    let release = std::env::var("MC_RPM_RELEASE").unwrap_or_else(|_| "14".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_builds_are_locked_symbol_bearing_and_isolated() {
        let root = crate::root();
        for (ui, directory) in [(true, "ui"), (false, "magpie")] {
            let target = root.join("target/diagnostic").join(directory);
            let cmd = release_build(&root, &target, ui, true);
            let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
            assert_eq!(&args[..4], &["+1.96.1", "build", "--locked", "--release"]);
            assert_eq!(args[4], if ui { "-p" } else { "--manifest-path" });
            assert_eq!(args[6], "--no-default-features");
            assert_eq!(args[7], "--features");
            assert_eq!(
                args[8],
                if ui {
                    "diagnostic-profile,glow,wgpu"
                } else {
                    "diagnostic-profile"
                }
            );
            assert_eq!(args[9], "--target-dir");
            assert_eq!(args[10], target.to_string_lossy());
            let env: Vec<_> = cmd.get_envs().collect();
            assert_eq!(
                env,
                vec![
                    (
                        std::ffi::OsStr::new("CARGO_PROFILE_RELEASE_DEBUG"),
                        Some(std::ffi::OsStr::new("2")),
                    ),
                    (
                        std::ffi::OsStr::new("CARGO_PROFILE_RELEASE_STRIP"),
                        Some(std::ffi::OsStr::new("none")),
                    ),
                ]
            );
        }
        for ui in [false, true] {
            let normal = release_build(&root, &root.join("target/production"), ui, false);
            let args: Vec<_> = normal.get_args().map(|arg| arg.to_string_lossy()).collect();
            assert_eq!(args[0], "build");
            assert!(
                !args
                    .iter()
                    .any(|arg| arg.contains("diagnostic") || arg == "--no-default-features")
            );
            assert_eq!(normal.get_envs().count(), 0);
        }
    }

    #[test]
    fn diagnostic_rejects_arguments_before_building() {
        assert!(
            diagnostic(&["--rpm".into()])
                .unwrap_err()
                .contains("no arguments")
        );
    }

    #[test]
    fn printed_command_quotes_spaces_and_apostrophes() {
        assert_eq!(shell_quote(Path::new("/a b/c'd")), "'/a b/c'\\''d'");
    }
}
