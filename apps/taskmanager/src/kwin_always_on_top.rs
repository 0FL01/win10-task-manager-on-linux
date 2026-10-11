/* apps/taskmanager/src/kwin_always_on_top.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Native KWin scripting bridge. All files and IPC stay on the worker, not the UI thread.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mc_ui::{AlwaysOnTopError, AlwaysOnTopRequest, AlwaysOnTopResult};
use zbus::blocking::Connection;

const METHOD_TIMEOUT: Duration = Duration::from_secs(1);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const SCRIPTING: &str = "org.kde.kwin.Scripting";

pub struct Worker {
    requests: Option<SyncSender<AlwaysOnTopRequest>>,
    results: Receiver<AlwaysOnTopResult>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pending: Option<u64>,
}

impl Worker {
    pub fn new(ctx: egui::Context, app_id: &'static str) -> io::Result<Self> {
        let (requests, request_rx) = mpsc::sync_channel::<AlwaysOnTopRequest>(1);
        let (result_tx, results) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("taskmanager-kwin".into())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    if worker_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let (reply, cleanup) =
                        execute(request, app_id, std::process::id(), &worker_stop);
                    log::debug!(
                        "KWin always on top: window={}, actual={:?}, error={:?}",
                        reply.window_id,
                        reply.actual,
                        reply.error
                    );
                    let result = AlwaysOnTopResult {
                        id: request.id,
                        actual: reply.actual,
                        error: reply.error,
                    };
                    // Only one request can be outstanding, so the result slot cannot be full.
                    if result_tx.try_send(result).is_err() {
                        break;
                    }
                    ctx.request_repaint_after(Duration::from_millis(1));
                    if let Some(cleanup) = cleanup {
                        // Keep the lock while retiring an unconfirmed script, even after
                        // reporting the failure. The UI has disabled further requests.
                        cleanup.retire(&worker_stop);
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests: Some(requests),
            results,
            stop,
            thread: Some(thread),
            pending: None,
        })
    }

    pub fn submit(&mut self, request: AlwaysOnTopRequest) -> bool {
        let accepted = self
            .requests
            .as_ref()
            .is_some_and(|tx| tx.try_send(request).is_ok());
        if accepted {
            self.pending = Some(request.id);
        }
        accepted
    }

    pub fn try_result(&mut self) -> Option<AlwaysOnTopResult> {
        match self.results.try_recv() {
            Ok(result) => {
                self.pending = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending.take().map(|id| AlwaysOnTopResult {
                    id,
                    actual: None,
                    error: Some(AlwaysOnTopError::Failed),
                })
            }
        }
    }

    // Called during shell teardown only. In-flight IPC has finite deadlines; cancellation
    // interrupts the callback wait, then the worker unloads its script before it exits.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.requests.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug)]
struct Reply {
    actual: Option<bool>,
    error: Option<AlwaysOnTopError>,
    window_id: String,
}

struct Cleanup {
    _lock: File,
    connection: Connection,
    owner: String,
    plugin: String,
}

impl Cleanup {
    fn retire(self, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            if unload(&self.connection, &self.owner, &self.plugin).is_ok() {
                return;
            }
            // A restarted/exited compositor cannot retain the old script.
            let owner_exists: zbus::Result<bool> = self
                .connection
                .call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "NameHasOwner",
                    &(self.owner.as_str(),),
                )
                .and_then(|reply| reply.body().deserialize());
            if matches!(owner_exists, Ok(false)) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        // Teardown is cancellable even if KWin is unresponsive. A started script also
        // owns a self-retirement timer; never hang application exit on compositor IPC.
        let _ = unload(&self.connection, &self.owner, &self.plugin);
    }
}

impl Reply {
    fn failure(error: AlwaysOnTopError) -> Self {
        Self {
            actual: None,
            error: Some(error),
            window_id: String::new(),
        }
    }
}

struct Callback {
    owner: String,
    token: String,
    replies: SyncSender<Reply>,
}

#[zbus::interface(name = "io.github.taskmanager.TaskManager.AlwaysOnTop")]
impl Callback {
    fn report(
        &self,
        token: &str,
        status: &str,
        actual: bool,
        known: bool,
        window_id: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        log::debug!(
            "KWin script callback: sender={:?}, status={status}, known={known}",
            header.sender()
        );
        if header.sender().map(|name| name.as_str()) != Some(self.owner.as_str())
            || token != self.token
        {
            return Err(zbus::fdo::Error::AccessDenied(
                "Unexpected script reply".into(),
            ));
        }
        let error = match status {
            "ok" if known && !window_id.is_empty() => None,
            "rejected" if known && !window_id.is_empty() => Some(AlwaysOnTopError::Rejected),
            "not-found" if !known => Some(AlwaysOnTopError::NotFound),
            "ambiguous" if !known => Some(AlwaysOnTopError::Ambiguous),
            "failed" if !known => Some(AlwaysOnTopError::Failed),
            _ => {
                return Err(zbus::fdo::Error::InvalidArgs(
                    "Invalid script result".into(),
                ));
            }
        };
        // The first authenticated reply wins; a duplicate must never block the object server.
        let _ = self.replies.try_send(Reply {
            actual: known.then_some(actual),
            error,
            window_id: window_id.into(),
        });
        Ok(())
    }
}

fn session() -> zbus::Result<Connection> {
    let builder = zbus::connection::Builder::session()?.method_timeout(METHOD_TIMEOUT);
    async_io::block_on(futures_lite::future::or(builder.build(), async {
        async_io::Timer::after(METHOD_TIMEOUT).await;
        Err(io::Error::new(io::ErrorKind::TimedOut, "D-Bus connection timed out").into())
    }))
    .map(Connection::from)
}

fn dbus_failure(error: zbus::Error) -> Reply {
    log::warn!("KWin always on top: {error}");
    let category = match &error {
        zbus::Error::InputOutput(error) if error.kind() == io::ErrorKind::TimedOut => {
            AlwaysOnTopError::TimedOut
        }
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.UnknownInterface"
                    | "org.freedesktop.DBus.Error.UnknownMethod"
                    | "org.freedesktop.DBus.Error.AccessDenied"
            ) =>
        {
            AlwaysOnTopError::Unavailable
        }
        _ => AlwaysOnTopError::Failed,
    };
    Reply::failure(category)
}

fn script(app_id: &str, pid: u32, enabled: bool, destination: &str, plugin: &str) -> String {
    let parameters = serde_json::json!({
        "appId": app_id,
        "pid": pid,
        "enabled": enabled,
        "destination": destination,
        "plugin": plugin,
    });
    format!(
        "const request = {parameters};\n{}",
        include_str!("kwin_always_on_top.js")
    )
}

fn unload(connection: &Connection, owner: &str, plugin: &str) -> zbus::Result<()> {
    let _: bool = connection
        .call_method(
            Some(owner),
            "/Scripting",
            Some(SCRIPTING),
            "unloadScript",
            &(plugin,),
        )?
        .body()
        .deserialize()?;
    // `true` only schedules deleteLater. Wait for actual destruction before releasing
    // the inter-process lock: KWin allocates script ids from the current list length.
    let deadline = Instant::now() + METHOD_TIMEOUT;
    loop {
        let loaded: bool = connection
            .call_method(
                Some(owner),
                "/Scripting",
                Some(SCRIPTING),
                "isScriptLoaded",
                &(plugin,),
            )?
            .body()
            .deserialize()?;
        if !loaded {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(zbus::Error::Failure("KWin script is still loaded".into()));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn script_lock(deadline: Instant, stop: &AtomicBool) -> io::Result<File> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unavailable"))?;
    // Keep one empty runtime-only lock file. Unlinking it would let waiting processes
    // lock an obsolete inode while another process creates and locks a new one.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(runtime.join("taskmanager-always-on-top.lock"))?;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::Error(error)) => return Err(error),
            Err(TryLockError::WouldBlock) => {}
        }
        if stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Always-on-top request cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "KWin script lock timed out",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn execute(
    request: AlwaysOnTopRequest,
    app_id: &str,
    pid: u32,
    stop: &AtomicBool,
) -> (Reply, Option<Cleanup>) {
    // KWin 6.7 uses scripts.size() as the D-Bus object id. Overlapping temporary
    // scripts can collide after an out-of-order deletion, even across app instances.
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let lock = match script_lock(deadline, stop) {
        Ok(lock) => lock,
        Err(error) => return (dbus_failure(error.into()), None),
    };
    let connection = match session() {
        Ok(connection) => connection,
        Err(error) => return (dbus_failure(error), None),
    };
    let owner: String = match connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &("org.kde.KWin",),
        )
        .and_then(|reply| reply.body().deserialize())
    {
        Ok(owner) => owner,
        Err(error) => return (dbus_failure(error), None),
    };
    let mut file = match tempfile::Builder::new()
        .prefix("taskmanager-aot-")
        .suffix(".js")
        .tempfile()
    {
        Ok(file) => file,
        Err(error) => return (dbus_failure(error.into()), None),
    };
    let plugin = file
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let destination = connection.unique_name().unwrap().as_str();
    let (replies, reply_rx) = mpsc::sync_channel(1);
    let result = (|| -> zbus::Result<Reply> {
        connection.object_server().at(
            "/AlwaysOnTop",
            Callback {
                owner: owner.clone(),
                token: plugin.clone(),
                replies,
            },
        )?;
        file.write_all(script(app_id, pid, request.enabled, destination, &plugin).as_bytes())?;
        file.flush()?;
        if stop.load(Ordering::Relaxed) {
            return Ok(Reply::failure(AlwaysOnTopError::Failed));
        }
        let id: i32 = connection
            .call_method(
                Some(owner.as_str()),
                "/Scripting",
                Some(SCRIPTING),
                "loadScript",
                &(file.path().to_string_lossy().as_ref(), plugin.as_str()),
            )?
            .body()
            .deserialize()?;
        if id < 0 {
            return Ok(Reply::failure(AlwaysOnTopError::Failed));
        }
        log::debug!("Loaded KWin script {id} for PID {pid}");
        connection.call_method(
            Some(owner.as_str()),
            format!("/Scripting/Script{id}").as_str(),
            Some("org.kde.kwin.Script"),
            "run",
            &(),
        )?;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(Reply::failure(AlwaysOnTopError::Failed));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(Reply::failure(AlwaysOnTopError::TimedOut));
            }
            match reply_rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
                Ok(reply) => return Ok(reply),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(Reply::failure(AlwaysOnTopError::Failed));
                }
            }
        }
    })();
    let mut reply = result.unwrap_or_else(dbus_failure);
    // Even loadScript's reply can be lost after KWin loaded the script. Unload by our
    // unique name on every path, not just when we have received a numeric script id.
    if let Err(error) = unload(&connection, &owner, &plugin) {
        log::warn!("KWin always-on-top cleanup: {error}");
        reply.error = Some(AlwaysOnTopError::Cleanup);
        return (
            reply,
            Some(Cleanup {
                _lock: lock,
                connection,
                owner,
                plugin,
            }),
        );
    }
    (reply, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_are_json_not_executable_fragments() {
        let generated = script("app\"; throw Error();", 42, true, ":1.23", "plugin\"\\\n");
        let parameters: serde_json::Value = serde_json::from_str(
            generated
                .lines()
                .next()
                .unwrap()
                .strip_prefix("const request = ")
                .unwrap()
                .strip_suffix(';')
                .unwrap(),
        )
        .unwrap();
        assert_eq!(parameters["appId"], "app\"; throw Error();");
        assert_eq!(parameters["pid"], 42);
        assert_eq!(parameters["enabled"], true);
        assert_eq!(parameters["plugin"], "plugin\"\\\n");
    }

    #[test]
    fn connection_wait_is_bounded_without_a_session_bus() {
        // Test the same timeout race with a peer which never completes its handshake.
        use std::os::unix::net::UnixStream;
        let (client, _unresponsive_server) = UnixStream::pair().unwrap();
        let builder = zbus::connection::Builder::async_io_unix_stream(client);
        let result = async_io::block_on(futures_lite::future::or(builder.build(), async {
            async_io::Timer::after(Duration::from_millis(20)).await;
            Err(io::Error::new(io::ErrorKind::TimedOut, "handshake timed out").into())
        }));
        assert!(
            matches!(result, Err(zbus::Error::InputOutput(error)) if error.kind() == io::ErrorKind::TimedOut)
        );
    }

    /// Exercises the real script in KWin, not a mocked successful D-Bus `run` reply.
    #[test]
    #[ignore = "requires KDE Wayland and cargo build -p taskmanager-app"]
    fn native_kwin_toggle_and_instance_isolation() {
        use std::collections::HashMap;
        use std::process::{Child, Command, Stdio};
        let _ = env_logger::Builder::new()
            .filter_level(log::LevelFilter::Warn)
            .filter_module("taskmanager", log::LevelFilter::Debug)
            .try_init();

        struct Window(Child);
        impl Drop for Window {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("taskmanager");
        let first = Window(
            Command::new(&binary)
                .args(["--synthetic", "5", "--no-config"])
                .env("RUST_LOG", "warn")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let second = Window(
            Command::new(&binary)
                .args(["--synthetic", "5", "--no-config"])
                .env("RUST_LOG", "warn")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let stop = AtomicBool::new(false);
        let apply = |pid, enabled| {
            let (reply, cleanup) = execute(
                AlwaysOnTopRequest { id: 1, enabled },
                crate::APP_ID,
                pid,
                &stop,
            );
            assert!(cleanup.is_none(), "script cleanup must complete");
            assert_eq!(reply.error, None, "{reply:?}");
            assert_eq!(reply.actual, Some(enabled), "{reply:?}");
            reply.window_id
        };
        // Startup requests also wait for a not-yet-mapped window.
        let first_id = apply(first.0.id(), false);
        let second_id = apply(second.0.id(), false);
        let connection = session().unwrap();
        let above = |window_id: &str| {
            let info: HashMap<String, zbus::zvariant::OwnedValue> = connection
                .call_method(
                    Some("org.kde.KWin"),
                    "/KWin",
                    Some("org.kde.KWin"),
                    "getWindowInfo",
                    &(window_id,),
                )
                .unwrap()
                .body()
                .deserialize()
                .unwrap();
            bool::try_from(info.get("keepAbove").unwrap()).unwrap()
        };
        apply(first.0.id(), true);
        assert!(above(&first_id));
        assert!(!above(&second_id), "another instance must not be changed");
        apply(first.0.id(), false);
        assert!(!above(&first_id));

        // A no-match request is reported explicitly, never transport-success.
        let (missing, cleanup) = execute(
            AlwaysOnTopRequest {
                id: 2,
                enabled: true,
            },
            "io.github.taskmanager.NoSuchWindow",
            first.0.id(),
            &stop,
        );
        assert!(cleanup.is_none());
        assert_eq!(missing.error, Some(AlwaysOnTopError::NotFound));
    }
}
