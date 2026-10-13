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

fn build_magpie(root: &Path) -> Result<PathBuf, String> {
    let target = root.join("target/magpie");
    run(Command::new("cargo")
        .args(["build", "--locked", "--release", "--manifest-path"])
        .arg(root.join("subprojects/magpie/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target))?;
    Ok(target.join("release/magpie"))
}

/// Local development uses the host ABI, with no redistributed libraries or packaging metadata.
pub fn magpie_dev_binary(root: &Path) -> Result<PathBuf, String> {
    let built = build_magpie(root)?;
    let directory = root.join("target/magpie/dev/bin");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let named = directory.join("missioncenter-magpie");
    #[cfg(unix)]
    {
        // The launcher needs the installed name, while current_exe resolves to the original
        // Cargo binary, whose --test intentionally does not require a private GPU bundle.
        match fs::read_link(&named) {
            Ok(target) if target == built => {}
            Ok(_) => return Err(format!("Unexpected daemon link: {}", named.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::os::unix::fs::symlink(&built, &named).map_err(|e| e.to_string())?;
            }
            Err(error) => return Err(format!("{}: {error}", named.display())),
        }
    }
    #[cfg(not(unix))]
    fs::copy(&built, &named).map_err(|e| e.to_string())?;
    run(Command::new(&named).arg("--test"))?;
    Ok(named)
}

/// Builds a completed relocatable daemon tree (bin + private GPU libraries).
pub fn magpie_binary(root: &Path) -> Result<PathBuf, String> {
    let built = build_magpie(root)?;
    let target = root.join("target/magpie");
    fs::create_dir_all(target.join("bin")).map_err(|e| e.to_string())?;
    let named = target.join("bin/missioncenter-magpie");
    fs::copy(&built, &named).map_err(|e| e.to_string())?;
    run(Command::new("python3")
        .arg(root.join("support/bundle-gpu-libs.py"))
        .arg("--binary")
        .arg(&named)
        .arg("--dest")
        .arg(target.join("lib/taskmanager/gpu")))?;
    run(Command::new(&named).arg("--test"))?;
    Ok(named)
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
        run(Command::new("cargo")
            .current_dir(&root)
            .args(["build", "--locked", "--release", "-p", "taskmanager-app"])
            .env("MC_LOCALEDIR", &locale_dir))?;

        mkdir(&base.join("bin"))?;
        copy(
            &root.join("target/release/taskmanager"),
            &base.join("bin/taskmanager"),
        )?;
        install_hardware_files(&root, &prefix, &base)?;
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

/// Install just the optional host component, without rebuilding/bundling the application.
pub fn install_hardware(args: &[String]) -> Result<(), String> {
    let mut prefix = PathBuf::from("/usr");
    let mut destdir = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--prefix" => prefix = PathBuf::from(it.next().ok_or("Missing --prefix value")?),
            "--destdir" => {
                destdir = Some(PathBuf::from(it.next().ok_or("Missing --destdir value")?))
            }
            _ => return Err(format!("Unknown install-hardware argument: {arg}")),
        }
    }
    if prefix != Path::new("/usr") && prefix != Path::new("/usr/local") {
        return Err("Hardware helper prefix must be /usr or /usr/local.".into());
    }
    let base = destdir.map_or_else(
        || prefix.clone(),
        |dir| dir.join(prefix.strip_prefix("/").unwrap()),
    );
    install_hardware_files(&crate::root(), &prefix, &base)
}

fn install_hardware_files(root: &Path, prefix: &Path, base: &Path) -> Result<(), String> {
    let helper = base.join("libexec/taskmanager-hardware");
    fs::create_dir_all(helper.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::copy(root.join("support/taskmanager-hardware"), &helper).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    for (source, destination) in [
        (
            "taskmanager-hardware.service",
            "lib/systemd/system/taskmanager-hardware.service",
        ),
        (
            "taskmanager-hardware.socket",
            "lib/systemd/system/taskmanager-hardware.socket",
        ),
        (
            "io.github.taskmanager.hardware.policy",
            "share/polkit-1/actions/io.github.taskmanager.hardware.policy",
        ),
    ] {
        let target = base.join(destination);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        let content =
            fs::read_to_string(root.join("data").join(source)).map_err(|e| e.to_string())?;
        fs::write(
            target,
            content.replace(
                "/usr/libexec/taskmanager-hardware",
                &prefix
                    .join("libexec/taskmanager-hardware")
                    .to_string_lossy(),
            ),
        )
        .map_err(|e| e.to_string())?;
    }
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
mod hardware_tests {
    use super::*;

    #[test]
    fn staged_helper_install_has_matching_exec_policy_and_units() {
        let directory = std::env::temp_dir().join(format!(
            "taskmanager-helper-install-test-{}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let base = directory.join("usr/local");
        install_hardware_files(&crate::root(), Path::new("/usr/local"), &base).unwrap();
        let helper = base.join("libexec/taskmanager-hardware");
        assert!(helper.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&helper).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        let service =
            fs::read_to_string(base.join("lib/systemd/system/taskmanager-hardware.service"))
                .unwrap();
        let policy = fs::read_to_string(
            base.join("share/polkit-1/actions/io.github.taskmanager.hardware.policy"),
        )
        .unwrap();
        assert!(service.contains("ExecStart=/usr/local/libexec/taskmanager-hardware serve"));
        assert!(policy.contains(">/usr/local/libexec/taskmanager-hardware</annotate>"));
        assert!(policy.contains(">enable</annotate>"));
        let socket =
            fs::read_to_string(base.join("lib/systemd/system/taskmanager-hardware.socket"))
                .unwrap();
        assert!(socket.contains("ListenStream=/run/taskmanager-hardware.sock"));
        assert!(!directory.join("var/lib/taskmanager-hardware").exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
