/* xtask/src/main.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Developer and packaging tasks: `cargo xtask <command>`.

use std::path::{Path, PathBuf};
use std::process::Command;

mod bench;
mod dist;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}

fn help() {
    println!(
        "cargo xtask <command>\n\n\
         run-dev [args]      build and run the app with the cargo-built magpie daemon\n\
         bench-idle [opts]   egui idle benchmark (wgpu, glow) in nested kwin\n\
         \x20                   --seconds N --warmup N --repeats N --out FILE\n\
         \x20                   --egui FILE [--egui-glow FILE]\n\
         magpie              build the magpie daemon (cargo, release)\n\
         diagnostic          build a runnable local release bundle with debug symbols\n\
         mo [DIR]            compile po/*.po into DIR/<lang>/LC_MESSAGES/taskmanager.mo\n\
         pot                 regenerate po/taskmanager.pot from Rust sources\n\
         install [--prefix P] [--destdir D] [--data-only]   install binaries, data, icons, translations\n\
         rpm                build a Fedora RPM from a fresh staged install"
    );
}

fn run_dev(args: &[String]) -> Result<(), String> {
    let root = root();
    let status = Command::new("cargo")
        .args(["build", "-p", "taskmanager-app"])
        .current_dir(&root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("build failed".into());
    }
    let magpie = dist::magpie_binary(&root)?;
    let hwdb = match std::env::var_os("MC_MAGPIE_HW_DB") {
        Some(path) => PathBuf::from(path),
        None => dist::hwdb(&root, &root.join("target/hwdb"))?,
    };
    let locale = root.join("target/locale");
    dist::compile_mo(&root, &locale)?;
    let path = format!(
        "{}:{}",
        magpie.parent().unwrap().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(root.join("target/debug/taskmanager"))
        .args(args)
        .env("PATH", path)
        .env("MC_LOCALEDIR", locale)
        .env("MC_MAGPIE_HW_DB", hwdb)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}"))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        help();
        return;
    };
    let rest = &args[1..];
    let result = match cmd.as_str() {
        "run-dev" => run_dev(rest),
        "bench-idle" => bench::bench_idle(rest),
        "magpie" => dist::magpie_binary(&root()).map(|p| println!("{}", p.display())),
        "diagnostic" => dist::diagnostic(rest),
        "mo" => {
            let dir = rest
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| root().join("target/locale"));
            dist::compile_mo(&root(), &dir)
        }
        "pot" => dist::update_pot(&root()),
        "install" => dist::install(rest),
        "rpm" => dist::rpm(rest),
        _ => {
            help();
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
