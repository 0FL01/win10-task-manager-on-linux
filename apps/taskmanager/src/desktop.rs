/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Desktop launch integration. Files and D-Bus operations run outside the rendering thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use zbus::blocking::{Connection, connection::Builder};

const COMPONENT: &str = "io.github.taskmanager.TaskManager.shortcuts.desktop";
const ACCEL: &str = "org.kde.kglobalaccel";
const KEYS: [i32; 2] = [0x07000000, 0x0d000007]; // Qt Ctrl+Shift+Escape, Ctrl+Alt+Delete

struct Activation {
    requested: Arc<AtomicBool>,
    wake: SyncSender<()>,
}

#[zbus::interface(name = "io.github.taskmanager.TaskManager")]
impl Activation {
    fn activate(&self) {
        self.requested.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
}

pub struct Instance {
    _connection: Connection,
    requested: Arc<AtomicBool>,
    wake: Option<Receiver<()>>,
}

impl Instance {
    /// A name race is resolved by activating the winner. Failure of the session bus permits
    /// a normal launch rather than making the task manager inaccessible.
    pub fn claim() -> Result<Option<Self>, zbus::Error> {
        let requested = Arc::new(AtomicBool::new(false));
        let (wake, receiver) = mpsc::sync_channel(1);
        let connection = Builder::session()?
            .method_timeout(Duration::from_secs(2))
            .serve_at(
                "/TaskManager",
                Activation {
                    requested: requested.clone(),
                    wake,
                },
            )?
            .build()
            .and_then(|connection| {
                connection.request_name_with_flags(
                    super::APP_ID,
                    zbus::fdo::RequestNameFlags::DoNotQueue.into(),
                )?;
                Ok(connection)
            });
        match connection {
            Ok(connection) => Ok(Some(Self {
                _connection: connection,
                requested,
                wake: Some(receiver),
            })),
            Err(zbus::Error::NameTaken) => {
                let connection = Builder::session()?
                    .method_timeout(Duration::from_secs(2))
                    .build()?;
                connection.call_method(
                    Some(super::APP_ID),
                    "/TaskManager",
                    Some(super::APP_ID),
                    "Activate",
                    &(),
                )?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn attach(&mut self, ctx: egui::Context) {
        if let Some(receiver) = self.wake.take() {
            std::thread::spawn(move || {
                while receiver.recv().is_ok() {
                    ctx.request_repaint_after(Duration::from_millis(1));
                }
            });
        }
    }

    pub fn take_activation(&self) -> bool {
        self.requested.swap(false, Ordering::AcqRel)
    }
}

pub struct Shortcuts {
    requests: SyncSender<[bool; 2]>,
    results: Receiver<Result<[bool; 2], String>>,
}

impl Shortcuts {
    pub fn new(ctx: egui::Context, initial: [bool; 2]) -> Self {
        let (requests, receiver) = mpsc::sync_channel(1);
        let (sender, results) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut next = Some(initial);
            let mut restore = true;
            while let Some(settings) = next.take().or_else(|| receiver.recv().ok()) {
                if sender.send(install_shortcuts(settings, restore)).is_err() {
                    break;
                }
                restore = false;
                ctx.request_repaint_after(Duration::from_millis(1));
            }
        });
        Self { requests, results }
    }

    pub fn submit(&self, settings: [bool; 2]) -> bool {
        self.requests.try_send(settings).is_ok()
    }

    pub fn try_result(&self) -> Option<Result<[bool; 2], String>> {
        self.results.try_recv().ok()
    }
}

fn data_home() -> Result<PathBuf, String> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "The user data directory is unavailable.".into())
}

fn desktop_exec(executable: &Path) -> Result<String, String> {
    let value = executable
        .to_str()
        .ok_or("The executable path is not UTF-8.")?;
    if value.contains(['\n', '\r']) {
        return Err("The executable path contains a newline.".into());
    }
    // Desktop Entry Exec quoting is distinct from shell quoting; % is a field-code escape.
    let mut escaped = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\\\\\"),
            '"' | '$' | '`' => {
                escaped.push_str("\\\\");
                escaped.push(ch);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    Ok(escaped)
}

fn install_shortcuts(settings: [bool; 2], restore: bool) -> Result<[bool; 2], String> {
    if !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop
            .split(':')
            .any(|part| part.eq_ignore_ascii_case("KDE"))
    }) || std::env::var_os("FLATPAK_ID").is_some()
        || std::env::var_os("SNAP").is_some()
    {
        return Err("Automatic launch shortcuts are available on native KDE installations. Assign taskmanager in your desktop's keyboard settings.".into());
    }
    let connection = Builder::session()
        .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build())
        .map_err(|error| error.to_string())?;
    let proxy =
        zbus::blocking::Proxy::new(&connection, ACCEL, "/kglobalaccel", "org.kde.KGlobalAccel")
            .map_err(|error| error.to_string())?;
    let action = [COMPONENT, "_launch", "Task Manager", "Launch Task Manager"];
    let known: Vec<Vec<String>> = proxy
        .call("allActionsForComponent", &(action,))
        .unwrap_or_default();
    let existing: Vec<i32> = proxy.call("shortcut", &(action,)).unwrap_or_default();
    let settings = if restore
        && known
            .iter()
            .any(|entry| entry.get(1).map(String::as_str) == Some("_launch"))
    {
        KEYS.map(|key| existing.contains(&key))
    } else {
        settings
    };
    // Check every requested key before creating defaults or changing the existing assignment.
    for (index, enabled) in settings.iter().enumerate() {
        if *enabled {
            let available: bool = proxy
                .call("isGlobalShortcutAvailable", &(KEYS[index], COMPONENT))
                .map_err(|error| error.to_string())?;
            if !available {
                let owner: Vec<String> = proxy.call("action", &(KEYS[index],)).unwrap_or_default();
                return Err(format!(
                    "{} is already assigned to {}. Change that assignment in KDE Keyboard settings first.",
                    ["Ctrl+Shift+Esc", "Ctrl+Alt+Delete"][index],
                    owner.get(3).map(String::as_str).unwrap_or("another action")
                ));
            }
        }
    }
    let directory = data_home()?.join("kglobalaccel");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(COMPONENT);
    let exec = desktop_exec(&std::env::current_exe().map_err(|error| error.to_string())?)?;
    let content = format!(
        "[Desktop Entry]\n# Managed by Task Manager\nType=Application\nName=Task Manager\nExec={exec}\nIcon={}\nStartupNotify=true\nX-KDE-GlobalAccel-CommandShortcut=true\n",
        super::APP_ID
    );
    if path.exists() {
        let old = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if !old.contains("# Managed by Task Manager\n") {
            return Err("The launch desktop file belongs to another program.".into());
        }
    }
    std::fs::write(&path, content).map_err(|error| error.to_string())?;
    proxy
        .call::<_, _, ()>("doRegister", &(action,))
        .map_err(|error| error.to_string())?;
    let keys: Vec<i32> = KEYS
        .into_iter()
        .zip(settings)
        .filter_map(|(key, enabled)| enabled.then_some(key))
        .collect();
    // SetPresent | NoAutoloading; only this component's launch action is changed.
    let applied: Vec<i32> = proxy
        .call(
            "setShortcut",
            &(action, keys.clone(), if restore { 2_u32 } else { 6_u32 }),
        )
        .map_err(|error| error.to_string())?;
    if keys.iter().any(|key| !applied.contains(key)) {
        return Err("KDE did not apply all requested shortcuts.".into());
    }
    Ok(KEYS.map(|key| applied.contains(&key)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_paths_are_desktop_entry_escaped() {
        assert_eq!(
            desktop_exec(Path::new("/opt/Task Manager/taskmanager")).unwrap(),
            "\"/opt/Task Manager/taskmanager\""
        );
        assert_eq!(
            desktop_exec(Path::new("/opt/100%/taskmanager")).unwrap(),
            "\"/opt/100%%/taskmanager\""
        );
        assert!(desktop_exec(Path::new("/tmp/bad\npath")).is_err());
    }

    #[test]
    #[ignore = "requires an isolated session bus; run with dbus-run-session"]
    fn second_instance_activates_the_existing_owner() {
        let owner = Instance::claim().unwrap().unwrap();
        assert!(Instance::claim().unwrap().is_none());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !owner.take_activation() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "requires KDE with Ctrl+Shift+Esc assigned to another application"]
    fn existing_desktop_shortcut_is_not_stolen() {
        let connection = Builder::session().unwrap().build().unwrap();
        let proxy =
            zbus::blocking::Proxy::new(&connection, ACCEL, "/kglobalaccel", "org.kde.KGlobalAccel")
                .unwrap();
        let before: Vec<String> = proxy.call("action", &(KEYS[0],)).unwrap();
        assert_ne!(before.first().map(String::as_str), Some(COMPONENT));
        assert!(
            install_shortcuts([true, false], false)
                .unwrap_err()
                .contains("already assigned")
        );
        let after: Vec<String> = proxy.call("action", &(KEYS[0],)).unwrap();
        assert_eq!(before, after);
    }
}
